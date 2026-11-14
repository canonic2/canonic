//! Private stdio indirection. Host entries carry an opaque random token.
use super::{Connection, Transport};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    process::{Command, ExitCode, Stdio},
};

#[cfg(windows)]
mod windows;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    pub(crate) token: String,
    digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    generation: Option<String>,
}

pub(crate) fn valid_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

impl Binding {
    pub(crate) fn new(connection: &Connection) -> Result<Self, crate::review::Error> {
        use ring::rand::{SecureRandom, SystemRandom};
        let mut bytes = [0_u8; 32];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| crate::review::Error::Invalid("private MCP token entropy unavailable"))?;
        let token = bytes
            .iter()
            .fold(String::with_capacity(64), |mut text, byte| {
                use std::fmt::Write;
                write!(&mut text, "{byte:02x}").expect("string write");
                text
            });
        Ok(Self {
            token,
            digest: connection.launch_digest(),
            generation: None,
        })
    }

    pub(crate) fn matches(&self, connection: &Connection) -> bool {
        valid_token(&self.token)
            && self.generation.as_deref().is_none_or(valid_token)
            && self.digest == connection.launch_digest()
    }

    /// Keep the locator usable by a host that retained its launch command, but
    /// change the host-entry identity so stale settings cannot match new policy.
    pub(crate) fn refreshed(&self, connection: &Connection) -> Result<Self, crate::review::Error> {
        if self.matches(connection) {
            return Ok(self.clone());
        }
        let mut next = Self::new(connection)?;
        if valid_token(&self.token) {
            next.generation = Some(next.token.clone());
            next.token.clone_from(&self.token);
        }
        Ok(next)
    }

    pub(crate) fn projected(
        &self,
        connection: &Connection,
        binary: &Path,
        root: &Path,
    ) -> Result<Transport, crate::review::Error> {
        if !self.matches(connection) {
            return Err(crate::review::Error::Invalid(
                "private MCP launch settings changed",
            ));
        }
        let mut transport = connection.projected(binary, root, Some(&self.token))?;
        if let (Some(generation), Transport::Stdio { env, .. }) = (&self.generation, &mut transport)
        {
            env.insert("SHIELD_MCP_GENERATION".into(), generation.clone());
        }
        Ok(transport)
    }
    pub(crate) fn portable_projected(
        &self,
        connection: &Connection,
        binary: &Path,
        root: &Path,
    ) -> Result<Transport, crate::review::Error> {
        let mut transport = self.projected(connection, binary, root)?;
        if connection.local
            && let Transport::Stdio { command, args, .. } = &mut transport
        {
            *command = "shield".into();
            args[3] = ".".into();
        }
        Ok(transport)
    }
}

pub(crate) fn private_state(root: &Path) -> Result<(), crate::review::Error> {
    let state = crate::layout::state(root);
    for path in [state.clone(), state.join("state.json")] {
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(crate::review::Error::Invalid(
                "private MCP state must not be linked",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(crate::review::Error::Invalid(
                    "MCP launch settings require private state permissions",
                ));
            }
        }
        #[cfg(windows)]
        crate::windows_security::check_private(&path)?;
        #[cfg(not(any(unix, windows)))]
        {
            return Err(crate::review::Error::Invalid(
                "private MCP launch permissions are unsupported on this platform",
            ));
        }
    }
    Ok(())
}

fn command(root: &Path, connection: &Connection, binding: &Binding) -> Result<Command, ()> {
    if !connection.enabled || !connection.local || !binding.matches(connection) {
        return Err(());
    }
    let Transport::Stdio { command, args, env } = &connection.transport else {
        return Err(());
    };
    private_state(root).map_err(|_| ())?;
    let mut child = Command::new(command);
    child
        .args(args)
        .envs(env)
        .current_dir(root)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    Ok(child)
}

pub(crate) fn run(input: &crate::cli::McpConnect) -> ExitCode {
    let result = (|| {
        let context = crate::review::open(&input.project).map_err(|_| ())?;
        crate::review::integrations::resume_mcp_launch(&context, &input.token).map_err(|_| ())?;
        let project = crate::runtime::Project::open(&input.project).map_err(|_| ())?;
        let snapshot = project.snapshot().map_err(|_| ())?;
        let (id, binding) = crate::review::integrations::resolve_mcp_token(
            snapshot.identity().project_root(),
            &input.token,
        )
        .map_err(|_| ())?;
        let connection = snapshot.mcp_connections().get(&id).ok_or(())?;
        let command = command(snapshot.identity().project_root(), connection, &binding)?;
        // Replace the launcher so the host's signals and stdio lifetime belong
        // directly to the connector; no detached child survives the launcher.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let _error = {
                let mut command = command;
                command.exec()
            };
        }
        #[cfg(windows)]
        return windows::run(command).map_err(|_| ());
        #[cfg(not(windows))]
        Err::<u8, ()>(())
    })();
    if let Ok(code) = result {
        ExitCode::from(code)
    } else {
        eprintln!(
            "Shield: this MCP launch is unavailable, removed or superseded. Inspect Shield's in-session status; confirmed publication resumes automatically."
        );
        ExitCode::from(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn portable_projection_keeps_nonlocal_stdio_arguments_unchanged() {
        let connection: Connection = serde_json::from_value(json!({"local":false,"transport":{
            "type":"stdio","command":"acme-connector","args":["short"]
        }}))
        .unwrap();
        let binding = Binding::new(&connection).unwrap();
        assert_eq!(
            binding
                .portable_projected(
                    &connection,
                    Path::new("/acme/shield"),
                    Path::new("/project")
                )
                .unwrap(),
            connection.transport
        );
    }

    #[test]
    fn connection_publication_locator_launches_only_the_refreshed_reviewed_transport() {
        let root = assert_fs::TempDir::new().unwrap();
        let bundle =
            crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None)
                .unwrap();
        crate::review::mcp::initialize_fixture(root.path(), &bundle);
        let mut connection:Connection=serde_json::from_value(json!({"local":true,"transport":{"type":"stdio","command":"acme-connector","args":["before"]}})).unwrap();
        let first = Binding::new(&connection).unwrap();
        let first_entry = first
            .projected(&connection, Path::new("/acme/shield"), root.path())
            .unwrap();
        let mut current = first.clone();
        for argument in ["after", "after-again"] {
            let Transport::Stdio { args, .. } = &mut connection.transport else {
                unreachable!()
            };
            *args = vec![argument.into()];
            assert!(command(root.path(), &connection, &current).is_err());
            let next = current.refreshed(&connection).unwrap();
            assert_eq!(next.token, first.token);
            assert_ne!(next.generation, current.generation);
            assert_ne!(
                next.projected(&connection, Path::new("/acme/shield"), root.path())
                    .unwrap(),
                first_entry
            );
            let launch = command(root.path(), &connection, &next).unwrap();
            assert_eq!(launch.get_args().collect::<Vec<_>>(), [argument]);
            assert_eq!(
                serde_json::to_value(next.refreshed(&connection).unwrap()).unwrap(),
                serde_json::to_value(&next).unwrap()
            );
            current = next;
        }
    }

    /// A project whose only configuration is an explicit file: the generated
    /// launcher names it, the CLI accepts that entry, and the launcher's own
    /// policy reload finds the reviewed connection there.
    #[test]
    fn launchers_carry_an_explicit_configuration_and_find_their_connection_in_it() {
        use clap::Parser as _;
        let root = assert_fs::TempDir::new().unwrap();
        let folder = root.path().canonicalize().unwrap();
        std::fs::create_dir_all(folder.join("policy")).unwrap();
        std::fs::write(
            folder.join("policy/acme.yml"),
            "schema_version: 1\npacks: []\n",
        )
        .unwrap();
        std::fs::write(
            folder.join("policy/config.local.yml"),
            "schema_version: 1\nmcp:\n  acme:\n    transport:\n      type: stdio\n      command: acme-connector\n      args: [serve]\n    agents:\n      claude: acme\n    enabled: true\n",
        )
        .unwrap();
        crate::layout::bind(
            &folder.join("policy/acme.yml"),
            crate::layout::Base::Invocation,
        );
        crate::review::open(&folder).unwrap();
        assert!(!folder.join(".shield/config.yml").exists());

        let snapshot = crate::runtime::Project::open(&folder)
            .unwrap()
            .snapshot()
            .unwrap();
        let connection = snapshot.mcp_connections().get("acme").unwrap();
        assert!(connection.local);
        let binding = Binding::new(connection).unwrap();
        let Transport::Stdio { args, .. } = binding
            .portable_projected(connection, Path::new("/acme/shield"), &folder)
            .unwrap()
        else {
            unreachable!()
        };
        assert_eq!(
            args,
            [
                "__internal",
                "mcp-connect",
                "--project",
                ".",
                "--token",
                binding.token.as_str(),
                "--config",
                "policy/acme.yml",
            ]
        );
        let cli = crate::cli::Cli::try_parse_from(std::iter::once("shield".to_owned()).chain(args))
            .unwrap();
        assert_eq!(cli.config.as_deref(), Some(Path::new("policy/acme.yml")));
        let launch = command(&folder, connection, &binding).unwrap();
        assert_eq!(launch.get_program(), "acme-connector");
        assert_eq!(launch.get_args().collect::<Vec<_>>(), ["serve"]);
    }

    #[test]
    fn mcp_connections_launcher_preserves_argv_and_refuses_stale_or_public_settings() {
        let root = assert_fs::TempDir::new().unwrap();
        let bundle =
            crate::configuration::Bundle::capture("schema_version: 1\npacks: []\n", &[], None)
                .unwrap();
        crate::review::mcp::initialize_fixture(root.path(), &bundle);
        let mut connection:Connection=serde_json::from_value(json!({"local":true,"transport":{"type":"stdio","command":"acme-connector","args":["two words","quoted \"value\""],"env":{"ACME_PROFILE":"acme-ro"}}})).unwrap();
        let binding = Binding::new(&connection).unwrap();
        assert_ne!(binding.token, Binding::new(&connection).unwrap().token);
        let launch = command(root.path(), &connection, &binding).unwrap();
        assert_eq!(launch.get_program(), "acme-connector");
        assert_eq!(
            launch.get_args().collect::<Vec<_>>(),
            ["two words", "quoted \"value\""]
        );
        assert!(
            launch
                .get_envs()
                .any(|(k, v)| k == "ACME_PROFILE" && v == Some(std::ffi::OsStr::new("acme-ro")))
        );
        let mut stale = binding.clone();
        stale.digest = "different-digest".into();
        assert!(command(root.path(), &connection, &stale).is_err());
        let original_transport = connection.transport.clone();
        if let Transport::Stdio { args, .. } = &mut connection.transport {
            args.push("changed".into());
        }
        assert!(command(root.path(), &connection, &binding).is_err());
        connection.transport = original_transport;
        connection.enabled = false;
        assert!(command(root.path(), &connection, &binding).is_err());
        connection.enabled = true;
        connection.local = false;
        assert!(command(root.path(), &connection, &binding).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                root.path().join(".shield/state"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
            assert!(private_state(root.path()).is_err());
        }
    }
}

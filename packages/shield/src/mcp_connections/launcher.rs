//! Private stdio indirection. Host entries carry an opaque random token.
use super::{Connection, Transport};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    process::{Command, ExitCode, Stdio},
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    pub(crate) token: String,
    digest: String,
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
        })
    }

    pub(crate) fn matches(&self, connection: &Connection) -> bool {
        valid_token(&self.token) && self.digest == connection.launch_digest()
    }
}

pub(crate) fn private_state(root: &Path) -> Result<(), crate::review::Error> {
    for path in [
        root.join(".shield/review"),
        root.join(".shield/review/state.json"),
    ] {
        let metadata = std::fs::symlink_metadata(path)?;
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
                    "MCP launch settings require private review-state permissions",
                ));
            }
        }
        #[cfg(not(unix))]
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
        let project = crate::runtime::Project::open(&input.project).map_err(|_| ())?;
        let snapshot = project.snapshot().map_err(|_| ())?;
        let (id, binding) = crate::review::integrations::resolve_mcp_token(
            snapshot.identity().project_root(),
            &input.token,
        )
        .map_err(|_| ())?;
        let connection = snapshot.mcp_connections().get(&id).ok_or(())?;
        let mut child = command(snapshot.identity().project_root(), connection, &binding)?;
        // Replace the launcher so the host's signals and stdio lifetime belong
        // directly to the connector; no detached child survives the launcher.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            let _error = child.exec();
        }
        Err::<u8, ()>(())
    })();
    if let Ok(code) = result {
        ExitCode::from(code)
    } else {
        eprintln!(
            "Shield: MCP connection is unavailable or changed; review setup in your terminal and restart the agent."
        );
        ExitCode::from(2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
                root.path().join(".shield/review"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
            assert!(private_state(root.path()).is_err());
        }
    }
}

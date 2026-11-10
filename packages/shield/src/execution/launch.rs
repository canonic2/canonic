//! One exact launch image shared by policy, approval and the native spawner.
use super::Error;
use crate::{policy, review::runtime::consumer::ResourceRequest};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EffectiveLaunch {
    version: u8,
    request: policy::Request,
    environment: BTreeMap<String, String>,
}
impl EffectiveLaunch {
    pub(super) fn prepare(submitted: &policy::Request) -> Result<Self, Error> {
        #[cfg(windows)]
        let supported = super::windows_binding::supported(submitted)
            || super::windows_git::application_directory(submitted).is_some()
                && super::git::supported(submitted);
        #[cfg(not(windows))]
        let supported = super::supported(submitted);
        if !supported {
            return Err(Error::Unsupported);
        }
        Ok(Self {
            version: 1,
            request: super::launch_request(submitted),
            environment: environment(submitted)?,
        })
    }
    pub(super) fn request(&self) -> &policy::Request {
        &self.request
    }
    pub(super) fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }
    pub(super) fn matches(&self, submitted: &policy::Request) -> bool {
        Self::prepare(submitted).is_ok_and(|expected| {
            serde_json::to_value(expected).ok() == serde_json::to_value(self).ok()
        })
    }
    pub(super) fn resource(&self, submitted: &policy::Request) -> ResourceRequest {
        ResourceRequest {
            mcp: None,
            input: policy::normalized::Input::Argv {
                executable: self.request.executable.clone(),
                arguments: self.request.arguments.clone(),
                original_input: json!(submitted),
            },
            context: policy::normalized::Context {
                cwd: self.request.cwd.clone(),
                environment: Some(self.environment.clone()),
                working_files: None,
            },
        }
    }
    #[cfg(all(test, not(windows)))]
    pub(super) fn process_fixture(request: &policy::Request) -> Result<Self, Error> {
        Ok(Self {
            version: 1,
            request: request.clone(),
            environment: environment(request)?,
        })
    }
}

#[cfg(windows)]
fn environment(request: &policy::Request) -> Result<BTreeMap<String, String>, Error> {
    super::windows_binding::environment_for(request)
}

#[cfg(not(windows))]
fn environment(request: &policy::Request) -> Result<BTreeMap<String, String>, Error> {
    let mut values = BTreeMap::from([("LC_ALL".into(), "C".into())]);
    if request.executable == super::git::EXECUTABLE {
        values.extend(
            [
                ("GIT_CONFIG_GLOBAL", "/dev/null"),
                ("GIT_CONFIG_SYSTEM", "/dev/null"),
                ("GIT_OPTIONAL_LOCKS", "0"),
                ("GIT_TERMINAL_PROMPT", "0"),
                ("GIT_NO_LAZY_FETCH", "1"),
                ("GIT_ATTR_NOSYSTEM", "1"),
                (
                    "GIT_ATTR_SOURCE",
                    "4b825dc642cb6eb9a060e54bf8d69288fbee4904",
                ),
            ]
            .map(|(key, value)| (key.into(), value.into())),
        );
        values.insert(
            "GIT_DIR".into(),
            std::path::Path::new(&request.project)
                .join(".git")
                .to_str()
                .ok_or(Error::Unsupported)?
                .into(),
        );
        values.insert("GIT_WORK_TREE".into(), request.project.clone());
    }
    Ok(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn git_policy_sees_exact_effective_arguments_and_cleared_environment() {
        let root = assert_fs::TempDir::new().unwrap();
        let project = root
            .path()
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let submitted = policy::Request {
            executable: super::super::git::EXECUTABLE.into(),
            arguments: vec!["status".into()],
            cwd: project.clone(),
            project,
        };
        let launch = EffectiveLaunch::prepare(&submitted).unwrap();
        assert_eq!(launch.request.arguments[0], "--no-pager");
        assert_eq!(
            launch.request.arguments.last().unwrap(),
            "--ignore-submodules=all"
        );
        assert!(!launch.environment.contains_key("PATH"));
        let rules=json!({"schema_version":1,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"],"rules":[{"id":"effective","decision":"allow","conditions":{"domain":"argv","executable":submitted.executable,"arguments":{"kind":"exact","values":launch.request.arguments},"environment":{"exact":launch.environment}}}]}).to_string();
        let bundle = crate::configuration::Bundle::capture(
            "schema_version: 1\npacks: []\nrules: rules.yml\n",
            &[],
            Some(&rules),
        )
        .unwrap();
        crate::review::mcp::initialize_fixture(root.path(), &bundle);
        let snapshot = crate::runtime::Project::open(root.path())
            .unwrap()
            .snapshot()
            .unwrap();
        let evidence = snapshot
            .evaluate_resource(launch.request.clone(), launch.resource(&submitted))
            .unwrap();
        assert_eq!(evidence.evaluation().decision, policy::Decision::Allow);
        assert_eq!(
            snapshot
                .evaluate(submitted.clone())
                .unwrap()
                .evaluation()
                .decision,
            policy::Decision::Ask
        );
        let resource = evidence.resource().unwrap();
        assert_eq!(json!(resource.input)["original_input"], json!(submitted));
        assert_eq!(
            resource.context.environment.as_ref().unwrap(),
            launch.environment()
        );
    }

    #[test]
    fn altered_launch_arguments_or_environment_cannot_keep_the_binding() {
        let project = crate::test_support::absolute("/workspace/acme").into_owned();
        #[cfg(not(windows))]
        let (executable, arguments) = ("/bin/echo".to_owned(), vec!["Acme text".into()]);
        #[cfg(windows)]
        let (executable, arguments) = (
            std::path::PathBuf::from(winsafe::GetSystemDirectory().unwrap())
                .join("timeout.exe")
                .to_str()
                .unwrap()
                .to_owned(),
            vec!["/t".into(), "0".into(), "/nobreak".into()],
        );
        let submitted = policy::Request {
            executable,
            arguments,
            cwd: project.clone(),
            project,
        };
        let launch = EffectiveLaunch::prepare(&submitted).unwrap();
        assert!(launch.matches(&submitted));
        let mut changed = launch.clone();
        changed.request.arguments.push("extra".into());
        assert!(!changed.matches(&submitted));
        let mut changed = launch.clone();
        changed.environment.insert("PATH".into(), ".".into());
        assert!(!changed.matches(&submitted));
        let mut changed = launch;
        changed.version = 0;
        assert!(!changed.matches(&submitted));
    }
}

//! Machine-local executable bindings captured before the ordinary policy review.
//! Discovery observes files only; it neither executes tools nor activates policy.
use crate::{
    policy,
    review::{Error, inspection::Settings},
};
use policy::model::variables::{Declaration, executables::Candidate};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub(in crate::review) struct Scope {
    writable: Vec<PathBuf>,
}

impl Scope {
    pub(in crate::review) fn for_candidate(
        project: &Path,
        candidate: &Value,
    ) -> Result<Self, Error> {
        let config = crate::configuration::layers::merged(candidate)?;
        let mut writable = crate::agents::temporary_candidates();
        writable.push(project.to_owned());
        for field in ["access_paths", "local_access_paths"] {
            writable.extend(
                config[field]["write"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(PathBuf::from),
            );
        }
        let resolved: Vec<_> = writable
            .iter()
            .filter_map(|path| path.canonicalize().ok())
            .collect();
        writable.extend(resolved);
        Ok(Self { writable })
    }

    pub(super) fn permits(&self, path: &Path) -> bool {
        path.is_absolute()
            && path.canonicalize().is_ok_and(|resolved| {
                !self
                    .writable
                    .iter()
                    .any(|root| path.starts_with(root) || resolved.starts_with(root))
            })
    }
}

pub(super) fn executable(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn find(
    declaration: &Declaration,
    home: Option<&Path>,
    env: &dyn Fn(&str) -> Option<PathBuf>,
    scope: &Scope,
) -> Option<Value> {
    paths(declaration, home, env).find_map(|path| {
        if !scope.permits(&path) || !executable(&path) {
            return None;
        }
        // Preserve a rustup proxy's spelling: resolving its symlink would bind
        // rustup instead of cargo/rustc/rustfmt and change argument semantics.
        let value = json!(path.to_str()?);
        declaration.check(&value).ok()?;
        Some(value)
    })
}

fn paths<'a>(
    declaration: &'a Declaration,
    home: Option<&'a Path>,
    env: &'a dyn Fn(&str) -> Option<PathBuf>,
) -> impl Iterator<Item = PathBuf> + 'a {
    declaration
        .discover
        .iter()
        .flatten()
        .filter_map(move |candidate| match candidate {
            Candidate::Home { home: suffix } => Some(home?.join(suffix)),
            Candidate::Environment { env: name, path } => Some(env(name)?.join(path)),
        })
}

pub(super) fn declares_path(declaration: &Declaration, path: &Path) -> bool {
    let home = std::env::home_dir();
    paths(declaration, home.as_deref(), &|name| {
        std::env::var_os(name).map(PathBuf::from)
    })
    .any(|candidate| candidate == path)
}

pub(in crate::review) fn settings(
    artifact: &str,
    settings: &mut Settings,
    scope: &Scope,
) -> Result<bool, Error> {
    let home = std::env::home_dir();
    bind(artifact, settings, home.as_deref(), scope, &|name| {
        std::env::var_os(name).map(PathBuf::from)
    })
}

fn bind(
    artifact: &str,
    settings: &mut Settings,
    home: Option<&Path>,
    scope: &Scope,
    env: &dyn Fn(&str) -> Option<PathBuf>,
) -> Result<bool, Error> {
    let effective = settings.effective();
    let mut discovered = std::collections::BTreeMap::new();
    for (name, declaration) in policy::ordered_declarations(artifact)? {
        if effective.variables.contains_key(&name)
            || settings
                .local
                .as_ref()
                .is_some_and(|overlay| overlay.unset.contains(&name))
        {
            continue;
        }
        if let Some(value) = find(&declaration, home, env, scope) {
            discovered.insert(name, value);
        }
    }
    if discovered.is_empty() {
        return Ok(false);
    }
    settings.edit(true, |binding| {
        binding.variables.extend(discovered);
        Ok(())
    })?;
    Ok(true)
}

pub(super) fn candidate(
    project: &Path,
    candidate: Value,
    mut meta: Value,
    selected: Option<usize>,
) -> Result<(Value, Value), Error> {
    let scope = Scope::for_candidate(project, &candidate)?;
    let artifacts: Vec<String> = serde_json::from_value(candidate["packs"].clone())
        .map_err(|_| Error::Invalid("pack artifacts missing"))?;
    let rows = meta["installed"]
        .as_array_mut()
        .ok_or(Error::Invalid("installed packs missing"))?;
    let mut changed = false;
    for (index, row) in rows.iter_mut().enumerate() {
        if selected.is_some_and(|selected| selected != index) {
            continue;
        }
        let mut binding = crate::review::inspection::selection_settings(&candidate, index)?;
        if settings(&artifacts[index], &mut binding, &scope)? {
            super::bind(row, &binding);
            changed = true;
        }
    }
    if !changed {
        return Ok((candidate, meta));
    }
    let candidate = super::rebuild(&candidate, rows, &artifacts, None)?;
    super::unbind(rows);
    Ok((candidate, meta))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bind(
        artifact: &str,
        settings: &mut Settings,
        home: Option<&Path>,
        env: &dyn Fn(&str) -> Option<PathBuf>,
    ) -> Result<bool, Error> {
        super::bind(artifact, settings, home, &Scope { writable: vec![] }, env)
    }

    fn artifact() -> String {
        json!({"schema_version":1,"kind":"pack","id":"acme/rust","version":"1.0.0",
            "description":"Acme","license":"MIT","maintainers":["Acme"],
            "engines":{"shield":env!("CARGO_PKG_VERSION")},
            "capabilities":["structured-rules-v1","argument-rules-v1","variables-v1","executable-discovery-v1"],
            "variables":{"cargo_executable":{"type":"string","description":"Acme cargo","layer":"local",
                "discover":[{"env":"CARGO_HOME","path":"bin/cargo"},{"home":".cargo/bin/cargo"}]}},
            "rules":[{"id":"build","conditions":{"domain":"argv","executable":{"var":"cargo_executable"},
                "arguments":{"kind":"exact","values":["build"]}},"decision":"allow"}]
        }).to_string()
    }

    fn empty() -> Settings {
        Settings {
            shared: policy::Binding::default(),
            local: Some(crate::configuration::layers::Overlay::default()),
        }
    }

    #[cfg(unix)]
    fn proxy(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn executable_discovery_preserves_proxy_spelling_and_local_scope() {
        let temp = assert_fs::TempDir::new().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let rustup = home.join(".cargo/bin/rustup");
        let cargo = home.join(".cargo/bin/cargo");
        proxy(&rustup);
        std::os::unix::fs::symlink("rustup", &cargo).unwrap();
        let mut settings = empty();
        assert!(bind(&artifact(), &mut settings, Some(&home), &|_| None).unwrap());
        assert!(settings.shared.variables.is_empty());
        assert_eq!(
            settings.local.as_ref().unwrap().variables["cargo_executable"],
            json!(cargo)
        );
        assert!(!bind(&artifact(), &mut settings, Some(&home), &|_| None).unwrap());
        // A different executable named cargo is never selected by filename.
        let other = home.join("project/cargo");
        proxy(&other);
        std::fs::remove_file(&cargo).unwrap();
        assert!(!bind(&artifact(), &mut empty(), Some(&home), &|_| None).unwrap());
    }

    #[test]
    #[cfg(unix)]
    fn executable_discovery_prefers_cargo_home_and_preserves_answers() {
        let temp = assert_fs::TempDir::new().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let custom = home.join("custom");
        proxy(&home.join(".cargo/bin/cargo"));
        proxy(&custom.join("bin/cargo"));
        let mut settings = empty();
        let env = |name: &str| (name == "CARGO_HOME").then(|| custom.clone());
        bind(&artifact(), &mut settings, Some(&home), &env).unwrap();
        assert_eq!(
            settings.effective().variables["cargo_executable"],
            json!(custom.join("bin/cargo"))
        );
        let saved = settings.clone();
        assert!(!bind(&artifact(), &mut settings, Some(&home), &|_| None).unwrap());
        assert!(settings == saved);
        let mut opted_out = empty();
        opted_out
            .local
            .as_mut()
            .unwrap()
            .unset
            .push("cargo_executable".into());
        assert!(!bind(&artifact(), &mut opted_out, Some(&home), &env).unwrap());
    }

    #[test]
    #[cfg(unix)]
    fn executable_discovery_ignores_nonexecutables_and_directories() {
        let temp = assert_fs::TempDir::new().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let cargo = home.join(".cargo/bin/cargo");
        std::fs::create_dir_all(cargo.parent().unwrap()).unwrap();
        std::fs::write(&cargo, "Acme").unwrap();
        assert!(!bind(&artifact(), &mut empty(), Some(&home), &|_| None).unwrap());
        std::fs::remove_file(&cargo).unwrap();
        std::fs::create_dir(&cargo).unwrap();
        assert!(!bind(&artifact(), &mut empty(), Some(&home), &|_| None).unwrap());
    }

    #[test]
    #[cfg(unix)]
    fn discovery_excludes_writable_locations_and_symlinks_into_them() {
        let fixture = assert_fs::TempDir::new().unwrap();
        let root = fixture.path().canonicalize().unwrap();
        let project = root.join("project");
        let approved = root.join("approved");
        let temp = root.join("temporary");
        let stable = root.join("stable");
        let scope = Scope {
            writable: vec![project.clone(), approved.clone(), temp.clone()],
        };
        for path in [&project, &approved, &temp] {
            let executable = path.join("bin/cargo");
            proxy(&executable);
            assert!(!scope.permits(&executable));
            let mut setup = empty();
            assert!(
                !super::bind(&artifact(), &mut setup, None, &scope, &|_| Some(
                    path.clone()
                ))
                .unwrap()
            );
        }
        let executable = stable.join("bin/cargo");
        proxy(&executable);
        assert!(scope.permits(&executable));
        std::fs::remove_file(&executable).unwrap();
        std::os::unix::fs::symlink(project.join("bin/cargo"), &executable).unwrap();
        assert!(!scope.permits(&executable));
    }

    #[test]
    fn executable_discovery_admission_requires_local_bindings_and_capability() {
        let source = artifact();
        policy::Pack::from_yaml(&source).unwrap();
        let mut bounds: Value = policy::parse(&source).unwrap();
        for count in [0, 17] {
            bounds["variables"]["cargo_executable"]["discover"] =
                json!(vec![json!({"home":"bin/cargo"}); count]);
            assert!(
                policy::Pack::from_yaml(&bounds.to_string()).is_err(),
                "{count}"
            );
        }
        let original: Value = serde_json::from_str(&source).unwrap();
        for change in ["shared", "parent", "capability"] {
            let mut pack = original.clone();
            match change {
                "shared" => pack["variables"]["cargo_executable"]["layer"] = json!("project"),
                "parent" => {
                    pack["variables"]["cargo_executable"]["discover"][1]["home"] =
                        json!("../cargo");
                }
                _ => pack["capabilities"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|value| value != "executable-discovery-v1"),
            }
            assert!(
                policy::Pack::from_yaml(&pack.to_string()).is_err(),
                "{change}"
            );
        }
    }
}

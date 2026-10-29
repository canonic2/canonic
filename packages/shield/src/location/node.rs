//! Recognizes Node's fixed, single-file syntax-check form without executing it.
use std::path::Path;

pub(super) fn check_file<'a>(executable: &str, arguments: &[&'a str]) -> Option<&'a str> {
    let path = Path::new(executable);
    if executable != "node"
        && !(path.file_name().is_some_and(|name| name == "node")
            && path
                .parent()
                .is_some_and(|parent| [Path::new("/bin"), Path::new("/usr/bin")].contains(&parent)))
    {
        return None;
    }
    let (flag, file, separated) = match arguments {
        [flag, file] => (*flag, *file, false),
        [flag, "--", file] => (*flag, *file, true),
        _ => return None,
    };
    (["--check", "-c"].contains(&flag)
        && !file.is_empty()
        && file != "-"
        && (separated || !file.starts_with('-')))
    .then_some(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_node_basename_outside_system_folders_does_not_establish_syntax_checking() {
        for executable in ["node", "/bin/node", "/usr/bin/node"] {
            assert_eq!(
                check_file(executable, &["--check", "acme.js"]),
                Some("acme.js")
            );
        }
        for executable in [
            "./node",
            "tools/node",
            "/workspace/acme/tools/node",
            "/usr/bin/../local/bin/node",
        ] {
            assert_eq!(check_file(executable, &["--check", "acme.js"]), None);
        }
    }
}

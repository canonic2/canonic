//! Real file bindings reject replacement and malformed normalized authority claims.
use super::*;
use assert_fs::prelude::*;

#[test]
#[cfg(unix)]
fn external_text_reads_and_approval_checks_bind_the_confirmed_root_identity() {
    let dir = assert_fs::TempDir::new().unwrap();
    let parent = dir.path().canonicalize().unwrap();
    let project = parent.join("project");
    let read = parent.join("read");
    let outside = parent.join("outside");
    for root in [&project, &read, &outside] {
        fs::create_dir(root).unwrap();
        fs::write(root.join("notes.txt"), "Acme").unwrap();
    }
    let alias = parent.join("alias");
    std::os::unix::fs::symlink(&read, &alias).unwrap();
    let paths = crate::configuration::AccessPaths {
        read: vec![alias.to_str().unwrap().into()],
        write: vec![],
    };
    let path = read.join("notes.txt");
    assert!(Binding::open(&project, &project, path.to_str().unwrap()).is_err());
    let binding = Binding::open_scoped(&project, &project, path.to_str().unwrap(), &paths).unwrap();
    binding.verify().unwrap();
    let request = policy::Request {
        executable: "shield:read-text".into(),
        arguments: vec![path.to_str().unwrap().into()],
        cwd: project.to_str().unwrap().into(),
        project: project.to_str().unwrap().into(),
    };
    crate::enforcement::read_text::verify_approval_read(&request, &binding.digest, &paths).unwrap();
    assert!(
        Binding::open_scoped(
            &project,
            &project,
            outside.join("notes.txt").to_str().unwrap(),
            &paths
        )
        .is_err()
    );
    fs::create_dir(read.join(".shield")).unwrap();
    fs::write(read.join(".shield/state.txt"), "Acme private state").unwrap();
    assert!(
        Binding::open_scoped(
            &project,
            &project,
            read.join(".shield/state.txt").to_str().unwrap(),
            &paths
        )
        .is_err()
    );
    fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(&outside, &alias).unwrap();
    assert!(binding.verify().is_err());
    assert!(
        crate::enforcement::read_text::verify_approval_read(&request, &binding.digest, &paths)
            .is_err()
    );
    assert_eq!(fs::read_to_string(path).unwrap(), "Acme");
}

#[test]
#[cfg(unix)]
fn binding_refuses_inode_replacement_changes_and_parent_symlinks() {
    let root = assert_fs::TempDir::new().unwrap();
    root.child("acme.txt").write_str("Acme text").unwrap();
    let canonical = root.path().canonicalize().unwrap();
    let path = canonical.join("acme.txt");
    let binding = Binding::open(&canonical, &canonical, path.to_str().unwrap()).unwrap();
    binding.verify().unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    fs::write(&path, "Other txt").unwrap();
    fs::File::open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert!(
        binding.verify().is_err(),
        "content changes with restored mtime must fail"
    );
    fs::rename(&path, canonical.join("old.txt")).unwrap();
    fs::write(&path, "Acme replacement").unwrap();
    assert!(binding.verify().is_err());
    let binding = Binding::open(&canonical, &canonical, path.to_str().unwrap()).unwrap();
    fs::write(&path, "Changed text").unwrap();
    assert!(binding.verify().is_err());
    #[cfg(unix)]
    {
        let directory = canonical.join("nested");
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("read.txt"), "Acme text").unwrap();
        let nested = directory.join("read.txt");
        let binding = Binding::open(&canonical, &directory, nested.to_str().unwrap()).unwrap();
        fs::rename(&directory, canonical.join("replacement")).unwrap();
        std::os::unix::fs::symlink(canonical.join("replacement"), &directory).unwrap();
        assert!(binding.verify().is_err());
        assert!(Binding::open(&canonical, &directory, nested.to_str().unwrap()).is_err());
    }
}

#[test]
fn approval_read_arguments_admit_only_canonical_windows() {
    let mut request = policy::Request {
        executable: "shield:read-text".into(),
        arguments: vec!["/acme/a.txt".into()],
        cwd: "/acme".into(),
        project: "/acme".into(),
    };
    assert!(crate::enforcement::read_text::valid_request(&request));
    for tail in [vec!["--limit", "1"], vec!["--offset", "2", "--limit", "3"]] {
        request.arguments = std::iter::once("/acme/a.txt")
            .chain(tail)
            .map(str::to_owned)
            .collect();
        assert!(crate::enforcement::read_text::valid_request(&request));
    }
    for tail in [
        vec!["--approved", "true"],
        vec!["--limit", "0"],
        vec!["--offset", "01"],
        vec!["--offset"],
        vec!["--limit", "1", "--offset", "2"],
    ] {
        request.arguments = std::iter::once("/acme/a.txt")
            .chain(tail)
            .map(str::to_owned)
            .collect();
        assert!(!crate::enforcement::read_text::valid_request(&request));
    }
}

#[test]
fn shell_file_and_web_requests_without_native_context_fail_closed() {
    let root = assert_fs::TempDir::new().unwrap();
    let rules = json!({"schema_version":1,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"],
        "rules":[{"id":"cat","decision":"allow","conditions":{"domain":"argv","executable":"cat","arguments":{"kind":"any"}}}]})
    .to_string();
    let bundle = crate::configuration::Bundle::capture(
        "schema_version: 1\npacks: []\nrules: rules.yml\n",
        &[],
        Some(&rules),
    )
    .unwrap();
    let context = crate::review::mcp::initialize_fixture(root.path(), &bundle);
    let project = Path::new(&context.project_root);
    for operation in [
        json!({"kind":"shell","source":"cat .shield/review/state.json","dialect":"bash"}),
        json!({"kind":"file","action":"read","path":project.join(".shield/review/state.json"),"recursive":false}),
        json!({"kind":"web","action":"fetch","url":"https://example.com/","query":null}),
    ] {
        let input = json!({"protocol_version":1,"request_id":"acme","expected_project_id":context.project_id,
            "cwd":project,"operation":operation});
        let (value, code) = evaluate(project, &serde_json::to_vec(&input).unwrap());
        assert_eq!(code, 0, "{value}");
        assert_eq!(value["decision"], "deny", "{value}");
        assert_eq!(value["code"], "unsupported-operation", "{value}");
    }
}

#[test]
fn a_briefly_locked_runtime_has_its_own_retryable_code() {
    assert_eq!(
        runtime_failure(&crate::runtime::Error::Locked),
        "runtime-busy"
    );
    assert_eq!(
        handoff_failure(&Error::Runtime(crate::runtime::Error::Locked)),
        "runtime-busy"
    );
    assert_eq!(
        runtime_failure(&crate::runtime::Error::Missing),
        "runtime-unavailable"
    );
    assert_eq!(handoff_failure(&Error::Denied), "handoff-failed");
}

#[test]
fn duplicate_keys_unknown_fields_and_oversized_input_fail_without_enrollment() {
    let root = assert_fs::TempDir::new().unwrap();
    for bytes in [
        br#"{"protocol_version":1,"protocol_version":1,"request_id":"acme","cwd":"/","operation":{"kind":"register"}}"#.as_slice(),
        br#"{"protocol_version":1,"request_id":"acme","cwd":"/","approved":true,"operation":{"kind":"register"}}"#,
        br#"{"protocol_version":1,"request_id":"acme","cwd":"/","operation":{"kind":"read-text","path":"/a.txt","path":"/b.txt"}}"#,
    ] {
        let (value, code) = evaluate(root.path(), bytes);
        assert_eq!(code, 2);
        assert_eq!(value["decision"], "deny");
        assert_eq!(value["code"], "invalid-input");
    }
    let (value, code) = evaluate(root.path(), &vec![b' '; MAX_INPUT + 1]);
    assert_eq!(code, 2);
    assert_eq!(value["code"], "invalid-input");
    assert!(!root.path().join(".shield").exists());
}

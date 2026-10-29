//! Exercises config-driven profiles, scoped overrides, retained artifacts, and revisions.

use super::*;
use crate::{configuration::Candidate, policy::Profile};
use serde_json::{Value, json};

fn snapshot(selections: &[(&str, &str, Profile, &str)], custom: Option<&str>) -> Snapshot {
    let packs: Vec<_> = selections.iter().map(|(id,version,_,operation)| {
        json!({"schema_version":1,"id":id,"version":version,"description":"Acme","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["argument-rules-v1","structured-rules-v1"],"rules":[{"id":"operation","reason":"Acme operation","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":[operation]}},"decision":{"loose":"allow","recommended":"ask","strict":"deny"}}],"kind":"pack"}).to_string()
    }).collect();
    let mut config = json!({"schema_version":1,"packs":selections.iter().map(|(id,version,profile,_)|json!({"id":id,"version":version,"profile":profile,"path":format!("packs/{}.yml",id.split('/').next_back().expect("pack name"))})).collect::<Vec<_>>()});
    if custom.is_some() {
        config["rules"] = json!("rules.yml");
    }
    Candidate::from_yaml(&config.to_string())
        .expect("valid config")
        .capture(&packs, custom)
        .expect("valid captured artifacts")
}

fn custom(_scope: Scope, decision: Decision) -> String {
    json!({"schema_version":1,"rules":[{"id":"same-local-id","decision":decision,"reason":"Acme custom operation","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["write"]}}}],"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]}).to_string()
}

fn request(operation: &str) -> Request {
    let context = std::env::current_dir()
        .expect("current directory")
        .to_str()
        .expect("UTF-8 context")
        .to_owned();
    Request {
        executable: "acme".into(),
        arguments: vec![operation.into()],
        cwd: context.clone(),
        project: context,
    }
}

#[test]
fn prepared_evaluator_preserves_all_candidate_evidence_across_requests() {
    let inputs = vec![(
        Scope::Project,
        snapshot(
            &[("acme/test", "0.1.0", Profile::Loose, "write")],
            Some(&custom(Scope::Project, Decision::Allow)),
        ),
    )];
    let expected: Vec<_> = ["write", "read", "absent"]
        .iter()
        .map(|operation| value(&evaluate(&inputs, &request(operation)).unwrap()))
        .collect();
    let evaluator = Evaluator::new(inputs).unwrap();
    for _ in 0..3 {
        for (operation, expected) in ["write", "read", "absent"].iter().zip(&expected) {
            assert_eq!(
                value(&evaluator.evaluate(&request(operation)).unwrap()),
                *expected
            );
        }
    }
    let mut invalid = request("write");
    invalid.cwd = "relative".into();
    assert!(evaluator.evaluate(&invalid).is_err());
    let retained = evaluator.evaluate(&request("write")).unwrap();
    drop(evaluator);
    assert_eq!(value(&retained), expected[0]);
    assert!(Evaluator::new(vec![]).is_err());
}

fn value(result: &CandidateEvaluation) -> Value {
    serde_json::to_value(result).expect("candidate result JSON")
}

fn approved_root_evaluator(dir: &assert_fs::TempDir) -> Evaluator {
    for name in ["project", "reference", "scratch", "outside"] {
        std::fs::create_dir(dir.path().join(name)).unwrap();
        std::fs::write(dir.path().join(name).join("item"), "Acme").unwrap();
        std::fs::create_dir(dir.path().join(name).join(".git")).unwrap();
    }
    let reference = dir.path().join("reference").canonicalize().unwrap();
    let scratch = dir.path().join("scratch").canonicalize().unwrap();
    let rows = vec![
        json!({"id":"read","decision":"allow","conditions":{"domain":"argv","executable":{"one_of":["cat","head","tail","wc","ls"]},"arguments":{"kind":"any"}},"requirements":{"targets":{"action":"read","all":{"within":{"root":"read"}}}},"on_unmet":{"decision":"deny"}}),
        json!({"id":"remove","decision":"allow","conditions":{"domain":"argv","executable":"rm","arguments":{"kind":"any"}},"requirements":{"targets":{"action":"remove","all":{"within":{"root":"write"},"exclude":[{"root":"write","path":".git"}],"match":{"regex":".+"}}}},"on_unmet":{"decision":"deny"}}),
    ];
    let document = crate::policy::rules::document(&rows).unwrap().to_string();
    let config = json!({"schema_version":1,"packs":[],"rules":"rules.yml","access_paths":{"read":[reference],"write":[scratch]}}).to_string();
    Evaluator::new(vec![(
        Scope::Project,
        Candidate::from_yaml(&config)
            .unwrap()
            .capture(&[], Some(&document))
            .unwrap(),
    )])
    .unwrap()
}

#[test]
fn approved_roots_bind_reads_removals_exclusions_and_compiled_requests() {
    let dir = assert_fs::TempDir::new().unwrap();
    let evaluator = approved_root_evaluator(&dir);
    let project = dir.path().join("project").canonicalize().unwrap();
    let reference = dir.path().join("reference").canonicalize().unwrap();
    let scratch = dir.path().join("scratch").canonicalize().unwrap();
    let cached =
        Evaluator::from_authenticated_compiled(evaluator.compiled_bytes().to_vec()).unwrap();
    let mut request = Request {
        executable: "rm".into(),
        arguments: vec![],
        cwd: project.to_str().unwrap().into(),
        project: project.to_str().unwrap().into(),
    };
    for (exe, args, decision) in [
        ("rm", vec!["item".to_owned()], Decision::Allow),
        (
            "rm",
            vec![scratch.join("item").to_str().unwrap().into()],
            Decision::Allow,
        ),
        (
            "rm",
            vec![reference.join("item").to_str().unwrap().into()],
            Decision::Deny,
        ),
        (
            "rm",
            vec![dir.path().join("outside/item").to_str().unwrap().into()],
            Decision::Deny,
        ),
        (
            "rm",
            vec![
                "item".into(),
                reference.join("item").to_str().unwrap().into(),
            ],
            Decision::Deny,
        ),
        ("rm", vec![scratch.to_str().unwrap().into()], Decision::Deny),
        ("rm", vec!["-r".into(), ".".into()], Decision::Deny),
        (
            "rm",
            vec![scratch.join(".git").to_str().unwrap().into()],
            Decision::Deny,
        ),
        (
            "rm",
            vec!["--unknown".into(), "item".into()],
            Decision::Deny,
        ),
        (
            "cat",
            vec![reference.join("item").to_str().unwrap().into()],
            Decision::Allow,
        ),
        (
            "cat",
            vec![scratch.join("item").to_str().unwrap().into()],
            Decision::Allow,
        ),
        (
            "cat",
            vec![dir.path().join("outside/item").to_str().unwrap().into()],
            Decision::Deny,
        ),
        (
            "head",
            vec!["-n".into(), "2".into(), "item".into()],
            Decision::Allow,
        ),
        ("tail", vec!["-F".into(), "item".into()], Decision::Deny),
        ("wc", vec!["-l".into(), "item".into()], Decision::Allow),
        ("ls", vec!["-la".into()], Decision::Allow),
        ("ls", vec!["-R".into()], Decision::Deny),
    ] {
        request.executable = exe.into();
        request.arguments = args;
        for engine in [&evaluator, &cached] {
            let result = engine.evaluate(&request).unwrap();
            assert_eq!(result.decision, decision, "{} {:?}", exe, request.arguments);
            assert_ne!(
                value(&result)["evaluation"]["matches"]
                    .as_array()
                    .unwrap()
                    .len(),
                0
            );
        }
    }
    std::fs::remove_dir_all(&scratch).unwrap();
    request.executable = "rm".into();
    request.arguments = vec!["item".into()];
    assert_eq!(cached.evaluate(&request).unwrap().decision, Decision::Deny);
    assert!(project.join("item").exists());
    assert!(reference.join("item").exists());
}

#[test]
fn head_tail_count_forms_bind_read_targets() {
    let dir = assert_fs::TempDir::new().unwrap();
    let evaluator = approved_root_evaluator(&dir);
    let project = dir.path().join("project").canonicalize().unwrap();
    let outside = dir.path().join("outside/item");
    for (exe, args, decision) in [
        ("head", vec!["-2", "item"], Decision::Allow),
        ("tail", vec!["-2", "item"], Decision::Allow),
        ("tail", vec!["-n", "+2", "item"], Decision::Allow),
        ("tail", vec!["-n+2", "item"], Decision::Allow),
        ("head", vec!["-n", "+2", "item"], Decision::Deny),
        (
            "head",
            vec!["-2", outside.to_str().unwrap()],
            Decision::Deny,
        ),
        ("head", vec!["-q", "-2", "item"], Decision::Deny),
        ("cat", vec!["-2", "item"], Decision::Deny),
    ] {
        let request = Request {
            executable: exe.into(),
            arguments: args.iter().map(|&a| a.into()).collect(),
            cwd: project.to_str().unwrap().into(),
            project: project.to_str().unwrap().into(),
        };
        assert_eq!(
            evaluator.evaluate(&request).unwrap().decision,
            decision,
            "{exe} {args:?}"
        );
    }
}

#[test]
#[cfg(unix)]
fn approved_root_reads_follow_links_but_removal_preserves_the_leaf() {
    let dir = assert_fs::TempDir::new().unwrap();
    let evaluator = approved_root_evaluator(&dir);
    let cached =
        Evaluator::from_authenticated_compiled(evaluator.compiled_bytes().to_vec()).unwrap();
    let project = dir.path().join("project").canonicalize().unwrap();
    std::os::unix::fs::symlink(dir.path().join("outside"), project.join("escape")).unwrap();
    let mut request = Request {
        executable: "cat".into(),
        arguments: vec!["escape/item".into()],
        cwd: project.to_str().unwrap().into(),
        project: project.to_str().unwrap().into(),
    };
    assert_eq!(cached.evaluate(&request).unwrap().decision, Decision::Deny);
    request.executable = "rm".into();
    request.arguments = vec!["escape".into()];
    assert_eq!(cached.evaluate(&request).unwrap().decision, Decision::Allow);
    assert!(project.join("escape").is_symlink());
}

#[test]
fn outside_checks_the_whole_read_union_and_exact_checks_each_member() {
    let dir = assert_fs::TempDir::new().unwrap();
    let _ = approved_root_evaluator(&dir);
    let project = dir.path().join("project").canonicalize().unwrap();
    let read = dir.path().join("reference").canonicalize().unwrap();
    let write = dir.path().join("scratch").canonicalize().unwrap();
    let rows = vec![
        json!({"id":"outside","decision":"deny","conditions":{"domain":"file","action":"read","sources":{"all":{"outside":{"root":"read"}}}}}),
        json!({"id":"exact","decision":"allow","conditions":{"domain":"file","action":"read","sources":{"all":{"exact":{"root":"read","path":"item"}}}}}),
    ];
    let document = crate::policy::rules::document(&rows).unwrap();
    assert!(
        document["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("access-paths-v1"))
    );
    let config = json!({"schema_version":1,"packs":[],"rules":"rules.yml","access_paths":{"read":[read],"write":[write]}}).to_string();
    let evaluator = Evaluator::new(vec![(
        Scope::Project,
        Candidate::from_yaml(&config)
            .unwrap()
            .capture(&[], Some(&document.to_string()))
            .unwrap(),
    )])
    .unwrap();
    let request = Request {
        executable: "file".into(),
        arguments: vec![],
        cwd: project.to_str().unwrap().into(),
        project: project.to_str().unwrap().into(),
    };
    let context = policy::normalized::Context {
        working_files: None,
        cwd: request.cwd.clone(),
        environment: None,
    };
    for (name, decision) in [
        ("project", Decision::Allow),
        ("reference", Decision::Allow),
        ("scratch", Decision::Allow),
        ("outside", Decision::Deny),
    ] {
        let input = policy::normalized::Input::File {
            action: "read".into(),
            sources: vec![dir.path().join(name).join("item").to_str().unwrap().into()],
            destination: None,
            recursive: false,
            original_input: json!({}),
        };
        assert_eq!(
            evaluator
                .evaluate_resource(&request, &input, &context, &|| Ok(()))
                .unwrap()
                .decision,
            decision
        );
    }
}

#[test]
fn independent_pack_profiles_and_nonmatching_abstention_use_shared_evaluator() {
    let inputs = [(
        Scope::Project,
        snapshot(
            &[
                ("acme/first", "0.1.0", Profile::Loose, "write"),
                ("acme/second", "0.1.0", Profile::Strict, "other"),
            ],
            None,
        ),
    )];
    assert_eq!(
        evaluate(&inputs, &request("write")).unwrap().decision,
        Decision::Allow
    );
    assert_eq!(
        evaluate(&inputs, &request("other")).unwrap().decision,
        Decision::Deny
    );
    assert_eq!(
        evaluate(&inputs, &request("unknown")).unwrap().decision,
        Decision::Ask
    );
    let result = value(&evaluate(&inputs, &request("write")).unwrap());
    assert_eq!(result["evaluation"]["packs"][0]["profile"], "loose");
    assert_eq!(result["evaluation"]["packs"][1]["profile"], "strict");
    assert_eq!(result["trust"], "unverified");
}

#[test]
fn captured_input_and_lock_cannot_be_changed_by_mutating_source_buffers() {
    let config = Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.yml\n").unwrap();
    let mut bytes = custom(Scope::Project, Decision::Allow);
    let captured = config.capture(&[], Some(&bytes)).unwrap();
    let revision = captured.lock().revision().unwrap();
    bytes = custom(Scope::Project, Decision::Deny);
    let inputs = [(Scope::Project, captured)];
    assert_eq!(
        evaluate(&inputs, &request("write")).unwrap().decision,
        Decision::Allow
    );
    assert_eq!(revision, inputs[0].1.lock().revision().unwrap());
    assert_ne!(
        revision,
        config
            .capture(&[], Some(&bytes))
            .unwrap()
            .lock()
            .revision()
            .unwrap()
    );
    let mut invalid = request("write");
    invalid.cwd = "relative".into();
    assert!(evaluate(&inputs, &invalid).is_err());
}

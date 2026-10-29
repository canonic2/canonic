use super::*;

#[test]
fn pagers_require_command_policy_even_when_their_input_is_readable() {
    use crate::{configuration::Candidate, policy::Scope};
    let root = assert_fs::TempDir::new().unwrap();
    std::fs::write(root.path().join("report.txt"), "Acme report").unwrap();
    let cwd = root.path().to_str().unwrap();
    let executables = json!({"one_of":["less", "more", "/bin/less", "/usr/bin/less", "/bin/more", "/usr/bin/more"]});
    for yielding in [false, true] {
        let mut capabilities = vec!["location-v1", "requirements-v1", "structured-rules-v1"];
        let mut rules = vec![json!({"id":"read", "decision":"allow",
            "conditions":{"domain":"file", "action":"read"},
            "requirements":{"sources":{"all":{"within":{"root":"project"}}}},
            "on_unmet":{"decision":"deny", "message":"Read inside the project."}})];
        if yielding {
            capabilities.extend(["argument-rules-v1", "fallback-rules-v1"]);
            rules.push(json!({"id":"pager-fallback", "decision":"ask", "fallback":true,
                "conditions":{"domain":"argv", "executable":executables, "arguments":{"kind":"any"}}}));
        }
        let pack = json!({"schema_version":1, "kind":"pack", "id":"acme/read", "version":"1.0.0",
            "description":"Acme file inspection", "license":"MIT", "maintainers":["Acme"],
            "engines":{"shield":env!("CARGO_PKG_VERSION")},
            "capabilities":capabilities,
            "rules":rules})
        .to_string();
        for decision in [None, Some("allow"), Some("ask"), Some("deny")] {
            let custom = decision.map(|decision| {
                json!({"schema_version":1, "kind":"custom",
                "capabilities":["argument-rules-v1", "structured-rules-v1"],
                "rules":[{"id":"pager", "decision":decision, "conditions":{
                    "domain":"argv", "executable":executables, "arguments":{"kind":"any"}}}]})
                .to_string()
            });
            let mut config = json!({"schema_version":1, "packs":[{"id":"acme/read", "version":"1.0.0", "profile":"recommended", "path":"read.yml"}]});
            if custom.is_some() {
                config["rules"] = json!("rules.yml");
            }
            let candidate = Candidate::from_yaml(&config.to_string())
                .unwrap()
                .capture(std::slice::from_ref(&pack), custom.as_deref())
                .unwrap();
            let evaluator = Evaluator::new(vec![(Scope::Project, candidate)]).unwrap();
            // The read rule works for a modeled filter; it grants no pager authority.
            assert_eq!(
                analyze("sed -n 1p report.txt", Dialect::Bash, cwd, cwd, &evaluator).unwrap()["decision"],
                "allow"
            );
            for source in [
                "less report.txt",
                "less -F report.txt",
                "more report.txt",
                "/bin/less report.txt",
                "/usr/bin/less report.txt",
                "/bin/more report.txt",
                "/usr/bin/more report.txt",
                "LESSOPEN='|./pager-helper %s' less report.txt",
                "LESSCLOSE='./pager-cleanup %s %s' less report.txt",
                "LESS='+!./pager-helper' less report.txt",
                "MORE='+!./pager-helper' more report.txt",
                "LESSOPEN='|./pager-helper %s' less report.txt | sed -n 1p",
            ] {
                let result = analyze(source, Dialect::Bash, cwd, cwd, &evaluator).unwrap();
                let expected = decision.unwrap_or("ask");
                // Assignments independently retain environment review, even
                // when a command rule allows the pager itself.
                let assigned = source.starts_with("LESS") || source.starts_with("MORE");
                let shell_expected = if assigned && expected == "allow" {
                    "ask"
                } else {
                    expected
                };
                assert_eq!(result["decision"], shell_expected, "{source}: {result}");
                assert!(
                    result["operations"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|op| {
                            op["request"]["executable"].as_str().is_some_and(|name| {
                                name.ends_with("less") || name.ends_with("more")
                            })
                        })
                        .all(|op| op["policy_request"].is_null() && op["decision"] == expected),
                    "{result}"
                );
            }
        }
    }
}

#[test]
fn jq_file_filters_follow_read_policy_and_keep_explicit_refusals() {
    use crate::{configuration::Candidate, policy::Scope};
    let root = assert_fs::TempDir::new().unwrap();
    std::fs::write(root.path().join("report.json"), "{}").unwrap();
    std::fs::create_dir(root.path().join(".aws")).unwrap();
    std::fs::write(root.path().join(".aws/credentials"), "{}").unwrap();
    let cwd = root.path().to_str().unwrap();
    let pack = json!({"schema_version":1,"kind":"pack","id":"acme/baseline","version":"1.0.0",
        "description":"Acme text inspection","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},
        "capabilities":["access-paths-v1","argument-rules-v1","fallback-rules-v1","location-v1","requirements-v1","structured-rules-v1"],"rules":[
            {"id":"jq-fallback","decision":"ask","fallback":true,"conditions":{"domain":"argv","executable":"jq","arguments":{"kind":"any"}}},
            {"id":"read","decision":"allow","conditions":{"domain":"file","action":"read"},
                "requirements":{"targets":{"action":"read","all":{"within":{"root":"read"},"exclude":[{"root":"read","path":".aws"}]}}},
                "on_unmet":{"decision":"deny","message":"Read only approved files."}}
        ]}).to_string();
    for refusal in [None, Some("ask"), Some("deny")] {
        let custom = refusal.map(|decision| json!({"schema_version":1,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"],
            "rules":[{"id":"explicit","decision":decision,"conditions":{"domain":"argv","executable":"jq","arguments":{"kind":"any"}}}]}).to_string());
        let mut config = json!({"schema_version":1,"packs":[{"id":"acme/baseline","version":"1.0.0","profile":"recommended","path":"baseline.yml"}]});
        if custom.is_some() {
            config["rules"] = json!("rules.yml");
        }
        let candidate = Candidate::from_yaml(&config.to_string())
            .unwrap()
            .capture(std::slice::from_ref(&pack), custom.as_deref())
            .unwrap();
        let evaluator = Evaluator::new(vec![(Scope::Project, candidate)]).unwrap();
        for source in [
            r#"jq -r '.data | "desc: \(.description)\nprofiles: \(.profiles)\nrules: \(.rules|length)\nrule keys: \(.rules[0]|keys)"' report.json"#,
            r#"jq -r '.data.rules[] | select(.id|test("npm")) | [.id, (.profile_decisions|tostring), .reason[0:90]] | @tsv' report.json"#,
            r#"jq -c '.data.rules[] | select((tostring|test("npm|script";"i"))) | {id, decision, pack: (.pack // .source // null), conditions}' report.json"#,
        ] {
            let result = analyze(source, Dialect::Bash, cwd, cwd, &evaluator).unwrap();
            assert_eq!(result["decision"], refusal.unwrap_or("allow"), "{result}");
        }
        if refusal.is_none() {
            for source in [
                "jq env report.json",
                "jq -f program.jq report.json",
                "jq -r 'inputs' report.json",
                "jq '{id, value: env}' report.json",
                "jq '{id, value: inputs}' report.json",
            ] {
                assert_eq!(
                    analyze(source, Dialect::Bash, cwd, cwd, &evaluator).unwrap()["decision"],
                    "ask",
                    "{source}"
                );
            }
            assert_eq!(
                analyze(
                    "jq -r '.data | length' .aws/credentials",
                    Dialect::Bash,
                    cwd,
                    cwd,
                    &evaluator
                )
                .unwrap()["decision"],
                "deny"
            );
        }
    }
}

#[test]
fn node_syntax_checks_use_file_policy_without_overriding_explicit_refusals() {
    use crate::{configuration::Candidate, policy::Scope};
    let root = assert_fs::TempDir::new().unwrap();
    std::fs::write(
        root.path().join("acme.js"),
        "throw new Error('must not execute');",
    )
    .unwrap();
    let cwd = root.path().to_str().unwrap();
    let pack = json!({"schema_version":1,"kind":"pack","id":"acme/baseline","version":"1.0.0","description":"Acme syntax fixture","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["argument-rules-v1","fallback-rules-v1","location-v1","requirements-v1","structured-rules-v1"],"rules":[
        {"id":"node-fallback","decision":"ask","fallback":true,"conditions":{"domain":"argv","executable":"node","arguments":{"kind":"any"}}},
        {"id":"read","decision":"allow","conditions":{"domain":"file","action":"read"},"requirements":{"sources":{"all":{"within":{"root":"project"}}}},"on_unmet":{"decision":"deny","message":"Read inside the project."}}
    ]}).to_string();
    for refusal in [None, Some("allow"), Some("ask"), Some("deny")] {
        let custom = refusal.map(|decision| json!({"schema_version":1,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"],"rules":[{"id":"node-explicit","decision":decision,"conditions":{"domain":"argv","executable":"node","arguments":{"kind":"any"}}}]}).to_string());
        let mut config = json!({"schema_version":1,"packs":[{"id":"acme/baseline","version":"1.0.0","profile":"recommended","path":"baseline.yml"}]});
        if custom.is_some() {
            config["rules"] = json!("rules.yml");
        }
        let captured = Candidate::from_yaml(&config.to_string())
            .unwrap()
            .capture(std::slice::from_ref(&pack), custom.as_deref())
            .unwrap();
        let evaluator = Evaluator::new(vec![(Scope::Project, captured)]).unwrap();
        let result = analyze("node --check acme.js", Dialect::Posix, cwd, cwd, &evaluator).unwrap();
        assert_eq!(result["decision"], refusal.unwrap_or("allow"), "{result}");
        assert_eq!(result["coverage"], "single-literal-invocation", "{result}");
        if refusal.is_none() {
            let impostor = root.path().join("tools/node");
            let source = format!("'{}' --check acme.js", impostor.display());
            assert_eq!(
                analyze(&source, Dialect::Posix, cwd, cwd, &evaluator).unwrap()["decision"],
                "ask"
            );
            assert_eq!(result["operations"][0]["request"]["executable"], "node");
            assert_eq!(result["operations"][0]["policy_request"]["kind"], "file");
            for source in [
                "node acme.js",
                "node --check",
                "node --check -",
                "node --check --require ./preload.cjs acme.js",
                "node --check acme.js extra.js",
                "node --require ./preload.cjs --check acme.js",
                "node --check --import ./preload.mjs acme.js",
                "node --check --env-file=.env acme.js",
            ] {
                assert_eq!(
                    analyze(source, Dialect::Posix, cwd, cwd, &evaluator).unwrap()["decision"],
                    "ask",
                    "{source}"
                );
            }
            let outside = assert_fs::TempDir::new().unwrap();
            let file = outside.path().join("outside.js");
            std::fs::write(&file, "const outside = true;").unwrap();
            assert_eq!(
                analyze(
                    &format!("node --check '{}'", file.display()),
                    Dialect::Posix,
                    cwd,
                    cwd,
                    &evaluator
                )
                .unwrap()["decision"],
                "deny"
            );
        }
    }
}

fn http_client_evaluator(fetch: &Value, explicit: Option<&str>) -> Evaluator {
    use crate::{configuration::Candidate, policy::Scope};
    let clients = json!({"one_of":["curl","wget","/usr/bin/curl","/usr/bin/wget"]});
    let pack = json!({"schema_version":1,"kind":"pack","id":"acme/baseline","version":"1.0.0","description":"Acme download fixture","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["argument-rules-v1","fallback-rules-v1","location-v1","requirements-v1","structured-rules-v1"],"rules":[
        {"id":"client-fallback","decision":"ask","fallback":true,"conditions":{"domain":"argv","executable":clients,"arguments":{"kind":"any"}}},
        {"id":"registry","decision":"allow","conditions":fetch},
        {"id":"blocked","decision":"deny","conditions":{"domain":"web","action":"fetch","url":{"schemes":["https"],"hosts":["blocked.example.com"]}}},
        {"id":"write","decision":"allow","conditions":{"domain":"file","action":"write"},"requirements":{"destination":{"within":{"root":"project"}}},"on_unmet":{"decision":"deny","message":"Write inside the project."}}
    ]}).to_string();
    let custom = explicit.map(|decision| json!({"schema_version":1,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"],"rules":[{"id":"explicit","decision":decision,"conditions":{"domain":"argv","executable":clients,"arguments":{"kind":"any"}}}]}).to_string());
    let mut config = json!({"schema_version":1,"packs":[{"id":"acme/baseline","version":"1.0.0","profile":"recommended","path":"baseline.yml"}]});
    if custom.is_some() {
        config["rules"] = json!("rules.yml");
    }
    let captured = Candidate::from_yaml(&config.to_string())
        .unwrap()
        .capture(std::slice::from_ref(&pack), custom.as_deref())
        .unwrap();
    Evaluator::new(vec![(Scope::Project, captured)]).unwrap()
}

const REGISTRY_FETCH: &str = r#"{"domain":"web","action":"fetch","url":{"schemes":["https"],"hosts":["registry.example.com"]}}"#;

#[test]
fn recognized_downloads_use_web_and_file_policy_without_overriding_explicit_rules() {
    let root = assert_fs::TempDir::new().unwrap();
    let cwd = root.path().to_str().unwrap();
    let fetch: Value = serde_json::from_str(REGISTRY_FETCH).unwrap();
    for explicit in [None, Some("allow"), Some("ask"), Some("deny")] {
        let evaluator = http_client_evaluator(&fetch, explicit);
        let download = "curl -fsS -o catalog.json https://registry.example.com/catalog.json";
        let result = analyze(download, Dialect::Posix, cwd, cwd, &evaluator).unwrap();
        assert_eq!(result["decision"], explicit.unwrap_or("allow"), "{result}");
        if explicit.is_some() {
            assert!(
                result["operations"][0]["policy_request"].is_null(),
                "{result}"
            );
            continue;
        }
        assert_eq!(result["operations"][0]["policy_request"]["kind"], "web");
        assert_eq!(
            result["operations"][0]["policy_request"]["conditions"],
            json!({"client":"shell","redirects":"none"})
        );
        let outside = assert_fs::TempDir::new().unwrap();
        for (source, expected, decided_by) in [
            (
                "wget --no-hsts -qO- https://registry.example.com/catalog.json",
                "allow",
                "web",
            ),
            ("curl -sI https://registry.example.com/", "allow", "web"),
            (
                "curl -fsS https://other.example.com/catalog.json",
                "ask",
                "web",
            ),
            (
                "curl -fsS -o catalog.json https://blocked.example.com/",
                "deny",
                "web",
            ),
            (
                &format!(
                    "curl -fsS -o '{}' https://registry.example.com/catalog.json",
                    outside.path().join("catalog.json").display()
                ),
                "deny",
                "file",
            ),
        ] {
            let result = analyze(source, Dialect::Posix, cwd, cwd, &evaluator).unwrap();
            assert_eq!(result["decision"], expected, "{source}: {result}");
            assert_eq!(
                result["operations"][0]["policy_request"]["kind"], decided_by,
                "{source}"
            );
        }
        for source in [
            "curl -d @notes.txt https://registry.example.com/",
            "curl -H Authorization:x https://registry.example.com/",
            "wget --no-hsts https://registry.example.com/catalog.json",
            "wget --no-hsts --no-netrc --spider -O catalog.json https://registry.example.com/",
            "wget --no-hsts --no-netrc -qOcatalog.json --spider https://registry.example.com/",
            // The HSTS database in the home folder is read and rewritten.
            "wget -qO- https://registry.example.com/catalog.json",
            "https_proxy=http://proxy.example.com curl -fsS https://registry.example.com/",
            "export CURL_HOME=/srv/acme; curl -fsS https://registry.example.com/",
            "/opt/acme/bin/curl -fsS https://registry.example.com/",
            // Wrappers retain Ask and unknown inherited environment, even for
            // a child command that would be a recognized fetch on its own.
            "timeout 10 curl -fsS https://registry.example.com/",
            "sh -c 'curl -fsS https://registry.example.com/'",
        ] {
            let result = analyze(source, Dialect::Posix, cwd, cwd, &evaluator).unwrap();
            assert_eq!(result["decision"], "ask", "{source}: {result}");
        }
    }
}

#[test]
fn download_rules_can_name_their_client_and_redirect_behavior() {
    let root = assert_fs::TempDir::new().unwrap();
    let cwd = root.path().to_str().unwrap();
    let mut fetch: Value = serde_json::from_str(REGISTRY_FETCH).unwrap();
    fetch["request_fields"] = json!({"/client":{"exact":"shell"},"/redirects":{"exact":"none"}});
    let evaluator = http_client_evaluator(&fetch, None);
    for (source, expected) in [
        ("curl -fsS https://registry.example.com/", "allow"),
        ("curl -fsSL https://registry.example.com/", "ask"),
        ("wget --no-hsts -qO- https://registry.example.com/", "ask"),
        (
            "wget --no-hsts --max-redirect=0 -qO- https://registry.example.com/",
            "allow",
        ),
    ] {
        let result = analyze(source, Dialect::Posix, cwd, cwd, &evaluator).unwrap();
        assert_eq!(result["decision"], expected, "{source}: {result}");
    }
    fetch["request_fields"] = json!({"/client":{"exact":"agent"}});
    let agents_only = http_client_evaluator(&fetch, None);
    let result = analyze(
        "curl -fsS https://registry.example.com/",
        Dialect::Posix,
        cwd,
        cwd,
        &agents_only,
    )
    .unwrap();
    assert_eq!(result["decision"], "ask", "{result}");
}

#[test]
fn downloads_ask_when_reported_or_observed_configuration_can_change_them() {
    let root = assert_fs::TempDir::new().unwrap();
    let cwd = root.path().to_str().unwrap();
    let fetch: Value = serde_json::from_str(REGISTRY_FETCH).unwrap();
    let evaluator = http_client_evaluator(&fetch, None);
    let download = "curl -fsS https://registry.example.com/";
    let proxy = BTreeMap::from([(
        "HTTPS_PROXY".to_owned(),
        "http://proxy.example.com".to_owned(),
    )]);
    let reported = analyze_controlled(
        download,
        Dialect::Posix,
        cwd,
        Some(&proxy),
        cwd,
        &evaluator,
        &|| Ok(()),
    )
    .unwrap();
    assert_eq!(reported["decision"], "ask", "{reported}");

    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let observed = |source: &str, home: Option<&std::path::Path>| {
        let scope = crate::location::WorkingFiles {
            home: home.map(std::path::Path::to_path_buf),
            ..Default::default()
        };
        analyze_with_working_files(
            source,
            Dialect::Posix,
            cwd,
            None,
            cwd,
            &evaluator,
            &|| Ok(()),
            Some(&scope),
        )
        .unwrap()["decision"]
            .clone()
    };
    assert_eq!(observed(download, Some(&home)), "allow");
    assert_eq!(observed(download, None), "ask");
    // curl -q reads no configuration, so an unknown home folder cannot change it.
    assert_eq!(
        observed("curl -q -fsS https://registry.example.com/", None),
        "allow"
    );
    std::fs::write(home.join(".curlrc"), "upload-file = notes.txt").unwrap();
    assert_eq!(observed(download, Some(&home)), "ask");
    assert_eq!(
        observed("curl -q -fsS https://registry.example.com/", Some(&home)),
        "allow"
    );
    let wget = "wget --no-hsts -qO- https://registry.example.com/";
    let without_netrc = "wget --no-hsts --no-netrc -qO- https://registry.example.com/";
    assert_eq!(observed(wget, Some(&home)), "allow");
    // wget sends ~/.netrc credentials when a server asks unless --no-netrc;
    // curl reads them only with -n, which is outside the subset.
    std::fs::write(
        home.join(".netrc"),
        "machine registry.example.com login acme",
    )
    .unwrap();
    assert_eq!(observed(wget, Some(&home)), "ask");
    assert_eq!(observed(without_netrc, Some(&home)), "allow");
    assert_eq!(
        observed("curl -q -fsS https://registry.example.com/", Some(&home)),
        "allow"
    );
    let netrc = BTreeMap::from([("NETRC".to_owned(), "/srv/acme/netrc".to_owned())]);
    let reported = analyze_controlled(
        without_netrc,
        Dialect::Posix,
        cwd,
        Some(&netrc),
        cwd,
        &evaluator,
        &|| Ok(()),
    )
    .unwrap();
    assert_eq!(reported["decision"], "ask", "{reported}");
    std::fs::write(home.join(".wgetrc"), "post_data = x").unwrap();
    assert_eq!(observed(without_netrc, Some(&home)), "ask");
}

#[test]
fn standalone_test_brackets_are_literal_without_admitting_pathname_patterns() {
    for dialect in [Dialect::Posix, Dialect::Bash] {
        assert_eq!(
            literal_words("[ -f AGENTS.md ]", dialect).unwrap(),
            Some(vec![
                "[".into(),
                "-f".into(),
                "AGENTS.md".into(),
                "]".into()
            ])
        );
        for source in ["[ab]", "[[]", "[]]", "[a-z]", "[*]", "[?]", "[\\]]"] {
            assert!(
                literal_words(source, dialect).unwrap().is_none(),
                "{source}"
            );
        }
        assert_eq!(preview("[ -f AGENTS.md ]; acme write")["decision"], "deny");
    }
}

#[test]
fn embedded_revision_tildes_are_literal_while_home_and_assignment_tildes_stay_unresolved() {
    for dialect in [Dialect::Posix, Dialect::Bash] {
        assert_eq!(
            words::literal_words("acme HEAD~3 'HEAD'~2 x/y~1", dialect).unwrap(),
            Some(vec![
                "acme".into(),
                "HEAD~3".into(),
                "HEAD~2".into(),
                "x/y~1".into()
            ])
        );
        for source in [
            "acme ~",
            "acme ~/path",
            "acme ~someone",
            "acme X=~",
            "X=~/path acme",
        ] {
            assert!(
                words::literal_words(source, dialect).unwrap().is_none(),
                "{source}"
            );
        }
    }
}

#[test]
#[cfg(unix)]
fn actual_shells_keep_revision_tildes_literal() {
    for shell in ["/bin/sh", "/bin/bash"] {
        let output = std::process::Command::new(shell)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .args(["-c", "printf '%s\\n' HEAD~3 'HEAD'~2 x/y~1"])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"HEAD~3\nHEAD~2\nx/y~1\n");
    }
}

#[test]
fn declared_directory_options_use_command_policy_and_preserve_raw_refusals() {
    use crate::{configuration::Candidate, policy::Scope};
    let mut document = json!({"schema_version":1,"kind":"pack","id":"acme/git","version":"1.0.0","description":"Acme Git fixture","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["argument-rules-v1","directory-options-v1","fallback-rules-v1","structured-rules-v1"],"rules":[
        {"id":"status","decision":"allow","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"prefix","values":["status"]}}},
        {"id":"reset","decision":"deny","conditions":{"domain":"argv","executable":"git","arguments":{"kind":"prefix","values":["reset"]}}},
        {"id":"unmodeled","decision":"ask","fallback":true,"conditions":{"domain":"argv","executable":"git","arguments":{"kind":"any"}}}
    ],"directory_options":[{"executable":"git","option":"-C","repeat":"chain","paths":"follow"}]});
    let pack = document.to_string();
    // Without the declaration, core knows no Git option.
    document["capabilities"] = json!([
        "argument-rules-v1",
        "fallback-rules-v1",
        "structured-rules-v1"
    ]);
    document
        .as_object_mut()
        .unwrap()
        .remove("directory_options");
    let undeclared = Candidate::from_yaml(
        &json!({"schema_version":1,"packs":[{"id":"acme/git","version":"1.0.0","profile":"recommended","path":"git.yml"}]}).to_string(),
    )
    .unwrap()
    .capture(&[document.to_string()], None)
    .unwrap();
    let undeclared = Evaluator::new(vec![(Scope::Project, undeclared)]).unwrap();
    assert_eq!(
        analyze(
            "git -C /acme/project status",
            Dialect::Posix,
            "/acme/project",
            "/acme/project",
            &undeclared
        )
        .unwrap()["decision"],
        "ask"
    );
    for refusal in [None, Some("ask"), Some("deny")] {
        let custom = refusal.map(|decision| json!({"schema_version":1,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"],"rules":[{"id":"raw-context","decision":decision,"conditions":{"domain":"argv","executable":"git","arguments":{"kind":"prefix","values":["-C"]}}}]}).to_string());
        let mut config = json!({"schema_version":1,"packs":[{"id":"acme/git","version":"1.0.0","profile":"recommended","path":"git.yml"}]});
        if custom.is_some() {
            config["rules"] = json!("rules.yml");
        }
        let captured = Candidate::from_yaml(&config.to_string())
            .unwrap()
            .capture(std::slice::from_ref(&pack), custom.as_deref())
            .unwrap();
        let evaluator = Evaluator::new(vec![(Scope::Project, captured)]).unwrap();
        let result = analyze(
            "git -C /acme/project status",
            Dialect::Posix,
            "/acme/project",
            "/acme/project",
            &evaluator,
        )
        .unwrap();
        assert_eq!(result["decision"], refusal.unwrap_or("allow"), "{result}");
        if refusal.is_none() {
            assert_eq!(
                result["operations"][0]["policy_request"]["arguments"],
                json!(["status"])
            );
            assert_eq!(
                analyze(
                    "git -C /acme/project reset --hard",
                    Dialect::Posix,
                    "/acme/project",
                    "/acme/project",
                    &evaluator
                )
                .unwrap()["decision"],
                "deny"
            );
            // Another folder is matched as `cd` there would be: this status
            // rule has no folder limit, so it allows there too.
            let elsewhere = analyze(
                "git -C ../elsewhere status",
                Dialect::Posix,
                "/acme/project",
                "/acme/project",
                &evaluator,
            )
            .unwrap();
            assert_eq!(elsewhere["decision"], "allow", "{elsewhere}");
            assert_eq!(
                elsewhere["operations"][0]["policy_request"]["cwd"],
                "/acme/project/../elsewhere"
            );
        }
    }
}

#[test]
fn a_project_declared_directory_option_lets_one_project_rule_cover_every_form() {
    use crate::{configuration::Candidate, policy::Scope};
    let root = assert_fs::TempDir::new().unwrap();
    let project = root.path().canonicalize().unwrap();
    let outside = project.parent().unwrap().to_str().unwrap().to_owned();
    let project = project.to_str().unwrap();
    let rule = json!({"id":"npm-start","decision":"allow","reason":"Acme start",
        "conditions":{"domain":"argv","executable":"npm","arguments":{"kind":"exact","values":["start"]},
            "cwd":{"within":{"root":"project"}}}});
    let declaration =
        json!({"executable":"npm","option":"--prefix","position":"anywhere","paths":"follow"});
    let custom = crate::policy::rules::document_with(&[rule], &[declaration])
        .unwrap()
        .to_string();
    let captured = Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.json\n")
        .unwrap()
        .capture(&[], Some(&custom))
        .unwrap();
    let evaluator = Evaluator::new(vec![(Scope::Project, captured)]).unwrap();
    let decide = |source: &str, cwd: &str| {
        analyze(source, Dialect::Posix, cwd, project, &evaluator).unwrap()
    };
    for (source, cwd) in [
        ("npm start".to_owned(), project),
        (format!("npm --prefix {project} start"), "/"),
        (format!("npm --prefix={project} start"), "/"),
        ("npm --prefix . start".to_owned(), project),
        (format!("npm start --prefix {project}"), outside.as_str()),
    ] {
        assert_eq!(
            decide(&source, cwd)["decision"],
            "allow",
            "{source} from {cwd}"
        );
    }
    // Normalization never widens the folder a rule covers.
    let elsewhere = decide(&format!("npm --prefix {outside} start"), project);
    assert_eq!(elsewhere["decision"], "ask", "{elsewhere}");
    // After `cd` to an unknown folder, a relative value names nothing known,
    // and no request is normalized from an unknown cwd.
    for source in [
        "cd \"$ACME_DIR\"; npm --prefix . start".to_owned(),
        format!("cd \"$ACME_DIR\"; npm --prefix {project} start"),
    ] {
        let unknown = decide(&source, project);
        assert_ne!(unknown["decision"], "allow", "{source}: {unknown}");
        let operations = unknown["operations"].as_array().expect("operations");
        assert!(
            operations
                .iter()
                .all(|operation| operation["policy_request"]["directory_option"] != true),
            "{source}: {unknown}"
        );
    }
    // A command no rule covers is reported as the plain command in its folder,
    // which is what rule drafts are built from.
    let unmatched = decide(&format!("npm --prefix {project} run dev"), "/");
    assert_eq!(unmatched["decision"], "ask");
    let typed = &unmatched["operations"][0]["policy_request"];
    assert_eq!(typed["directory_option"], true, "{unmatched}");
    assert_eq!(typed["arguments"], json!(["run", "dev"]));
    assert_eq!(typed["cwd"], project);
}

#[cfg(unix)]
#[test]
fn a_directory_option_leaving_the_project_through_a_link_is_not_in_the_project() {
    use crate::{configuration::Candidate, policy::Scope};
    let root = assert_fs::TempDir::new().unwrap();
    let elsewhere = assert_fs::TempDir::new().unwrap();
    let project = root.path().canonicalize().unwrap();
    std::fs::create_dir(project.join("web")).unwrap();
    std::fs::create_dir(elsewhere.path().join("acme")).unwrap();
    std::os::unix::fs::symlink(elsewhere.path().join("acme"), project.join("link")).unwrap();
    let project = project.to_str().unwrap();
    let rule = json!({"id":"npm-start","decision":"allow","reason":"Acme start",
        "conditions":{"domain":"argv","executable":"npm","arguments":{"kind":"exact","values":["start"]},
            "cwd":{"within":{"root":"project"}}}});
    let declaration =
        json!({"executable":"npm","option":"--prefix","position":"anywhere","paths":"follow"});
    let custom = crate::policy::rules::document_with(&[rule], &[declaration])
        .unwrap()
        .to_string();
    let captured = Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.json\n")
        .unwrap()
        .capture(&[], Some(&custom))
        .unwrap();
    let evaluator = Evaluator::new(vec![(Scope::Project, captured)]).unwrap();
    let decide =
        |source: &str| analyze(source, Dialect::Posix, project, project, &evaluator).unwrap();
    // `..` after a real folder returns to the project.
    assert_eq!(decide("npm --prefix web/.. start")["decision"], "allow");
    // After a link it returns to the folder holding the link's target.
    let escaped = decide("npm --prefix link/.. start");
    assert_eq!(escaped["decision"], "ask", "{escaped}");
}

fn evaluator() -> Evaluator {
    use crate::{configuration::Candidate, policy::Scope};
    let custom = json!({"schema_version":1,"rules":[{"id":"read","decision":"allow","reason":"Acme read","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["read"]}}},{"id":"write","decision":"deny","reason":"Acme write","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["write"]}}}],"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]}).to_string();
    let candidate =
        Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.json\n").unwrap();
    Evaluator::new(vec![(
        Scope::Project,
        candidate.capture(&[], Some(&custom)).unwrap(),
    )])
    .unwrap()
}

fn preview(source: &str) -> Value {
    analyze(source, Dialect::Posix, "/acme", "/acme", &evaluator()).unwrap()
}

#[test]
fn quote_fragments_preserve_raw_spans_and_ansi_ascii_is_literal_data() {
    for source in ["acme $'write'", "acme wr$'\\x69'te", "acme $'\\167rite'"] {
        let result = analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap();
        assert_eq!(result["decision"], "deny", "{source}");
    }
    for source in [
        "acme $'write\\0ignored'",
        "acme $'wr\\x00ignored'ite",
        "acme $'wr\\000ignored\\\'more'ite",
        "acme $'wr\\x00\\xffignored'ite",
        "acme $'wr\\c@ignored'ite",
    ] {
        let result = analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap();
        assert_eq!(result["decision"], "deny", "{source}");
    }
    let source = "acme pre'quoted'\"double\"\\ space$(acme read)";
    let result = preview(source);
    let fragments = result["structure"]["words"][1]["fragments"]
        .as_array()
        .unwrap();
    assert_eq!(
        fragments
            .iter()
            .map(|fragment| fragment["raw"].as_str().unwrap())
            .collect::<String>(),
        &source[5..]
    );
    for fragment in fragments {
        let start = usize::try_from(fragment["span"]["start"].as_u64().unwrap()).unwrap();
        let end = usize::try_from(fragment["span"]["end"].as_u64().unwrap()).unwrap();
        assert_eq!(fragment["raw"], source[start..end]);
    }
    for source in [
        "acme $'\\\'$(acme write)'",
        "acme $'\\xff'",
        "acme $'\\0write'",
        "acme $'\\c1'",
        "acme $'\\c?'",
        "acme $'wr\\u0000ignored'ite",
        "acme $'\\u0061'",
        "acme $'a\\\nb'",
        "acme $\"write\"",
    ] {
        let result = analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap();
        assert_eq!(result["decision"], "ask", "{source}");
        assert_eq!(result["source_units"].as_array().unwrap().len(), 1);
    }
    assert_eq!(
        preview("acme read 0<&0-")["operations"][0]["descriptor_flow"]["stdin"]["kind"],
        "closed"
    );
}

fn restriction_evaluator(kind: &str, cwd: Option<&str>, tool: &str) -> Evaluator {
    use crate::{configuration::Candidate, policy::Scope};
    let arguments = if kind == "any" {
        json!({"kind":"any"})
    } else {
        json!({"kind":"prefix","values":["write"]})
    };
    let mut matcher = json!({"executable":tool,"arguments":arguments});
    if let Some(cwd) = cwd {
        matcher["cwd"] = json!({"exact":{"root":"absolute","path":cwd}});
    }
    matcher["domain"] = json!("argv");
    let document = crate::policy::rules::document(&[json!({"id":"cap","decision":"deny","reason":"Acme project restriction","conditions":matcher})]).unwrap();
    let candidate =
        Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.json\n").unwrap();
    Evaluator::new(vec![(
        Scope::Project,
        candidate.capture(&[], Some(&document.to_string())).unwrap(),
    )])
    .unwrap()
}

#[test]
fn unresolved_arguments_preserve_only_guaranteed_project_restrictions_and_provenance() {
    let make = restriction_evaluator;
    let root = assert_fs::TempDir::new().unwrap();
    let cwd = root.path().canonicalize().unwrap();
    let cwd = cwd.to_str().unwrap();
    for source in ["acme write \"$unknown\"", "cd /other; acme write $unknown"] {
        let report = analyze(
            source,
            Dialect::Bash,
            cwd,
            cwd,
            &make("prefix", None, "acme"),
        )
        .unwrap();
        assert_eq!(report["decision"], "deny");
        let bound = report["operations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|operation| operation["evaluation_model"] == "known-executable-restriction-bound")
            .unwrap();
        assert_eq!(bound["request"]["arguments"], json!(["write"]));
        assert_eq!(bound["context"]["arguments_known"], false);
    }
    for source in [
        "acme \"$unknown\"",
        "cd /other; acme \"$unknown\"",
        "acme $unknown",
        "acme $(opaque)",
    ] {
        let report = analyze(source, Dialect::Bash, cwd, cwd, &make("any", None, "acme")).unwrap();
        assert_eq!(report["decision"], "deny", "{source}");
        let operation = report["operations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|operation| operation["evaluation_model"] == "known-executable-restriction-bound")
            .unwrap();
        assert_eq!(operation["context"]["arguments_known"], false);
        assert_eq!(
            operation["evaluation"]["evaluation"]["matches"][0]["applied"],
            true
        );
    }
    let report = analyze(
        "acme \"$unknown\"",
        Dialect::Bash,
        cwd,
        cwd,
        &make("prefix", None, "acme"),
    )
    .unwrap();
    assert_eq!(report["decision"], "ask");
    assert_eq!(
        report["operations"][0]["evaluation"]["evaluation"]["matches"],
        json!([])
    );
    assert_eq!(
        analyze(
            "cd /other; acme \"$unknown\"",
            Dialect::Bash,
            cwd,
            cwd,
            &make("any", Some(cwd), "acme")
        )
        .unwrap()["decision"],
        "ask"
    );
    assert_eq!(
        analyze(
            "acme \"$unknown\"",
            Dialect::Bash,
            cwd,
            cwd,
            &make("any", Some(cwd), "acme")
        )
        .unwrap()["decision"],
        "deny"
    );
    assert_eq!(
        analyze(
            "cd \"$unknown\"",
            Dialect::Bash,
            cwd,
            cwd,
            &make("any", Some(cwd), "cd")
        )
        .unwrap()["decision"],
        "deny"
    );
}

#[test]
fn possible_functions_retain_external_policy_and_wrapper_denials() {
    for (source, expected) in [
        ("if acme read; then acme() { :; }; fi; acme write", "deny"),
        ("acme() { :; }; acme write", "ask"),
        (
            "if acme read; then nice() { :; }; fi; nice acme write",
            "deny",
        ),
    ] {
        assert_eq!(preview(source)["decision"], expected, "{source}");
    }
}

#[test]
fn literal_brace_options_are_scoped_and_child_shells_reset_option_state() {
    let uncertain = analyze(
        "acme re{a..a}d",
        Dialect::Bash,
        "/acme",
        "/acme",
        &evaluator(),
    )
    .unwrap();
    assert_eq!(uncertain["decision"], "ask");
    assert!(
        uncertain["operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|op| {
                op["coverage"] == "possible-brace-expansion"
                    && op["syntax"] == "braceexpand-unobserved"
            })
    );
    assert!(
        uncertain["operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|op| {
                op["request"]["arguments"] == json!(["read"])
                    && op["evaluation"]["decision"] == "allow"
            })
    );
    for source in [
        "set +B; for part in {read,write}; do acme \"$part\"; done",
        "set +o braceexpand; for part in {read,write}; do acme \"$part\"; done",
        "set +B; (set -B); for part in {read,write}; do acme \"$part\"; done",
    ] {
        assert_eq!(
            analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap()["decision"],
            "ask",
            "{source}"
        );
    }
    for source in [
        "set +B; set -B; for part in {read,write}; do acme \"$part\"; done",
        "set -o braceexpand; for part in {read,write}; do acme \"$part\"; done",
        "set +B; if acme read; then set -B; fi; for part in {read,write}; do acme \"$part\"; done",
        "set +B; bash -c 'for part in {read,write}; do acme \"$part\"; done'",
    ] {
        assert_eq!(
            analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap()["decision"],
            "deny",
            "{source}"
        );
    }
}

#[test]
fn cross_function_loop_transfer_does_not_claim_version_specific_successor_values() {
    let source =
        "part=read; f() { break; part=write; }; for value in once; do f; done; acme \"$part\"";
    let result = analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap();
    assert_eq!(result["decision"], "ask");
    assert!(
        result["operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|operation| {
                operation["coverage"] == "possible-expansion-mutation"
                    || operation["coverage"] == "opaque-arguments"
            })
    );
}

#[test]
fn valued_wrapper_options_preserve_the_child_boundary() {
    for source in [
        "command -p acme write",
        "exec -c -a acme-name acme write",
        "env -u ACME -- acme write",
        "env --unset=ACME acme write",
        "sudo -n -u acme-user -- acme write",
        "sudo -C 3 acme write",
        "exec -a -C acme write",
        "nice acme write",
        "nice -n 0 -- acme write",
        "nice --adjustment=-1 acme write",
        "/usr/bin/nice -n0 acme write",
        "nohup -- nice -n 0 acme write",
        "/usr/bin/nohup sh -c 'acme write'",
        "env -C /other acme write",
        "timeout 5 acme write",
        "timeout 2.5s acme write",
        "gtimeout -s KILL -k 1 1m acme write",
        "timeout --signal=TERM --kill-after=2 --preserve-status -v -- 10 acme write",
        "timeout -sKILL --foreground 1h acme write",
        "nohup timeout 5 nice acme write",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    for source in [
        "exec -a acme write",
        "sudo -u acme write",
        "env -u acme write",
        "command -v acme write",
        "nice -n acme write",
        "nice -n write acme read",
        "nice --unknown acme write",
        "nice --help acme write",
        "nohup --help acme write",
        "sudo --unknown acme write",
        "timeout acme write",
        "timeout -s 5 acme write",
        "timeout --unknown 5 acme write",
        "timeout 5x acme write",
        "timeout . acme write",
    ] {
        assert_eq!(preview(source)["decision"], "ask", "{source}");
    }
}

#[test]
fn heredoc_continuations_join_before_delimiter_and_map_back_to_raw_bytes() {
    for source in [
        "sh <<SCRIPT\nacme wr\\\nite\nSCRIPT\n",
        "acme read <<DATA\n$(acme wr\\\nite)\nDATA\n",
        "acme $(cat <<DATA\n)\\\n)\nDATA\nacme write\n)",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    let source = "acme read <<-DATA\n\tpre\\\n\t$(acme write)\n\tDA\\\nTA\n";
    let result = preview(source);
    assert_eq!(result["decision"], "deny");
    let parent = &result["source_units"][1]["parent"]["span"];
    let start = usize::try_from(parent["start"].as_u64().unwrap()).unwrap();
    let end = usize::try_from(parent["end"].as_u64().unwrap()).unwrap();
    assert_eq!(&source[start..end], "$(acme write)");
    let result = preview("acme read <<'DATA'\n$(acme wr\\\nite)\nDATA\n");
    assert_eq!(result["source_units"].as_array().unwrap().len(), 1);
}

#[test]
fn static_quoted_variables_track_assignments_and_forget_unknown_changes() {
    for source in [
        "operation=write; acme \"$operation\"",
        "operation='write'; acme \"${operation}\"",
        "operation=write; f() { acme \"$operation\"; }; f",
        "operation=read; (operation=write; acme \"$operation\")",
        "operation=write; operation=read acme \"$operation\"",
        "f() { acme \"$operation\"; }; operation=write f",
        "for operation in read write; do acme \"$operation\"; done",
        "for operation in read; do operation=write; acme \"$operation\"; done",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    for source in [
        "operation=write; read operation; acme \"$operation\"",
        "operation=write; operation=$unknown; acme \"$operation\"",
        "operation=write; if acme read; then operation=read; fi; acme \"$operation\"",
        "operation=read; operation=write acme \"$operation\"",
        "f() { acme \"$1\"; }; operation=write f \"$operation\"",
        "operation=read; (operation=write); acme \"$operation\"",
        "f() { acme \"$10\"; }; f write",
        "operation=write; sh -c 'acme \"$operation\"'",
    ] {
        assert_eq!(preview(source)["decision"], "ask", "{source}");
    }
}

#[test]
fn scanning_interrupts_long_words_and_bounds_cross_expansion_recursion() {
    use std::cell::Cell;
    let source = format!("acme '{}'; acme write", "x".repeat(60_000));
    let calls = Cell::new(0);
    let result = analyze_controlled(
        &source,
        Dialect::Posix,
        "/acme",
        None,
        "/acme",
        &evaluator(),
        &|| {
            calls.set(calls.get() + 1);
            if calls.get() == 4 {
                Err(Error::Interrupted)
            } else {
                Ok(())
            }
        },
    );
    assert!(matches!(result, Err(Error::Interrupted)));
    let source = format!(
        "acme {}acme read{}",
        "$(acme \"".repeat(40),
        "\")".repeat(40)
    );
    assert!(matches!(
        analyze(&source, Dialect::Posix, "/acme", "/acme", &evaluator()),
        Err(Error::Limit)
    ));
}

#[test]
fn ordered_descriptor_copies_feed_interpreters_and_keep_all_file_opens() {
    for source in [
        "sh 3<<'SCRIPT' 0<&3\nacme write\nSCRIPT\n",
        "sh 3<<'SCRIPT' 0<&3-\nacme write\nSCRIPT\n",
        "bash <<< 'acme write'",
        "sh 3<<< 'acme write' 0<&3",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    let result = preview("acme read 2>&1 >first >second");
    let flow = &result["operations"][0]["descriptor_flow"];
    assert_eq!(
        flow["bindings"]["2"],
        json!({"kind":"inherited","descriptor":1})
    );
    assert_eq!(flow["bindings"]["1"]["path"], "second");
    assert_eq!(flow["opened"].as_array().unwrap().len(), 2);
    assert_eq!(preview("sh 0<&3 3<<<'acme write'")["decision"], "ask");
    assert_eq!(
        preview("sh <<'SCRIPT' </dev/null\nacme write\nSCRIPT\n")["decision"],
        "ask"
    );
    let result = preview("acme read 0<&$unknown");
    assert_eq!(
        result["operations"][0]["descriptor_flow"]["syntax_complete"],
        false
    );
    assert_eq!(
        result["operations"][0]["descriptor_flow"]["stdin"]["kind"],
        "unknown"
    );
}

#[test]
fn substitution_heredocs_keep_data_delimiters_separate_from_shell_code() {
    for source in [
        "acme $(cat <<'DATA'\n) \" $(acme read)\nDATA\nacme write\n)",
        "acme $(cat <<-DATA\n\t)\n\tDATA\nacme write\n)",
        "acme $(cat <<ONE <<'TWO'\n)\nONE\n$(acme read) )\nTWO\nacme write\n)",
        "acme $(cat <<DATA\n$(acme write) )\nDATA\n)",
        "acme $(acme \"$(acme write)\")",
        "acme ${value:-$(cat <<'DATA'\n}\nDATA\nacme write\n)}",
    ] {
        let result = preview(source);
        assert_eq!(result["decision"], "deny", "{source}");
    }
    let result = preview("acme $(cat <<'DATA'\n$(acme write) )\nDATA\nacme read\n)");
    assert_eq!(result["decision"], "ask");
    assert!(
        !result["operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|operation| { operation["request"]["arguments"] == json!(["write"]) })
    );
}

#[test]
fn quoted_function_and_shell_positionals_retain_denials_and_argument_boundaries() {
    for source in [
        "f() { acme \"$1\"; }; f write",
        "f() { acme \"${1}\"; }; f write",
        "f() { acme \"$@\"; }; f write",
        "f() { g \"$@\"; }; g() { acme \"$1\"; }; f write",
        "sh -c 'acme \"$1\"' acme-shell write",
        "bash -c 'acme \"$@\"' acme-shell write",
        "f() { sh -c 'acme \"$1\"' child \"$1\"; }; f write",
        "f() { shift; acme \"$1\"; }; f read write",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    for source in [
        "f() { acme \"$1\"; }; f 'write extra'",
        "f() { acme \"$1\"; }; f '$unknown'",
        "f() { set -- read; acme \"$1\"; }; f write",
    ] {
        assert_eq!(preview(source)["decision"], "ask", "{source}");
    }
}

#[test]
fn quoted_positional_vectors_join_edges_without_splitting_or_assuming_ifs() {
    for (source, expected) in [
        ("set -- a b; acme \"pre$@post\"", json!(["prea", "bpost"])),
        ("set -- a b; acme \"$@$@\"", json!(["a", "ba", "b"])),
        (
            "set -- a b; acme 'pre'\"${@}\"'post'",
            json!(["prea", "bpost"]),
        ),
        ("set -- ''; acme \"pre$@post\"", json!(["prepost"])),
        ("set --; acme \"pre$@post\"", json!(["prepost"])),
        ("set --; acme \"$@\"", json!([])),
        ("set --; acme \"$*\"", json!([""])),
        ("set -- 'a b' ''; acme \"$@\"", json!(["a b", ""])),
        ("IFS=:; set -- a b; acme \"$*\"", json!(["a:b"])),
        ("IFS=''; set -- wr ite; acme \"${*}\"", json!(["write"])),
        ("set -- write; acme \"$*\"", json!(["write"])),
        ("set -- wr ite; acme \"$*\"", json!(["wr ite"])),
        ("set -- wr ite; acme $@", json!(["wr", "ite"])),
        ("set -- write; acme \"\\$@\"", json!(["$@"])),
    ] {
        let result = preview(source);
        assert!(
            result["operations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|operation| {
                    operation["request"]["executable"] == "acme"
                        && operation["request"]["arguments"] == expected
                        && operation["context"]["arguments_known"] != false
                }),
            "{source}: {result}"
        );
    }
    for source in [
        "set -- ite; acme \"wr$@\"",
        "set -- wr; acme \"${@}ite\"",
        "IFS=''; set -- wr ite; acme \"$*\"",
        "f() { acme \"wr$@\"; }; f ite",
        "set -- read write; for part in \"$@\"; do acme \"$part\"; done",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    for source in [
        "IFS=''; set -- wr ite; read IFS; acme \"$*\"",
        "set --; acme \"\"\"$@\"",
        "acme \"pre$@post\"",
    ] {
        let result = preview(source);
        assert_eq!(result["decision"], "ask", "{source}");
        assert!(
            result["operations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|operation| {
                    operation["request"]["executable"] == "acme"
                        && operation["context"]["arguments_known"] == false
                }),
            "{source}"
        );
    }
}

#[cfg(unix)]
#[test]
fn fixed_native_quoted_positional_vectors_confirm_edge_and_empty_boundaries() {
    let output = std::process::Command::new("/bin/sh")
        .env_clear()
        .args(["-c", "p() { printf '%s:' \"$#\"; for x in \"$@\"; do printf '<%s>' \"$x\"; done; printf '\\n'; }; set -- a b; p \"pre$@post\"; p \"$@$@\"; set --; p \"$@\"; p \"pre$@post\"; p \"$*\"; IFS=''; set -- wr ite; p \"$*\"; set -- 'a b' ''; p \"$@\""])
        .output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        b"2:<prea><bpost>\n3:<a><ba><b>\n0:\n1:<prepost>\n1:<>\n1:<write>\n2:<a b><>\n"
    );
}

#[test]
fn explicit_ifs_splitting_and_home_tilde_values_preserve_exact_fields() {
    for (source, expected) in [
        ("IFS=''; part=write; acme $part", json!(["write"])),
        (
            "IFS=' '; part=' read  write '; acme ${part}",
            json!(["read", "write"]),
        ),
        (
            "IFS=' :'; part=' :a:: b: '; acme $part",
            json!(["", "a", "", "b"]),
        ),
        ("IFS=''; part=''; acme $part", json!([])),
        ("IFS=''; set -- wr ite ''; acme $@", json!(["wr", "ite"])),
        ("IFS=''; set -- wr ite ''; acme $*", json!(["wr", "ite"])),
        ("HOME='a b*'; acme ~/file", json!(["a b*/file"])),
        ("HOME=write; acme ~", json!(["write"])),
    ] {
        let result = preview(source);
        assert!(
            result["operations"].as_array().unwrap().iter().any(|op| {
                op["request"]["executable"] == "acme"
                    && op["request"]["arguments"] == expected
                    && op["context"]["arguments_known"] != false
            }),
            "{source}: {result}"
        );
    }
    for source in [
        "IFS=''; part=write; acme $part",
        "part=write; acme $part",
        "IFS=''; set -- read write; for value in $*; do acme \"$value\"; done",
        "HOME=write; acme ~",
        "IFS=' '; part='read write'; for value in $part; do acme \"$value\"; done",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    for source in [
        // Bash splits this on the starting IFS and zsh does not.
        "set -- 'read write'; acme $1",
        "IFS=''; part='*'; acme $part",
        "IFS=''; part='w[ri]te'; acme $part",
        "IFS=''; part=wr; acme ${part}ite",
        "IFS=0; set -- wr; acme $10",
        "IFS=''; part=write; read IFS; acme $part",
        "HOME=write; acme ~/f*",
        "HOME=write; acme ~other",
        "acme ~",
    ] {
        let result = preview(source);
        assert_eq!(result["decision"], "ask", "{source}");
        assert!(
            result["operations"].as_array().unwrap().iter().any(|op| {
                op["request"]["executable"] == "acme" && op["context"]["arguments_known"] == false
            }),
            "{source}"
        );
    }
    assert!(matches!(
        split_fields_test_interrupt(),
        Err(Error::Interrupted)
    ));
}

fn split_fields_test_interrupt() -> Result<(), Error> {
    let word = structure::parse("$part", Dialect::Posix, &|| Ok(()))?;
    let Node::Command { words, .. } = word else {
        panic!("command expected")
    };
    let mut state = State::default();
    state.variables.insert("IFS".into(), " ".into());
    state
        .variables
        .insert("part".into(), "a".repeat(256).into());
    let checks = std::cell::Cell::new(0);
    expansion::argument_words(&words[0], &state, Dialect::Posix, &|| {
        checks.set(checks.get() + 1);
        if checks.get() > 2 {
            Err(Error::Interrupted)
        } else {
            Ok(())
        }
    })?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn fixed_native_explicit_ifs_and_home_fixture_confirms_no_globbing_of_tilde_value() {
    let output = std::process::Command::new("/bin/sh").env_clear()
        .args(["-c", "p() { printf '%s:' \"$#\"; for x in \"$@\"; do printf '<%s>' \"$x\"; done; printf '\\n'; }; IFS=' :'; v=' :a:: b: '; p $v; IFS=''; set -- wr ite ''; p $@; p $*; HOME='a b*'; p ~/file; IFS=0; set -- wr; p $10"])
        .output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        b"4:<><a><><b>\n2:<wr><ite>\n2:<wr><ite>\n1:<a b*/file>\n1:<wr0>\n"
    );
}

#[test]
fn child_shell_option_arity_groups_and_dialect_preserve_source_boundaries() {
    for source in [
        "bash -euxc 'acme write'",
        "bash -c -e 'acme write'",
        "bash --noprofile --rcfile 'acme read' -o pipefail -c 'acme write'",
        "bash --init-file 'acme write' -c 'acme read; acme write'",
        "bash -c -- 'acme \"$1\"' child write",
        "sh -euxc 'acme write'",
        "sh -o errexit -c 'acme write'",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    for source in [
        "bash --rcfile 'acme write' -c 'acme read'",
        "bash -O 'acme write' -c 'acme read'",
        "bash --unknown -c 'acme write'",
        "bash -nc 'acme write'",
        "bash -c",
    ] {
        assert_eq!(preview(source)["decision"], "ask", "{source}");
    }
    for (source, dialect) in [
        ("bash --posix -c 'acme read'", "posix"),
        ("bash -o posix -c 'acme read'", "posix"),
        ("bash --posix +o posix -c 'acme read'", "bash"),
    ] {
        assert_eq!(preview(source)["source_units"][1]["dialect"], dialect);
    }
}

#[test]
fn zsh_and_dash_child_scripts_are_read_only_where_posix_parsing_matches() {
    for source in [
        "zsh -c 'acme write'",
        "/bin/zsh -lc 'acme read && acme write'",
        "zsh -l -c 'acme read; acme write'",
        "zsh -fexc 'acme write 2>&1'",
        "zsh -c -- 'acme \"write\"'",
        "env zsh -c 'acme write'",
        "nohup zsh -lc 'acme read; acme write'",
        "dash -c 'acme write'",
        "/bin/dash -ec 'acme read | acme write'",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    // Readable scripts still ask: the child shell changes how the command runs.
    for source in ["zsh -c 'acme read'", "dash -c 'acme read'"] {
        assert_eq!(preview(source)["decision"], "ask", "{source}");
    }
    // Syntax whose zsh meaning differs, and options that change parsing,
    // leave the script unread.
    for source in [
        "zsh -c 'acme write $part'",
        "zsh -c 'acme wr*'",
        "zsh -c 'acme {write,read}'",
        "zsh -c 'acme (write|read)'",
        "zsh -c 'acme ~/write'",
        "zsh -c '=acme write'",
        "zsh -c 'acme write &> /dev/null'",
        "zsh -c 'acme read |& acme write'",
        "zsh -c 'acme write <<< data'",
        "zsh -c 'acme write >& log'",
        "zsh -o shwordsplit -c 'acme write'",
        "zsh -i -c 'acme write'",
        "zsh --no-rcs -c 'acme write'",
        "zsh -c",
        "/usr/local/bin/zsh -c 'acme write'",
    ] {
        assert_eq!(preview(source)["decision"], "ask", "{source}");
    }
    // zsh-only precommand modifiers and reserved words, and loops binding a
    // name zsh ties to a search path, leave the script unread.
    for source in [
        "zsh -c 'repeat 2 acme write'",
        "zsh -c 'acme read; nocorrect acme write'",
        "zsh -c 'noglob acme write'",
        "zsh -c '- acme write'",
        "zsh -c 'acme read && coproc acme write'",
        "zsh -c 'for path in /acme; do acme write; done'",
        "zsh -c 'for cdpath in /acme; do acme write; done'",
    ] {
        let result = preview(source);
        assert_eq!(result["decision"], "ask", "{source}");
        assert_eq!(
            result["source_units"].as_array().map(Vec::len),
            Some(1),
            "{source}"
        );
    }
    // The same words as arguments keep the script readable.
    assert_eq!(
        preview("zsh -c 'acme repeat noglob; acme write'")["decision"],
        "deny"
    );
}

#[test]
fn missing_sed_inputs_are_absent_only_when_nothing_else_in_the_request_can_create_them() {
    use crate::{configuration::Candidate, policy::Scope};
    let root = assert_fs::TempDir::new().unwrap();
    let cwd = root.path().to_str().unwrap();
    std::fs::write(root.path().join("notes.txt"), "a\n").unwrap();
    let within = |action: &str| json!({"action":action,"all":{"within":{"root":"project"}}});
    let rules = json!({"schema_version":1,"kind":"custom",
    "capabilities":["argument-rules-v1","location-v1","requirements-v1","structured-rules-v1"],
    "rules":[
        {"id":"sed","decision":"allow","conditions":{"domain":"argv","executable":"sed","arguments":{"kind":"any"}},
            "requirements":{"targets":[within("read"), within("write")]},"on_unmet":{"decision":"ask"}},
        {"id":"ln","decision":"allow","conditions":{"domain":"argv","executable":"ln","arguments":{"kind":"any"}}}
    ]});
    let candidate =
        Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.json\n").unwrap();
    let engine = Evaluator::new(vec![(
        Scope::Project,
        candidate.capture(&[], Some(&rules.to_string())).unwrap(),
    )])
    .unwrap();
    let scope = crate::location::WorkingFiles::default();
    let decide = |source: &str| {
        analyze_with_working_files(
            source,
            Dialect::Bash,
            cwd,
            None,
            cwd,
            &engine,
            &|| Ok(()),
            Some(&scope),
        )
        .unwrap()["decision"]
            .clone()
    };
    assert_eq!(decide("sed -i '' 's/a/b/' notes.txt"), "allow");
    // A link created first would let sed read another file into the project.
    for source in [
        "ln -s /acme/secret k; sed -i.bak 's/x/y/' k",
        "ln -s /acme/secret p; sed -i '' p notes.txt",
        "ln -s /acme/secret k & sed -i '' 's/a/b/' k",
        "sed -i '' 's/a/b/' k 2>/dev/null",
        // A lone sed whose `w` file or backup can create a later input.
        "sed -i 'w k' k",
        "sed -i.bak 's/a/b/' notes.txt notes.txt.bak",
    ] {
        assert_eq!(decide(source), "ask", "{source}");
    }
}

#[test]
fn coprocess_commands_are_evaluated_and_ask() {
    for dialect in [Dialect::Posix, Dialect::Bash] {
        for (source, expected) in [
            ("coproc acme write", "deny"),
            ("coproc { acme read; acme write; }", "deny"),
            ("coproc ACME { acme write; }", "deny"),
            ("coproc ACME (acme write)", "deny"),
            ("acme read; coproc acme write > /dev/null", "deny"),
            ("coproc acme read", "ask"),
            ("coproc ACME { acme read; }", "ask"),
        ] {
            let result = analyze(source, dialect, "/acme", "/acme", &evaluator()).unwrap();
            assert_eq!(result["decision"], expected, "{dialect:?}: {source}");
        }
    }
}

#[test]
fn known_assignment_expansions_are_sequential_without_field_splitting() {
    for source in [
        "part=write; next=\"$part\"; acme \"$next\"",
        "part=wr; next=\"${part}ite\"; acme \"$next\"",
        "part=write next=$part; acme \"$next\"",
        "part=write next=${part}; acme \"$next\"",
        "part=write; next=$part sh -c 'acme \"$next\"'",
        "part=write; next=\"$part\" sh <<'EOF'\nacme \"$next\"\nEOF\n",
        "part=write; next=$part bash <<<'acme \"$next\"'",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    for source in [
        "part='write extra'; next=$part; acme \"$next\"",
        "next=$unknown; acme \"$next\"",
        "part=read; part=write acme \"$part\"",
        "part=write; next=$part sh -c 'acme read'; acme \"$next\"",
    ] {
        assert_eq!(preview(source)["decision"], "ask", "{source}");
    }
    let result = preview("number=$((1+1)); acme \"$number\"");
    assert!(
        result["operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|operation| { operation["request"]["arguments"] == json!(["2"]) })
    );
}

#[test]
fn abrupt_exits_preserve_reachable_state_and_inspect_isolated_successors() {
    for source in [
        "part=read; f() { part=write; return; part=read; }; f; acme \"$part\"",
        "part=read; f() { part=write; command return; part=read; }; f; acme \"$part\"",
        "part=read; f() { part=write; builtin return; part=read; }; f; acme \"$part\"",
        "part=read; f() { (return); part=write; }; f; acme \"$part\"",
        "part=read; f() { sh -c 'return'; part=write; }; f; acme \"$part\"",
        "part=read; f() { printf '%s' \"$(return)\"; part=write; }; f; acme \"$part\"",
        "f() { return; acme write; }; f",
    ] {
        let result = analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap();
        assert_eq!(result["decision"], "deny", "{source}");
    }
    for source in [
        "part=read; f() { if acme read; then local part=write; return; fi; part=write; }; f; acme \"$part\"",
        "part=read; f() { if acme read; then local part=write; else part=write; fi; }; f; acme \"$part\"",
    ] {
        assert_eq!(
            analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap()["decision"],
            "ask",
            "{source}"
        );
    }
    for source in [
        "part=read; f() { return; part=write; }; f; acme \"$part\"",
        "part=read; f() { if acme read; then part=write; return; fi; part=read; }; f; acme \"$part\"",
        "part=read; for value in one; do break; part=write; done; acme \"$part\"",
        "part=read; for value in one; do continue; part=write; done; acme \"$part\"",
        "part=read; f() { return \"$unknown\"; part=write; }; f; acme \"$part\"",
    ] {
        let result = preview(source);
        assert_eq!(result["decision"], "ask", "{source}");
        assert!(
            !result["operations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|operation| {
                    operation["request"]["executable"] == "acme"
                        && operation["request"]["arguments"] == json!(["write"])
                }),
            "{source}"
        );
    }
}

#[test]
fn opaque_arithmetic_and_parameter_mutations_do_not_leave_stale_bindings() {
    for source in [
        "part=read; printf '%s' \"$((part=1))\"; acme \"$part\"",
        "part=''; printf '%s' \"${part:=write}\"; acme \"$part\"",
        "part=read; acme \"$((part=1))\" \"$part\"",
    ] {
        let result = preview(source);
        assert_eq!(result["decision"], "ask");
        assert!(
            result["operations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|operation| { operation["coverage"] == "possible-expansion-mutation" })
        );
        assert!(
            !result["operations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|operation| {
                    operation["request"]["executable"] == "acme"
                        && operation["request"]["arguments"] == json!(["read"])
                }),
            "{source}"
        );
    }
}

#[test]
fn literal_braces_and_scalar_trimming_retain_denials_and_expansion_bounds() {
    for source in [
        "for part in {read,write}; do acme \"$part\"; done",
        "for part in {read,{other,write}}; do acme \"$part\"; done",
        "for part in {1..3}; do acme write; done",
        "part=write-extra; acme \"${part%-extra}\"",
        "part=pre-write; acme \"${part#pre-}\"",
        "part=write-extra; acme \"${part%%-extra}\"",
        "part=pre-write; acme \"${part##pre-}\"",
    ] {
        assert_eq!(
            analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap()["decision"],
            "deny",
            "{source}"
        );
    }
    let report = analyze(
        "acme{a,b}{1,2}",
        Dialect::Bash,
        "/acme",
        "/acme",
        &evaluator(),
    )
    .unwrap();
    let derived = report["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|operation| operation["request"]["executable"] == "acmea1")
        .unwrap();
    assert_eq!(report["decision"], "ask");
    assert_eq!(
        derived["request"]["arguments"],
        json!(["acmea2", "acmeb1", "acmeb2"])
    );
    let length = preview("part=write; acme \"${#part}\"");
    assert!(
        length["operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|operation| { operation["request"]["arguments"] == json!(["5"]) })
    );
    assert!(matches!(
        analyze(
            "acme {1..10000}",
            Dialect::Bash,
            "/acme",
            "/acme",
            &evaluator()
        ),
        Err(Error::Limit)
    ));
    assert!(matches!(
        brace::expand("{a,b}", &|| Err(Error::Interrupted)),
        Err(Error::Interrupted)
    ));
    // Bash leaves an empty pair literal, including before a later expansion.
    // Expansions remain shell source, so the pair is quoted.
    for (word, expected) in [
        ("{}", vec!["'{}'"]),
        ("a{}b", vec!["a'{}'b"]),
        ("{}{x,y}", vec!["'{}'x", "'{}'y"]),
    ] {
        assert_eq!(
            brace::expand(word, &|| Ok(())).unwrap(),
            Some(expected.into_iter().map(str::to_owned).collect())
        );
    }
    for source in [
        "acme {001..003}",
        "acme {1..3..2}",
        "acme {,write}",
        "part=write; acme \"${part%*}\"",
    ] {
        assert_eq!(
            analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap()["decision"],
            "ask",
            "{source}"
        );
    }
}

#[test]
fn known_finite_loops_keep_sequential_values_and_break_continue_endpoints() {
    for source in [
        "for part in read write; do :; done; acme \"$part\"",
        "part=read; for value in one; do part=write; done; acme \"$part\"",
        "for part in read write; do continue; part=read; done; acme \"$part\"",
        "part=read; for value in one two; do part=write; break; part=read; done; acme \"$part\"",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    for source in [
        "for RANDOM in write; do acme \"$RANDOM\"; done",
        "select RANDOM in write; do acme \"$RANDOM\"; done",
        "for EUID in write; do acme \"$EUID\"; done",
        // A dynamic name without an uppercase letter keeps no loop value.
        "for _ in write; do acme \"$_\"; done",
        "part=read; for value in one two; do if acme read; then break; fi; part=write; done; acme \"$part\"",
    ] {
        assert_eq!(preview(source)["decision"], "ask", "{source}");
    }
    assert_eq!(
        preview("for part in read write; do break; done; acme \"$part\"")["decision"],
        "allow"
    );
}

#[test]
fn finite_loops_aggregate_command_policy_without_a_structural_ask() {
    for dialect in [Dialect::Posix, Dialect::Bash] {
        for (source, expected) in [
            ("for value in one two; do acme read; done", "allow"),
            ("for value in one two; do acme read && break; done", "allow"),
            (
                "for value in one two; do continue; acme read; done",
                "allow",
            ),
            ("for value in read write; do acme \"$value\"; done", "deny"),
            ("for value in read absent; do acme \"$value\"; done", "ask"),
            ("for value in one two; do acme absent && break; done", "ask"),
            ("for value in one two; do break; acme write; done", "deny"),
            ("for value in one; do (break); acme read; done", "ask"),
            ("for value in one; do break 0; done", "ask"),
            ("for PATH in /acme/bin; do acme read; done", "ask"),
            // A possibly exported name reaches the launched commands.
            ("for HOME in /acme/home; do acme read; done", "ask"),
            ("for HOME in /acme/home; do :; done; acme read", "ask"),
            (
                "for https_proxy in http://proxy.example.com; do acme read; done",
                "ask",
            ),
            ("for path in /acme/bin; do acme read; done", "ask"),
            ("for value in one; do break \"$UNKNOWN\"; done", "ask"),
            ("for value in \"$UNKNOWN\"; do acme read; done", "ask"),
        ] {
            let result = analyze(source, dialect, "/acme", "/acme", &evaluator()).unwrap();
            assert_eq!(result["decision"], expected, "{dialect:?}: {source}");
        }
    }
    // A lowercase name the host reports as environment is inherited too.
    let source = "for acme_home in /acme/home; do acme read; done";
    let reported = std::collections::BTreeMap::from([("acme_home".to_owned(), "/acme".to_owned())]);
    for (environment, expected) in [(None, "allow"), (Some(&reported), "ask")] {
        let result = super::analyze_controlled(
            source,
            Dialect::Bash,
            "/acme",
            environment,
            "/acme",
            &evaluator(),
            &|| Ok(()),
        )
        .unwrap();
        assert_eq!(result["decision"], expected, "{environment:?}");
    }
}

#[test]
fn resolved_loop_transfers_preserve_explicit_policy_restrictions() {
    use crate::{configuration::Candidate, policy::Scope};
    for decision in ["ask", "deny"] {
        let custom = json!({"schema_version":1,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"],"rules":[{"id":"transfer","decision":decision,"conditions":{"domain":"argv","executable":{"one_of":["break","continue"]},"arguments":{"kind":"any"}}}]}).to_string();
        let candidate =
            Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.json\n").unwrap();
        let evaluator = Evaluator::new(vec![(
            Scope::Project,
            candidate.capture(&[], Some(&custom)).unwrap(),
        )])
        .unwrap();
        for source in [
            "for value in one; do break; done",
            "for value in one; do continue; done",
        ] {
            let result = analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator).unwrap();
            assert_eq!(result["decision"], decision, "{source}");
        }
    }
    let pack = json!({"schema_version":1,"kind":"pack","id":"acme/control","version":"1.0.0","description":"Acme loop restrictions","license":"MIT","maintainers":["Acme"],"engines":{"shield":env!("CARGO_PKG_VERSION")},"capabilities":["argument-rules-v1","structured-rules-v1","fallback-rules-v1"],"rules":[{"id":"transfer","decision":"ask","fallback":true,"conditions":{"domain":"argv","executable":{"one_of":["break","continue"]},"arguments":{"kind":"any"}}}]}).to_string();
    let candidate = Candidate::from_yaml("schema_version: 1\npacks:\n  - id: acme/control\n    version: 1.0.0\n    profile: recommended\n    path: control.json\n").unwrap();
    let evaluator = Evaluator::new(vec![(
        Scope::Project,
        candidate.capture(&[pack], None).unwrap(),
    )])
    .unwrap();
    for source in [
        "for value in one; do break; done",
        "for value in one; do continue; done",
    ] {
        assert_eq!(
            analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator).unwrap()["decision"],
            "ask"
        );
    }
}

#[test]
fn server_smoke_loop_checks_each_invocation_and_keeps_runtime_pid_uncertainty() {
    let source = "(PORT=3999 node src/server.js & pid=$!; for i in 1 2 3 4 5 6 7 8 9 10; do curl -s localhost:3999/health && break; sleep 0.3; done; echo; kill $pid)";
    let result = analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap();
    assert_eq!(result["decision"], "ask");
    let operations = result["operations"].as_array().unwrap();
    for executable in ["node", "curl", "sleep", "echo", "kill"] {
        assert!(
            operations.iter().any(|operation| {
                operation["request"]["executable"] == executable
                    && operation["evaluation"].is_object()
            }),
            "missing policy lookup: {executable}"
        );
    }
    for operation in operations
        .iter()
        .filter(|operation| operation["request"]["executable"] == "break")
    {
        assert_eq!(operation["decision"], "allow");
    }
    assert!(operations.iter().any(|operation| {
        operation["request"]["executable"] == "kill"
            && operation["coverage"] == "background-job-kill"
            && operation["decision"] == "ask"
    }));
}

#[test]
fn background_pid_provenance_survives_whole_references_and_variable_copies() {
    for source in [
        "sleep 10 & kill $!",
        "sleep 10 & pid=$!; kill $pid",
        "sleep 10 & pid=\"$!\"; copy=${pid}; kill -TERM \"$copy\"",
        "sleep 10 & pid=$!; copy=$pid; pid=123; kill -- \"${copy}\"",
        "sleep 10 & pid=$!; if acme read; then copy=$pid; else copy=$pid; fi; kill $copy",
        "sleep 10 & pid=$!; for i in 1 2 3; do acme read; done; kill -s TERM \"$pid\"",
        "sleep 10 & pid=$!; IFS=1; kill \"$pid\"",
        "sleep 10 & pid=$!; pid=123 kill \"$pid\"",
        "sleep 10 & pid=$!; pid=123 command kill \"$pid\"",
        "sleep 10 & pid=$!; IFS=1 kill $pid",
        "sleep 10 & f() { kill \"$pid\"; }; pid=$! f",
    ] {
        let result = analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap();
        let kill = result["operations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|operation| operation["coverage"] == "background-job-kill")
            .unwrap_or_else(|| panic!("missing provenance: {source}: {result}"));
        assert_eq!(kill["decision"], "ask");
        assert_eq!(kill["runtime_ownership"], "unverified");
        assert_eq!(kill["context"]["arguments_known"], false);
        assert_eq!(kill["targets"].as_array().unwrap().len(), 1);
        let span = &kill["targets"][0]["span"];
        let start = usize::try_from(span["start"].as_u64().unwrap()).unwrap();
        let end = usize::try_from(span["end"].as_u64().unwrap()).unwrap();
        assert_eq!(&source[start..end], "sleep 10");
        assert!(!kill["request"]["arguments"].to_string().contains("123"));
    }
}

#[test]
fn background_pid_provenance_is_not_invented_from_unknown_or_modified_values() {
    for source in [
        "pid=$!; kill $pid",
        "sleep 10 & pid=$! kill $pid",
        "sleep 1 & pid=$(cat pidfile); pid=$! kill \"$pid\"",
        "sleep 1 & pid=$(cat pidfile); pid=$! command kill \"$pid\"",
        "sleep 10 & pid='$!'; kill $pid",
        "sleep 10 & pid=$!; pid=$(cat pidfile); kill $pid",
        "sleep 10 & pid=$!; unset pid; kill $pid",
        "sleep 10 & pid=$!; read pid; kill $pid",
        "sleep 10 & pid=$!; eval 'pid=123'; kill $pid",
        "sleep 10 & pid=$!; source settings.sh; kill $pid",
        "sleep 10 & pid=$!; if acme read; then pid=123; fi; kill $pid",
        "sleep 10 & pid=$!; if acme read; then sleep 20 & pid=$!; fi; kill $pid",
        "(sleep 10 & pid=$!); kill $pid",
        "sleep 10 & (kill $!)",
        "sleep 10 & pid=$!; IFS=1; kill $pid",
        "sleep 10 & pid=$!; IFS=1; IFS=' ' kill $pid",
        "sleep 10 & pid=$!; kill -$pid",
        "sleep 10 & pid=$!; kill 0 $pid",
        "sleep 10 & pid=$!; kill ${pid:-123}",
        "sleep 10 & pid=$!; kill $((pid + 1))",
        "sleep 10 & pid=$!; kill 1$pid",
        "sleep 10 & pid=$!; kill ${!pid}",
        "sleep 10 & pid=$!; kill -l $pid",
        "sleep 10 & { sleep 20; } & pid=$!; kill $pid",
    ] {
        let result = analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap();
        assert!(
            !result["operations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|operation| operation["coverage"] == "background-job-kill"),
            "{source}: {result}"
        );
    }
}

#[test]
fn background_pid_tracking_never_grants_kill_permission_or_hides_policy_denial() {
    use crate::{configuration::Candidate, policy::Scope};
    for decision in ["allow", "ask", "deny"] {
        let custom = json!({"schema_version":1,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"],
            "rules":[{"id":"kill","decision":decision,"conditions":{"domain":"argv","executable":"kill","arguments":{"kind":"any"}}}]}).to_string();
        let candidate =
            Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.json\n").unwrap();
        let evaluator = Evaluator::new(vec![(
            Scope::Project,
            candidate.capture(&[], Some(&custom)).unwrap(),
        )])
        .unwrap();
        let result = analyze(
            "sleep 10 & p=$!; kill \"$p\"",
            Dialect::Bash,
            "/acme",
            "/acme",
            &evaluator,
        )
        .unwrap();
        let kill = result["operations"]
            .as_array()
            .unwrap()
            .iter()
            .find(|op| op["coverage"] == "background-job-kill")
            .unwrap();
        assert_eq!(
            kill["decision"],
            if decision == "deny" { "deny" } else { "ask" }
        );
    }
}

#[test]
fn builtin_mutations_and_special_variable_values_cannot_be_literal_allow_shortcuts() {
    use crate::{configuration::Candidate, policy::Scope};
    for name in [
        "typeset",
        "let",
        "getopts",
        "mapfile",
        "readarray",
        "enable",
        "shopt",
        "hash",
        "bind",
        "ulimit",
        "umask",
        "dirs",
        "history",
        "fc",
        "compgen",
        "complete",
        "compopt",
        "fg",
        "bg",
        "disown",
        "suspend",
        "printf",
        "python2.7",
        "python3.11",
        "python3.13t",
        "pypy3",
        "lua5.4",
        "ruby3.4",
        "perl5.40",
        "tclsh8.6",
    ] {
        let rules = json!({"schema_version":1,"rules":[{"id":"literal","decision":"allow","reason":"Acme builtin preview","conditions":{"domain":"argv","executable":name,"arguments":{"kind":"any"}}}],"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]});
        let candidate =
            Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.json\n").unwrap();
        let engine = Evaluator::new(vec![(
            Scope::Project,
            candidate.capture(&[], Some(&rules.to_string())).unwrap(),
        )])
        .unwrap();
        let source = if name == "printf" {
            "printf -v part '%s' write".into()
        } else {
            format!("{name} acme")
        };
        assert_eq!(
            analyze(&source, Dialect::Bash, "/acme", "/acme", &engine).unwrap()["decision"],
            "ask",
            "{source}"
        );
    }
    for variable in ["RANDOM", "SECONDS", "LINENO", "SHLVL", "BASH_VERSION", "_"] {
        let source = format!("{variable}=write; acme \"${variable}\"");
        assert_eq!(preview(&source)["decision"], "ask", "{source}");
    }
    for source in [
        "part=read; printf '%5n' part; acme \"$part\"",
        "part=read; printf -vpart '%s' write; acme \"$part\"",
        "env SHLVL=write sh -c 'acme \"$SHLVL\"'",
    ] {
        assert_eq!(preview(source)["decision"], "ask", "{source}");
    }
    let source = "eval() { acme read; }; eval 'acme write'";
    assert_eq!(preview(source)["decision"], "deny");
    assert_eq!(
        analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap()["decision"],
        "ask"
    );
}

#[test]
fn interpreter_version_queries_follow_policy_and_other_forms_still_ask() {
    use crate::{configuration::Candidate, policy::Scope};
    let allow = ["python3", "python3.12", "node", "bash"].map(|name| {
        json!({"id":name,"decision":"allow","reason":"Acme interpreter",
            "conditions":{"domain":"argv","executable":name,"arguments":{"kind":"any"}}})
    });
    let rules = json!({"schema_version":1,"kind":"custom",
        "capabilities":["argument-rules-v1","structured-rules-v1"],"rules":allow});
    let candidate =
        Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.json\n").unwrap();
    let engine = Evaluator::new(vec![(
        Scope::Project,
        candidate.capture(&[], Some(&rules.to_string())).unwrap(),
    )])
    .unwrap();
    let decide = |source: &str| {
        analyze(source, Dialect::Bash, "/acme", "/acme", &engine).unwrap()["decision"].clone()
    };
    for source in [
        "python3 --version",
        "python3 -V",
        "python3.12 --version",
        "node -v",
        "bash --version",
    ] {
        assert_eq!(decide(source), "allow", "{source}");
    }
    for source in [
        "python3 -c 'print(1)'",
        "python3 --version script.py",
        "python3 -v",
        "bash -V",
    ] {
        assert_eq!(decide(source), "ask", "{source}");
    }
}

#[test]
fn builtin_queries_follow_policy_and_their_changing_forms_still_ask() {
    use crate::{configuration::Candidate, policy::Scope};
    let allow = ["command", "ulimit"].map(|name| {
        json!({"id":name,"decision":"allow","reason":"Acme builtin query",
            "conditions":{"domain":"argv","executable":name,"arguments":{"kind":"any"}}})
    });
    let rules = json!({"schema_version":1,"kind":"custom",
        "capabilities":["argument-rules-v1","structured-rules-v1"],"rules":allow});
    let candidate =
        Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.json\n").unwrap();
    let engine = Evaluator::new(vec![(
        Scope::Project,
        candidate.capture(&[], Some(&rules.to_string())).unwrap(),
    )])
    .unwrap();
    let decide = |source: &str| {
        analyze(source, Dialect::Bash, "/acme", "/acme", &engine).unwrap()["decision"].clone()
    };
    for source in [
        "command -v acme",
        "command -V acme",
        "command -pv acme",
        "command -p -v acme",
        "ulimit",
        "ulimit -a",
        "ulimit -n",
        "ulimit -Sn",
    ] {
        assert_eq!(decide(source), "allow", "{source}");
    }
    for source in [
        "command acme write",
        "command -p acme write",
        "ulimit -n 4096",
        "ulimit unlimited",
    ] {
        assert_eq!(decide(source), "ask", "{source}");
    }
}

#[test]
fn scalar_quote_concatenation_and_positional_updates_keep_exact_operands() {
    for source in [
        "part=wr; acme \"${part}ite\"",
        "part=ite; acme \"wr$part\"",
        "part=ite; acme wr\"$part\"",
        "part=ite; acme 'wr'\"$part\"",
        "part=wr; acme \"$part\"ite",
        "part=''; acme \"${part:-write}\"",
        "part=read; acme \"${part:+write}\"",
        "part=''; acme \"${part+write}\"",
        "part=write; acme \"${part:-read}\"",
        "f() { acme \"${2-write}\"; }; f read",
        "f() { acme \"wr${1}\"; }; f ite",
        "set -- read write; shift; acme \"$1\"",
        "set -- write; shift 2; acme \"$1\"",
        "set -- read read write; shift 2; acme \"$1\"",
        "set -- write; shift 0; acme \"$@\"",
        "part=write; acme $part",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    for source in [
        "part='write extra'; acme \"$part\"",
        "part=write; acme \"\\$part\"",
        "acme \"${unknown:-write}\"",
        "part=''; acme \"${part-write}\"",
        "part=''; acme \"${part:+write}\"",
        "set -- ''; acme \"${1-write}\"",
        "set -- write; set --; acme \"$@\"",
        "set -- write; shift; acme \"$1\"",
        "set -- write; shift \"$unknown\"; acme \"$1\"",
        "set -- write; acme \"$10\"",
    ] {
        assert_eq!(preview(source)["decision"], "ask", "{source}");
    }
    let value = preview("set -- 'write extra'; acme \"prefix ${1} suffix\"");
    assert!(
        value["operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|op| { op["request"]["arguments"] == json!(["prefix write extra suffix"]) })
    );
}

#[test]
fn explicit_export_and_wrapper_environments_reach_only_the_declared_child() {
    for source in [
        "export part=write; sh -c 'acme \"$part\"'",
        "part=write; export part; sh -c 'acme \"$part\"'",
        "part=read; export part; part=write; bash -c 'acme \"$part\"'",
        "part=write sh -c 'acme \"$part\"'",
        "env -i part=write sh -c 'acme \"$part\"'",
        "part=write sh <<'SCRIPT'\nacme \"$part\"\nSCRIPT\n",
        "export part=write; sh <<'SCRIPT'\nacme \"$part\"\nSCRIPT\n",
        "env -i part=write sh <<'SCRIPT'\nacme \"$part\"\nSCRIPT\n",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    for source in [
        "part=write; sh -c 'acme \"$part\"'",
        "export part=write; env -i sh -c 'acme \"$part\"'",
        "export part=write; /usr/bin/env --ignore-environment sh -c 'acme \"$part\"'",
        "export part=write; env -u part sh -c 'acme \"$part\"'",
        "export part=write; env --unset=part sh -c 'acme \"$part\"'",
        "export part=write; exec -c sh -c 'acme \"$part\"'",
        "export part=write; sudo -E sh -c 'acme \"$part\"'",
        "export part=write; unset part; part=read; sh -c 'acme \"$part\"'",
    ] {
        assert_eq!(preview(source)["decision"], "ask", "{source}");
    }
}

#[test]
fn literal_bash_locals_restore_callers_and_preserve_dynamic_scope() {
    for source in [
        "part=read; f() { local part=write; acme \"$part\"; }; f",
        "part=write; f() { local part=read; }; f; acme \"$part\"",
        "f() { local part=write; g; }; g() { acme \"$part\"; }; f",
        "part=write; f() { local part=read; g; }; g() { local part=other; }; f; acme \"$part\"",
        "part=write; f() { local part=read; local part=other; }; f; acme \"$part\"",
    ] {
        assert_eq!(
            analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap()["decision"],
            "deny",
            "{source}"
        );
    }
    for source in [
        "part=read; f() { local part=write; }; f; acme \"$part\"",
        "part=read; f() { local -n part=other; acme \"$part\"; }; f",
        "part=read; f() { local part; acme \"$part\"; }; f",
        "f() { local part=write; bash -c 'local part=read; acme \"$part\"'; }; f",
        "part=write; f() { if unknown; then local part=read; else part=other; fi; }; f; acme \"$part\"",
    ] {
        assert_eq!(
            analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap()["decision"],
            "ask",
            "{source}"
        );
    }
}

#[test]
fn implicit_positional_loops_and_empty_menus_have_distinct_execution_candidates() {
    for source in [
        "f() { for operation; do acme \"$operation\"; done; }; f write",
        "set -- read write; for operation; do acme \"$operation\"; done",
        "set -- read write; for operation in \"$@\"; do acme \"$operation\"; done",
        "select operation in read; do acme \"${operation:-write}\"; done",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    for (source, decision) in [
        ("for operation in; do acme write; done", "allow"),
        ("set --; for operation; do acme write; done", "ask"),
        (
            "set --; for operation in \"$@\"; do acme write; done",
            "ask",
        ),
        ("select operation in; do acme write; done", "ask"),
    ] {
        let result = preview(source);
        assert_eq!(result["decision"], decision, "{source}");
        assert!(
            !result["operations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|op| op["request"]["arguments"] == json!(["write"]))
        );
    }
}

#[test]
fn literal_eval_reparses_joined_arguments_and_parent_builtins_keep_state() {
    for source in [
        "eval 'acme write'",
        "eval acme write",
        "part=write; eval 'acme \"$part\"'",
        "eval 'part=write'; acme \"$part\"",
        "eval 'f() { acme write; }'; f",
        "f() { acme write; }; command eval f",
        "command set -- write; acme \"$1\"",
        "set -- read write; command shift; acme \"$1\"",
        "command export part=write; sh -c 'acme \"$part\"'",
        "command cd /other; acme write",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    for source in [
        "builtin eval 'acme write'",
        "builtin set -- write; acme \"$1\"",
        "eval -- 'acme write'",
    ] {
        assert_eq!(
            analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap()["decision"],
            "deny",
            "{source}"
        );
    }
    for source in [
        "eval 'printf acme write'",
        "eval 'acme \"write extra\"'",
        "eval \"$unknown\"",
        "f() { acme write; }; command f",
        "builtin acme write",
        "sh -c ''",
        "eval '# acme write'",
    ] {
        assert_eq!(preview(source)["decision"], "ask", "{source}");
    }
    assert_eq!(preview("# acme write")["decision"], "allow");
    let result = preview("eval 'acme' 'write'");
    assert_eq!(result["source_units"][1]["source"], "acme write");
    assert_eq!(result["source_units"][1]["parent"]["kind"], "eval-script");
}

#[test]
fn constant_arithmetic_preserves_precedence_and_asks_on_side_effects_or_width_uncertainty() {
    use crate::{configuration::Candidate, policy::Scope};
    let doc = json!({"schema_version":1,"rules":[{"id":"two","decision":"deny","reason":"Acme two","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["2"]}}},{"id":"three","decision":"allow","reason":"Acme three","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"exact","values":["3"]}}}],"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]});
    let candidate =
        Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.json\n").unwrap();
    let evaluator = Evaluator::new(vec![(
        Scope::Project,
        candidate.capture(&[], Some(&doc.to_string())).unwrap(),
    )])
    .unwrap();
    for source in [
        "IFS=' '; acme $((1+1))",
        "acme $((1+1))",
        "acme \"$((8 / 3))\"",
        "IFS=''; acme $((010-6))",
        "IFS=' '; acme $((0x10 / (4 * 2)))",
    ] {
        assert_eq!(
            analyze(source, Dialect::Posix, "/acme", "/acme", &evaluator).unwrap()["decision"],
            "deny",
            "{source}"
        );
    }
    assert_eq!(
        analyze(
            "acme \"$((1+4/2))\"",
            Dialect::Posix,
            "/acme",
            "/acme",
            &evaluator
        )
        .unwrap()["decision"],
        "allow"
    );
    for source in [
        "IFS=2; acme $((1+1))",
        "acme $((unknown+1))",
        "acme $((1++2))",
        "acme $((2147483647+1))",
        "acme $((1/0))",
        "acme $((x=2))",
        "acme $((08))",
        "acme $((2**1))",
    ] {
        assert_eq!(
            analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator).unwrap()["decision"],
            "ask",
            "{source}"
        );
    }
    for (source, result) in [
        ("-7/3", "-2"),
        ("-7%3", "-1"),
        ("2*3+4", "10"),
        ("2*(3+4)", "14"),
    ] {
        assert_eq!(arithmetic::constant(source).as_deref(), Some(result));
    }
    assert!(arithmetic::constant(&format!("{}1{}", "(".repeat(40), ")".repeat(40))).is_none());
}

#[test]
fn definite_function_effects_survive_return_without_leaking_bash_prefix_bindings() {
    for source in [
        "part=read; f() { part=write; }; f; acme \"$part\"",
        "f() { part=write; }; f; acme \"$part\"",
        "f() { g() { acme write; }; }; f; g",
        "f() { set -- read; }; set -- write; f; acme \"$1\"",
        "if unknown; then part=write; else part=write; fi; acme \"$part\"",
        "if unknown; then f() { part=write; }; else f() { part=write; }; fi; f; acme \"$part\"",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    for source in [
        "part=write; f() { :; }; part=read f; acme \"$part\"",
        "part=write; f() { part=other; }; part=read f; acme \"$part\"",
    ] {
        assert_eq!(
            analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap()["decision"],
            "deny",
            "{source}"
        );
    }
    assert_eq!(
        preview("part=write; f() { :; }; part=read f; acme \"$part\"")["decision"],
        "ask"
    );
    assert_eq!(
        preview("part=read; if unknown; then f() { part=write; }; fi; f; acme \"$part\"")["decision"],
        "ask"
    );
}

#[test]
fn dense_explanations_cannot_accumulate_past_the_shared_evidence_budget() {
    use crate::{configuration::Candidate, policy::Scope};
    let rules: Vec<_> = (0..200)
        .map(|id| {
            json!({"id":format!("acme-{id}"),"decision":"allow","reason":"x".repeat(4096),"conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"}}})
        })
        .collect();
    let custom = json!({"schema_version":1,"rules":rules,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]}).to_string();
    let candidate =
        Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.json\n").unwrap();
    let evaluator = Evaluator::new(vec![(
        Scope::Project,
        candidate.capture(&[], Some(&custom)).unwrap(),
    )])
    .unwrap();
    assert_eq!(
        analyze("acme read", Dialect::Posix, "/acme", "/acme", &evaluator).unwrap()["decision"],
        "allow"
    );
    assert!(matches!(
        analyze(
            &"acme read;".repeat(12),
            Dialect::Posix,
            "/acme",
            "/acme",
            &evaluator
        ),
        Err(Error::Limit)
    ));
}

#[test]
fn dense_background_kill_explanations_respect_the_shared_evidence_budget() {
    use crate::{configuration::Candidate, policy::Scope};
    let rules: Vec<_> = (0..200)
        .map(|id| {
            json!({"id":format!("kill-{id}"),"decision":"ask","reason":"x".repeat(4096),"conditions":{"domain":"argv","executable":"kill","arguments":{"kind":"any"}}})
        })
        .collect();
    let custom = json!({"schema_version":1,"rules":rules,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]}).to_string();
    let candidate =
        Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.json\n").unwrap();
    let evaluator = Evaluator::new(vec![(
        Scope::Project,
        candidate.capture(&[], Some(&custom)).unwrap(),
    )])
    .unwrap();
    let launch = "sleep 10 & pid=$!;";
    let kill = "kill \"$pid\";";
    // Source is analyzed, never executed. Unknown-argument evaluation retains
    // restrictions, so Ask rules supply dense evidence without granting Allow.
    let result = analyze(
        &format!("{launch}{kill}"),
        Dialect::Bash,
        "/acme",
        "/acme",
        &evaluator,
    )
    .unwrap();
    let operation = result["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|operation| operation["coverage"] == "background-job-kill")
        .unwrap();
    assert_eq!(operation["decision"], "ask");
    assert_eq!(operation["runtime_ownership"], "unverified");
    assert!(matches!(
        analyze(
            &format!("{launch}{}", kill.repeat(12)),
            Dialect::Bash,
            "/acme",
            "/acme",
            &evaluator,
        ),
        Err(Error::Limit)
    ));
}

#[test]
fn expressions_arithmetic_loops_and_compound_functions_inspect_possible_code() {
    for source in [
        "[[ $(acme write) = value ]]",
        "acme ${ acme write; }",
        "acme ${| acme write; }",
        "(( $(acme write) + 1 ))",
        "for ((i=0; i<$(acme write); i++)); do acme read; done",
        "for ((i=0; i<2; i++)); do acme write; done",
        "f() if acme read; then acme write; fi; f",
        "f() { acme write; } >target; f",
        "time -p acme write",
        "( (acme write) )",
    ] {
        let result = analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator())
            .unwrap_or_else(|error| panic!("{source}: {error}"));
        assert_eq!(result["decision"], "deny", "{source}");
    }
    assert_eq!(preview("( (acme write) )")["decision"], "deny");
    for source in ["[[ value = value ]]", "(( i++ ))"] {
        assert_eq!(
            analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap()["decision"],
            "ask"
        );
    }
}

#[test]
fn expansions_assignments_and_compound_redirects_keep_nested_denials() {
    for source in [
        "acme `acme write`",
        "acme \"`acme write`\"",
        "acme ${value:-$(acme write)}",
        "acme ${value:-$(acme '}' ; acme write)}",
        "acme $((1 + $(acme write)))",
        "ACME=value acme write",
        "ACME=$(acme write) acme read",
        "! acme write",
        "time acme write",
        "acme read |& acme write",
        "{ acme read; acme write; } >target",
        "(acme write)  >target",
        "{ acme write; } 2>target",
        "(acme write) 2>&1",
        "acme read <<<$(acme write)",
        "case x in a) acme read ;& b) acme write;; esac",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    for source in ["acme '${value:-$(acme write)}'", "acme '`acme write`'"] {
        assert_eq!(preview(source)["source_units"].as_array().unwrap().len(), 1);
    }
    let result = preview("ACME=value acme read; acme read");
    assert_eq!(
        result["operations"].as_array().unwrap().last().unwrap()["context"]["environment_known"],
        true
    );
    let current = analyze(
        "acme ${ cd /other; }; acme read",
        Dialect::Bash,
        "/acme",
        "/acme",
        &evaluator(),
    )
    .unwrap();
    assert_eq!(
        current["operations"].as_array().unwrap().last().unwrap()["context"]["cwd_known"],
        false
    );
    let isolated = preview("acme $(cd /other); acme read");
    assert_eq!(
        isolated["operations"].as_array().unwrap().last().unwrap()["context"]["cwd_known"],
        true
    );
    let result = preview("case x in a) cd /other ;& b) acme read;; esac");
    assert_eq!(
        result["operations"].as_array().unwrap().last().unwrap()["context"]["cwd_known"],
        false
    );
    assert_eq!(preview("case x in a) ;; esac")["decision"], "ask");
    for source in [
        "acme ${unfinished",
        "acme `unfinished",
        "!",
        "acme read >>>target",
    ] {
        assert!(
            analyze(source, Dialect::Posix, "/acme", "/acme", &evaluator()).is_err(),
            "{source}"
        );
    }
}

#[test]
fn functions_loops_cases_and_nested_substitutions_preserve_reachable_denials() {
    for source in [
        "f() { acme write; }; f",
        "f() { acme write; }; f $unknown",
        "command -- acme write",
        "env -i ACME=value acme write",
        "sudo -- acme write",
        "exec acme write",
        "function f { acme write; }; f",
        "for item in a b; do acme write; done",
        "for item; do acme write; done",
        "case value in a) acme read;; b|c) acme write;; esac",
        "acme $(acme write)",
        "acme \"$(acme read; acme write)\"",
        "acme $(acme $(acme write))",
        "acme $(case value in a) acme write;; b) acme read;; esac)",
        "acme $(acme read # ignored )\nacme write)",
        "acme $(acme case; acme write)",
        "acme <(acme write)",
        "acme >(acme write)",
        "for item in $(acme write); do acme read; done",
        "case $(acme write) in a) acme read;; esac",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    assert_eq!(preview("f() { acme write; }; acme read")["decision"], "ask");
    assert_eq!(
        preview("acme '$(acme write)'")["source_units"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let case = preview("case value in a|b) acme write;; esac");
    assert_eq!(case["structure"]["arms"][0]["patterns"][1]["raw"], "b");
    let nested = preview("acme \"$(acme write)\"");
    assert_eq!(
        nested["source_units"][1]["parent"]["span"],
        json!({"start":6,"end":19})
    );
    assert_eq!(nested["operations"][0]["source_unit"], 1);
    assert!(matches!(
        analyze(
            "f() { f; }; f",
            Dialect::Posix,
            "/acme",
            "/acme",
            &evaluator()
        ),
        Err(Error::Limit)
    ));
}

#[test]
fn function_binding_redefinitions_and_subshells_keep_owned_state() {
    assert_eq!(
        preview("f() { acme write; }; f() { acme read; }; f")["decision"],
        "ask"
    );
    assert_eq!(
        preview("f() { acme read; }; (f() { acme write; }); f")["decision"],
        "ask"
    );
    assert_eq!(
        preview("if acme read; then f() { acme write; }; else f() { acme read; }; fi; f")["decision"],
        "deny"
    );
    assert_eq!(
        preview("f() { cd /other; }; f; acme read")["operations"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["context"]["cwd_known"],
        false
    );
    for source in [
        "for 1 in a; do acme read; done",
        "case a in a acme write",
        "f() acme read",
    ] {
        assert!(
            analyze(source, Dialect::Posix, "/acme", "/acme", &evaluator()).is_err(),
            "{source}"
        );
    }
}

#[test]
fn null_device_redirections_need_no_location_policy() {
    for source in [
        "acme read 2>/dev/null",
        "acme read >/dev/null 2>&1",
        "acme read &>/dev/null",
        "acme read >>/dev/null",
        "acme read </dev/null",
        "acme read 2>'/dev/null'",
        "acme read 2>&1",
        "acme read >&2",
        "acme read 3>&1 1>&2 2>&3 3>&-",
        "acme read 0<&0-",
    ] {
        assert_eq!(preview(source)["decision"], "allow", "{source}");
    }
    assert!(
        preview("acme read 2>/dev/null")["operations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|operation| operation["coverage"] == "null-device-redirection")
    );
    assert_eq!(preview("acme write 2>/dev/null")["decision"], "deny");
    for source in [
        "acme read >&target",
        "acme read 2>&$fd",
        "acme read 2>/dev/null2",
        "acme read 2>dev/null",
        "acme read 2>/dev/../dev/null",
        "acme read 2>/dev/null/",
        "acme read 2>$NULL",
    ] {
        assert_eq!(preview(source)["decision"], "ask", "{source}");
    }
}

#[test]
fn inline_input_redirections_do_not_add_approval_to_permitted_commands() {
    for source in [
        "acme read <<'EOF'\n$(acme write)\nEOF\n",
        "acme read <<EOF\nliteral data\nEOF\n",
        "acme read <<-EOF\n\tdata\n\tEOF\n",
        "acme read <<A <<'B'\none\nA\ntwo\nB\n",
        "acme read <<< 'literal data'",
    ] {
        let result = analyze(source, Dialect::Bash, "/acme", "/acme", &evaluator()).unwrap();
        assert_eq!(result["decision"], "allow", "{source}: {result}");
    }
    for source in [
        "acme read <<EOF\n$(acme write)\nEOF\n",
        "acme read <<< \"$(acme write)\"",
        "acme write <<'EOF'\ndata\nEOF\n",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    assert_eq!(
        preview("acme read <<EOF\ndata\nEOF\nacme unknown")["decision"],
        "ask"
    );
}

#[test]
fn redirects_and_heredocs_preserve_code_data_and_descriptor_boundaries() {
    for source in [
        "acme write>target",
        "acme write 2>>target",
        "acme read >$(acme write)",
        "sh <<'EOF'\nacme write\nEOF\n",
        "env -i sh <<'EOF'\nacme write\nEOF\n",
        "command bash -s <<'EOF'\nacme write\nEOF\n",
        "ACME=value sh <<'EOF'\nacme write\nEOF\n",
        "cat <<EOF\n$(acme write)\nEOF\n",
        "cat <<EOF\n'$(acme write)'\nEOF\n",
        "cat <<EOF\ndata\nEOF\nacme write",
    ] {
        assert_eq!(preview(source)["decision"], "deny", "{source}");
    }
    for source in [
        "cat <<'EOF'\nacme write\nEOF\n",
        "cat <<'EOF'\n$(acme write)\nEOF\n",
        "cat <<EOF\nacme write\nEOF\n",
        "cat <<-EOF\n\tdata\n\tEOF\n",
        "cat <<A <<'B'\none\nA\ntwo\nB\n",
        ">target",
        "sh <<EOF\nEOF\n",
        "sh -c 'acme read' <<'EOF'\nacme write\nEOF\n",
        "cat <<\"E\\x\"\ndata\nE\\x\n",
    ] {
        assert_eq!(preview(source)["decision"], "ask", "{source}");
    }
    let redirected = preview("acme read 2>>target");
    assert_eq!(
        redirected["structure"]["redirections"][0]["descriptor"],
        "2"
    );
    let source = "cat <<-EOF\n\t\tfirst\n\t$(acme write)\n\tEOF\n";
    let stripped = preview(source);
    assert_eq!(stripped["decision"], "deny");
    let parent = &stripped["source_units"][1]["parent"]["span"];
    let start = usize::try_from(parent["start"].as_u64().unwrap()).unwrap();
    let end = usize::try_from(parent["end"].as_u64().unwrap()).unwrap();
    assert_eq!(&source[start..end], "$(acme write)");
    assert_eq!(
        redirected["structure"]["words"].as_array().unwrap().len(),
        2
    );
    assert_eq!(
        preview("acme read 2 >target")["structure"]["redirections"][0]["descriptor"],
        Value::Null
    );
    for source in ["acme read >", "cat <<EOF\nmissing", "cat <<", "cat <<EOF"] {
        assert!(
            analyze(source, Dialect::Posix, "/acme", "/acme", &evaluator()).is_err(),
            "{source}"
        );
    }
    let nested_wrappers = format!("{}acme read", "env ".repeat(34));
    assert!(matches!(
        analyze(
            &nested_wrappers,
            Dialect::Posix,
            "/acme",
            "/acme",
            &evaluator()
        ),
        Err(Error::Limit)
    ));
}

#[cfg(unix)]
#[test]
fn fixed_harmless_shell_heredocs_confirm_delimiter_quote_semantics() {
    // These are fixed test-owned probes, never submitted analyzer source.
    for (source, expected) in [
        (
            "cat <<'EOF'\n$(printf expanded)\nEOF\n",
            "$(printf expanded)\n",
        ),
        ("cat <<EOF\n'$(printf expanded)'\nEOF\n", "'expanded'\n"),
        ("cat <<-EOF\n\tdata\n\tEOF\n", "data\n"),
        ("cat <<EOF\npre\\\npost\nEO\\\nF\n", "prepost\n"),
        ("cat <<'EOF'\npre\\\npost\nEOF\n", "pre\\\npost\n"),
        ("f() { printf '<%s>' \"$@\"; }; f 'a b' ''", "<a b><>"),
        (
            "printf '<%s>' $((1+1)) $((-7/3)) $((-7%3)) $((010+2)) $((0x10))",
            "<2><-2><-1><10><16>",
        ),
        ("part=ite; printf '<%s>' \"wr${part}\"", "<write>"),
        (
            "part=pre-write-extra; part=\"${part#pre-}\"; printf '<%s>' \"${part%-extra}\" \"${#part}\"",
            "<write><11>",
        ),
        (
            "part=read; f() { part=write; return; part=read; }; f; printf '<%s>' \"$part\"",
            "<write>",
        ),
        (
            "part=read; for value in one; do break; part=write; done; printf '<%s>' \"$part\"",
            "<read>",
        ),
        (
            "part=read; for value in one; do continue; part=write; done; printf '<%s>' \"$part\"",
            "<read>",
        ),
        ("part=write next=$part; printf '<%s>' \"$next\"", "<write>"),
        (
            "part='write extra'; next=$part; printf '<%s>' \"$next\"",
            "<write extra>",
        ),
        (
            "part=write; next=\"$part\" /bin/sh -c 'printf \"<%s>\" \"$next\"'",
            "<write>",
        ),
        (
            "/bin/sh -euxc 'printf \"<%s>\" \"$1\"' child write",
            "<write>",
        ),
        (
            "/bin/sh -c -e 'printf \"<%s>\" \"$1\"' child write",
            "<write>",
        ),
        (
            "part=''; printf '<%s>' \"${part:-write}\" \"${part-write}\" \"${part+write}\"",
            "<write><><write>",
        ),
        ("set -- read write; shift; printf '<%s>' \"$1\"", "<write>"),
        (
            "command set -- read write; command shift; printf '<%s>' \"$1\"",
            "<write>",
        ),
        ("set -- write; eval 'printf \"<%s>\" \"$1\"'", "<write>"),
        (
            "part=read; f() { local part=write; printf '<%s>' \"$part\"; }; f; printf '<%s>' \"$part\"",
            "<write><read>",
        ),
        ("set -- write; shift 2; printf '<%s>' \"$1\"", "<write>"),
        (
            "export part=write; /bin/sh -c 'printf \"<%s>\" \"$part\"'",
            "<write>",
        ),
    ] {
        let output = std::process::Command::new("/bin/sh")
            .args(["-c", source])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    }
}

#[cfg(unix)]
#[test]
fn native_old_bash_nested_heredoc_difference_is_not_reported_as_parser_equivalence() {
    let version = std::process::Command::new("/bin/sh")
        .arg("--version")
        .output()
        .unwrap();
    let old_bash = String::from_utf8_lossy(&version.stdout).contains("version 3.2.");
    let source = "printf '<%s>' \"$(cat <<'EOF'\n)\nEOF\n)\"";
    let output = std::process::Command::new("/bin/sh")
        .args(["-c", source])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        if old_bash { "<\nEOF\n)>" } else { "<)>" }
    );
    assert_eq!(preview(source)["decision"], "ask");
}

#[cfg(target_os = "macos")]
#[test]
fn fixed_native_bash_braces_confirm_order_and_literal_sequence_boundaries() {
    let output = std::process::Command::new("/bin/bash")
        .args([
            "--noprofile",
            "--norc",
            "-c",
            "printf '<%s>' {read,{other,write}} acme{a,b}{1,2} {3..1} {a..c}",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "<read><other><write><acmea1><acmea2><acmeb1><acmeb2><3><2><1><a><b><c>"
    );
    let output = std::process::Command::new("/bin/bash")
        .args([
            "--noprofile",
            "--norc",
            "-c",
            "printf '<%s>' $'write\\0ignored' $'wr\\x00ignored'ite $'wr\\000ignored\\\'more'ite $'wr\\x00\\xffignored'ite",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "<write><write><write><write>"
    );
    let output = std::process::Command::new("/bin/bash")
        .args([
            "--noprofile",
            "--norc",
            "-c",
            "printf '<%s>' $'\\cA' $'\\ca' $'\\c?' $'wr\\c@ignored'ite $'\\c['",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"<\x01><\x01><\x1f><write><\x1b>");
}

#[cfg(unix)]
#[test]
fn fixed_owned_nice_nohup_fixture_keeps_external_arguments_and_creates_no_output_file() {
    let root = assert_fs::TempDir::new().unwrap();
    let output = std::process::Command::new("/usr/bin/nohup")
        .args([
            "/usr/bin/nice",
            "-n",
            "0",
            "/usr/bin/printf",
            "<%s>",
            "write extra",
        ])
        .current_dir(root.path())
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", root.path())
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"<write extra>");
    assert!(!root.path().join("nohup.out").exists());
    let report = preview("/usr/bin/nohup /usr/bin/nice -n 0 acme write");
    assert_eq!(report["decision"], "deny");
    let report = preview("f() { acme write; }; nohup f");
    assert_eq!(report["decision"], "ask");
}

#[test]
fn every_structured_branch_retains_denials_and_allows_only_complete_literal_lists() {
    for source in [
        "acme read; acme write",
        "acme read && acme write",
        "acme read || acme write",
        "acme read | acme write",
        "(acme read; acme write)",
        "{ acme read; acme write; }",
        "if acme read; then acme read; else acme write; fi",
        "if acme read; then acme read; elif acme write; then acme read; fi",
        "while acme read; do acme write; done",
        "until acme read; do acme write; done",
        "acme write & acme read",
        "acme read\nacme write",
    ] {
        let result = preview(source);
        assert_eq!(result["decision"], "deny", "{source}");
        assert!(
            result["operations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|operation| operation["decision"] == "deny")
        );
    }
    assert_eq!(
        preview("acme read && (acme read | acme read)")["decision"],
        "allow"
    );
    assert_eq!(
        preview("acme read # harmless comment\nacme read")["decision"],
        "allow"
    );
    assert_eq!(preview("acme read; opaque")["decision"], "ask");
    assert_eq!(preview("acme write; opaque")["decision"], "deny");
    assert_eq!(
        preview("acme read > target; acme write")["decision"],
        "deny"
    );
    assert_eq!(preview("acme $(opaque); acme write")["decision"], "deny");
    assert_eq!(preview("acme \\\n read")["decision"], "allow");
    assert_eq!(
        preview("acme then fi for")["request"]["arguments"],
        json!(["then", "fi", "for"])
    );
}

#[test]
fn cd_to_the_current_directory_preserves_known_context_and_policy_refusals() {
    use crate::{configuration::Candidate, policy::Scope};
    for source in [
        "cd /acme; acme read",
        "cd . && acme read",
        "cd -- /acme && acme read",
        "command cd /acme && acme read",
    ] {
        let result = preview(source);
        assert_eq!(result["decision"], "allow", "{source}: {result}");
        let last = result["operations"].as_array().unwrap().last().unwrap();
        assert_eq!(last["context"]["cwd_known"], true, "{result}");
    }
    assert_eq!(preview("cd /acme; acme write")["decision"], "deny");
    for source in [
        "cd /other; cd /acme; acme read",
        "cd -; acme read",
        "cd; acme read",
        "cd child; acme read",
        "cd /acme/../acme; acme read",
    ] {
        assert_eq!(preview(source)["decision"], "ask", "{source}");
    }
    for decision in ["ask", "deny"] {
        let rules = json!({"schema_version":1,"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"],"rules":[
            {"id":"cd","decision":decision,"conditions":{"domain":"argv","executable":"cd","arguments":{"kind":"any"}}}
        ]}).to_string();
        let candidate = Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.yml\n")
            .unwrap()
            .capture(&[], Some(&rules))
            .unwrap();
        let evaluator = Evaluator::new(vec![(Scope::Project, candidate)]).unwrap();
        assert_eq!(
            analyze("cd /acme", Dialect::Bash, "/acme", "/acme", &evaluator).unwrap()["decision"],
            decision
        );
    }
}

#[test]
fn branch_subshell_pipeline_and_loop_state_is_conservative() {
    assert_eq!(
        preview("cd /other; acme read")["operations"][1]["context"]["cwd_known"],
        false
    );
    let result = preview("(cd /other); acme write");
    assert_eq!(result["decision"], "deny");
    assert_eq!(result["operations"][1]["context"]["cwd_known"], true);
    let result = preview("if acme read; then cd /other; else acme write; fi; acme read");
    assert_eq!(result["decision"], "deny");
    assert_eq!(result["operations"][2]["context"]["cwd_known"], true);
    assert_eq!(result["operations"][3]["context"]["cwd_known"], false);
    assert_eq!(preview("cd /other | acme write")["decision"], "deny");
    let result = preview("while acme read; do cd /other; done");
    assert_eq!(result["operations"][0]["context"]["cwd_known"], false);
    assert_eq!(preview("export X=y; acme write")["decision"], "deny");
    assert_eq!(preview("export X=y; acme read")["decision"], "ask");
}

#[test]
fn source_units_and_quoting_retain_exact_byte_boundaries() {
    let source = "acme 'λ'; sh -c 'acme write'";
    let result = preview(source);
    assert_eq!(result["decision"], "deny");
    for operation in result["operations"].as_array().unwrap() {
        let unit = usize::try_from(operation["source_unit"].as_u64().unwrap()).unwrap();
        let text = result["source_units"][unit]["source"].as_str().unwrap();
        let start = usize::try_from(operation["span"]["start"].as_u64().unwrap()).unwrap();
        let end = usize::try_from(operation["span"]["end"].as_u64().unwrap()).unwrap();
        assert!(text.get(start..end).is_some());
    }
    assert_eq!(result["source_units"][1]["source"], "acme write");
    assert_eq!(result["source_units"][1]["parent"]["source_unit"], 0);
    assert_eq!(preview("sh -c 'acme read'")["decision"], "ask");
    let result = preview("acme 'a;b' \"x|y\" ''");
    assert_eq!(result["operations"].as_array().unwrap().len(), 1);
    assert_eq!(result["request"]["arguments"], json!(["a;b", "x|y", ""]));
    let alias = analyze(
        "git -c 'alias.x=!acme write' x",
        Dialect::Bash,
        "/acme",
        "/acme",
        &evaluator(),
    )
    .unwrap();
    assert_eq!(alias["source_units"][0]["dialect"], "bash");
    assert_eq!(alias["source_units"][1]["dialect"], "posix");
}

#[test]
fn opaque_shapes_and_resource_failures_never_manufacture_allowance() {
    for source in [
        "acme $(danger)",
        "acme \"$(danger)\"",
        "acme `danger`",
        "acme *",
        "acme > file",
        "f() { acme read; }; f",
    ] {
        let result = preview(source);
        assert_eq!(result["decision"], "ask", "{source}");
        assert_eq!(result["coverage"], "partial");
    }
    for source in ["git status", "docker ps"] {
        let result = preview(source);
        assert_eq!(result["decision"], "ask", "unmatched command: {source}");
        assert_eq!(result["coverage"], "single-literal-invocation");
    }
    for source in [
        "acme &&",
        "if acme read; then acme read",
        "(acme read",
        "acme 'bad",
        "acme $(bad",
        "acme read | | acme read",
        "if acme read then acme read; fi",
        "if (acme read) then acme read; fi",
        "{ acme read }",
    ] {
        assert!(
            analyze(source, Dialect::Posix, "/acme", "/acme", &evaluator()).is_err(),
            "{source}"
        );
    }
    for source in [
        "acme read;".repeat(257),
        "acme read | ".repeat(100) + "acme read",
        format!("{}acme read{}", "(".repeat(40), ")".repeat(40)),
    ] {
        assert!(matches!(
            analyze(&source, Dialect::Posix, "/acme", "/acme", &evaluator()),
            Err(Error::Limit)
        ));
    }
    assert!(matches!(
        analyze_controlled(
            "acme read",
            Dialect::Posix,
            "/acme",
            None,
            "/acme",
            &evaluator(),
            &|| Err(Error::Interrupted)
        ),
        Err(Error::Interrupted)
    ));
}

proptest::proptest! {
    #[test]
    fn arbitrary_bounded_source_does_not_panic(source in ".{0,256}") {
        let evaluator = evaluator();
        let result = analyze(&source, Dialect::Posix, "/acme", "/acme", &evaluator);
        if let Ok(result) = result {
            proptest::prop_assert_eq!(&result["source"], &source);
            proptest::prop_assert!(result["operations"].as_array().unwrap().len() <= 256);
        }
    }
}

#[test]
fn project_restrictions_and_mid_analysis_cancellation_survive_aggregation() {
    use crate::{configuration::Candidate, policy::Scope};
    let input = |scope: Scope, decision: &str, arguments: Value| {
        let rules =
            json!({"schema_version":1,"rules":[{"id":"acme","decision":decision,"reason":"Acme restriction","conditions":{"domain":"argv","executable":"acme","arguments":arguments}}],"kind":"custom","capabilities":["argument-rules-v1","structured-rules-v1"]})
            .to_string();
        let candidate =
            Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.json\n").unwrap();
        (scope, candidate.capture(&[], Some(&rules)).unwrap())
    };
    let evaluator = Evaluator::new(vec![input(
        Scope::Project,
        "deny",
        json!({"kind":"exact","values":["write"]}),
    )])
    .unwrap();
    let result = analyze(
        "acme read; sh -c 'acme write'",
        Dialect::Posix,
        "/acme",
        "/acme",
        &evaluator,
    )
    .unwrap();
    assert_eq!(result["decision"], "deny");
    let matches = result["operations"][2]["evaluation"]["evaluation"]["matches"]
        .as_array()
        .unwrap();
    assert!(matches.iter().any(|rule| rule["applied"] == true));
    let result = analyze(
        "cd /other; acme write",
        Dialect::Posix,
        "/acme",
        "/acme",
        &evaluator,
    )
    .unwrap();
    assert_eq!(result["decision"], "deny");
    assert_eq!(
        result["operations"][1]["evaluation_model"],
        "cwd-independent-restriction-bound"
    );
    let candidate =
        Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.json\n").unwrap();
    let contextual = json!({"schema_version":1,"rules":[{"id":"cwd","decision":"deny","reason":"Acme context restriction","conditions":{"domain":"argv","executable":"acme","arguments":{"kind":"any"},"cwd":{"exact":{"root":"absolute","path":"/acme"}}}}],"kind":"custom","capabilities":["argument-rules-v1","location-v1","structured-rules-v1"]}).to_string();
    let contextual = Evaluator::new(vec![(
        Scope::Project,
        candidate.capture(&[], Some(&contextual)).unwrap(),
    )])
    .unwrap();
    let result = analyze(
        "cd /other; acme write",
        Dialect::Posix,
        "/acme",
        "/acme",
        &contextual,
    )
    .unwrap();
    assert_eq!(result["decision"], "ask");
    assert_eq!(
        result["operations"][1]["evaluation"]["evaluation"]["matches"],
        json!([])
    );
    let count = std::cell::Cell::new(0);
    let result = analyze_controlled(
        "acme read; acme read; acme write",
        Dialect::Posix,
        "/acme",
        None,
        "/acme",
        &evaluator,
        &|| {
            count.set(count.get() + 1);
            if count.get() == 10 {
                Err(Error::Interrupted)
            } else {
                Ok(())
            }
        },
    );
    assert!(matches!(result, Err(Error::Interrupted)));
    assert_eq!(count.get(), 10);
}

#[test]
fn indirect_tool_execution_retains_only_guaranteed_project_restrictions() {
    use crate::{configuration::Candidate, policy::Scope};
    let candidate =
        Candidate::from_yaml("schema_version: 1\npacks: []\nrules: rules.json\n").unwrap();
    for cwd in [None, Some("/acme")] {
        let mut matcher =
            json!({"executable":"acme","arguments":{"kind":"exact","values":["write"]}});
        if let Some(cwd) = cwd {
            matcher["cwd"] = json!({"exact":{"root":"absolute","path":cwd}});
        }
        matcher["domain"] = json!("argv");
        let rules = crate::policy::rules::document(&[
            json!({"id":"acme","decision":"deny","reason":"Acme restriction","conditions":matcher}),
        ])
        .unwrap()
        .to_string();
        let evaluator = Evaluator::new(vec![(
            Scope::Project,
            candidate.capture(&[], Some(&rules)).unwrap(),
        )])
        .unwrap();
        for source in [
            "git -c 'alias.x=!acme write' x",
            "git -c 'alias.x=!f() { acme \"$1\"; }; f' x write",
            "git submodule foreach --recursive 'acme write'",
            "git bisect run acme write",
            "docker container exec acme-container acme write",
            "docker run --entrypoint=sh acme-image -c 'acme write'",
        ] {
            let result = analyze(source, Dialect::Posix, "/acme", "/acme", &evaluator).unwrap();
            assert_eq!(
                result["decision"],
                if cwd.is_none() { "deny" } else { "ask" },
                "{source}"
            );
            let matched_rules: Vec<_> = result["operations"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|operation| operation["evaluation"]["evaluation"]["matches"].as_array())
                .flatten()
                .collect();
            if cwd.is_some() {
                assert!(matched_rules.is_empty(), "{source}");
            } else {
                assert!(
                    matched_rules.iter().any(|rule| rule["applied"] == true),
                    "{source}"
                );
            }
        }
        let result = analyze(
            "git -c 'alias.x=!acme \"$1\"' x write",
            Dialect::Posix,
            "/acme",
            "/acme",
            &evaluator,
        )
        .unwrap();
        assert_eq!(result["decision"], "ask");
        assert!(
            result["operations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|operation| operation["request"]["arguments"] == json!(["write", "write"]))
        );
    }
}

#[test]
fn preserves_literal_boundaries_and_quote_concatenation() {
    assert_eq!(
        literal_words(
            "acme '' a'b c'\"d\" \\* 'λ 😀' \"\\q \\$ \\\"\"",
            Dialect::Posix
        )
        .unwrap()
        .unwrap(),
        ["acme", "", "ab cd", "*", "λ 😀", "\\q $ \""]
    );
    assert_eq!(
        literal_words("acme a\\\nb \\\n x", Dialect::Bash)
            .unwrap()
            .unwrap(),
        ["acme", "ab", "x"]
    );
    assert_eq!(
        literal_words("acme a\u{a0}b a#b", Dialect::Posix)
            .unwrap()
            .unwrap(),
        ["acme", "a\u{a0}b", "a#b"]
    );
}

#[test]
fn unsupported_shapes_never_return_a_safe_prefix() {
    for source in [
        "acme safe; danger",
        "acme safe && danger",
        "acme | danger",
        "acme $(danger)",
        "acme $HOME",
        "acme > file",
        "acme *",
        "acme # comment",
        "acme\ndanger",
        "acme {x,y}",
        "acme <(danger)",
    ] {
        assert!(
            literal_words(source, Dialect::Posix).unwrap().is_none(),
            "{source}"
        );
    }
    assert!(
        literal_words("acme \"!event\"", Dialect::Bash)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        literal_words("acme \"!event\"", Dialect::Posix)
            .unwrap()
            .unwrap(),
        ["acme", "!event"]
    );
    for executable in ["env", "/bin/sh", "cd", "X=y", "", "sudo", "node", "eval"] {
        assert!(opaque_executable(executable));
    }
}

#[test]
fn malformed_supported_subset_and_budgets_fail() {
    for source in ["", "  ", "acme 'unfinished", "acme \"unfinished", "acme \\"] {
        assert!(literal_words(source, Dialect::Posix).is_err());
    }
    assert!(literal_words(&format!("acme {}", "x".repeat(4097)), Dialect::Posix).is_err());
    assert!(literal_words(&format!("acme {}", "x ".repeat(257)), Dialect::Posix).is_err());
}

#[cfg(unix)]
#[test]
fn harmless_posix_shell_probe_agrees_with_extracted_arguments() {
    // Only fixed printf source is executed here, never caller input.
    let source = "printf '%s\\0' '' a'b c'\"d\" \\* 'λ 😀' \"\\q \\$ \\\"\" a\\\nb";
    let words = literal_words(source, Dialect::Posix).unwrap().unwrap();
    let output = std::process::Command::new("/bin/sh")
        .args(["-c", source])
        .output()
        .unwrap();
    assert!(output.status.success());
    let actual: Vec<_> = output
        .stdout
        .split(|byte| *byte == 0)
        .take(words.len() - 2)
        .map(|word| String::from_utf8(word.to_vec()).unwrap())
        .collect();
    assert_eq!(actual, words[2..]);
}

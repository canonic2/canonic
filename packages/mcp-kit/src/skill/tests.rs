//! Static source admission, manifest consistency and namespace boundaries.

use super::*;

fn definition(name: &str) -> SkillDefinition {
    SkillDefinition::new(
        format!("skill://acme/{name}"),
        vec![
            SkillFile::text(
                "SKILL.md",
                format!(
                    "---\nname: {name}\ndescription: Inspect Acme status.\nmetadata:\n  owner: acme\nextra: [true, 12]\n---\nRead references/GUIDE.md.\n"
                ),
            ),
            SkillFile::text("references/GUIDE.md", "abc"),
            SkillFile::binary("assets/icon.bin", Arc::<[u8]>::from(&b"\0\xff"[..])),
        ],
    )
}

#[test]
fn source_bytes_frontmatter_manifest_and_binary_are_consistent() {
    let source = definition("status");
    let bytes = source.files[0].bytes.clone();
    let catalog = SkillCatalog::new(vec![source]).unwrap();
    let entry = catalog
        .query("skills/get", &json!({"uri":"skill://acme/status/SKILL.md"}))
        .unwrap()["skill"]
        .clone();
    assert_eq!(entry["frontmatter"]["extra"], json!([true, 12]));
    assert_eq!(entry["frontmatter"]["metadata"], json!({"owner":"acme"}));
    let guide = entry["resources"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["uri"] == "skill://acme/status/references/GUIDE.md")
        .unwrap();
    assert_eq!(
        guide["digest"],
        "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(guide["size"], 3);
    let read = catalog.read("skill://acme/status/SKILL.md").unwrap();
    assert_eq!(
        serde_json::to_value(read.result()).unwrap()["contents"][0]["text"]
            .as_str()
            .unwrap()
            .as_bytes(),
        bytes.as_ref()
    );
    let blob = catalog.read("skill://acme/status/assets/icon.bin").unwrap();
    assert_eq!(
        serde_json::to_value(blob.result()).unwrap()["contents"][0]["blob"],
        "AP8="
    );
    assert_eq!(
        catalog.query("skills/list", &json!({})).unwrap()["skills"][0],
        entry
    );
}

#[test]
fn source_profile_refuses_ambiguous_yaml_and_invalid_fields() {
    for yaml in [
        "name: status\nname: status\ndescription: x",
        "name: status\ndescription: x\nextra: {a: 1, a: 2}",
        "name: status\ndescription: x\nextra: &a [x]",
        "name: status\ndescription: x\nextra: !!str x",
        "name: status\ndescription: x\nextra: {1: x}",
        "name: status\ndescription: x\nextra: .nan",
        "name: status\ndescription: x\nmetadata: {key: 1}",
        "name: status\ndescription: ''",
        "name: Bad\ndescription: x",
        "name: bad--name\ndescription: x",
        "name: status\ndescription: x\ncompatibility: true",
        "name: status\ndescription: x\nextra: 18446744073709551616",
    ] {
        let source = SkillDefinition::new(
            "skill://acme/status",
            vec![SkillFile::text("SKILL.md", format!("---\n{yaml}\n---\n"))],
        );
        assert!(SkillCatalog::new(vec![source]).is_err(), "{yaml}");
    }
    let crlf = b"---\r\nname: status\r\ndescription: x\r\nextra: {yes: yes, number: 0x10}\r\n---\r\nbody\r\n";
    let value = source::frontmatter(crlf).unwrap();
    assert_eq!(value["extra"], json!({"yes":"yes","number":16}));
    assert_eq!(
        source::frontmatter(b"---\nname: status\ndescription: x\nextra: --1\n---\n").unwrap()["extra"],
        "--1"
    );
}

#[test]
fn source_limits_and_paths_refuse_before_publication() {
    for root in [
        "relative",
        "skill://acme/status/",
        "skill://acme/../status",
        "skill://acme/%2e%2e/status",
        "skill://acme/status?x",
        "skill://acme/other",
    ] {
        let mut source = definition("status");
        source.root = root.into();
        assert!(SkillCatalog::new(vec![source]).is_err(), "{root}");
    }
    for path in [
        "../secret",
        "/absolute",
        "a//b",
        "a\\b",
        "C:/secret",
        "a/./b",
        "a\0b",
    ] {
        let mut source = definition("status");
        source.files.push(SkillFile::text(path, "x"));
        assert!(SkillCatalog::new(vec![source]).is_err(), "{path}");
    }
    let mut source = definition("status");
    source.files.push(source.files[0].clone());
    assert!(SkillCatalog::new(vec![source]).is_err());
    let mut source = definition("status");
    source.files.push(SkillFile::binary(
        "huge",
        Arc::<[u8]>::from(vec![0; resource::MAX_BYTES + 1]),
    ));
    assert!(SkillCatalog::new(vec![source]).is_err());
    assert!(
        SkillCatalog::new(
            (0..=MAX_SKILLS)
                .map(|i| definition(&format!("skill-{i}")))
                .collect()
        )
        .is_err()
    );
    assert!(
        SkillCatalog::new(
            (0..MAX_SKILLS)
                .map(|i| definition(&format!("skill-{i}")))
                .collect()
        )
        .is_ok()
    );
}

#[test]
fn hidden_entries_pages_and_snapshot_cursors() {
    let mut hidden = definition("hidden");
    hidden.listed = false;
    let mut definitions: Vec<_> = (0..33).map(|i| definition(&format!("skill-{i}"))).collect();
    definitions.push(hidden);
    let catalog = SkillCatalog::new(definitions).unwrap();
    let first = catalog.query("skills/list", &json!({})).unwrap();
    assert_eq!(first["skills"].as_array().unwrap().len(), 32);
    let last = catalog
        .query("skills/list", &json!({"cursor":first["nextCursor"]}))
        .unwrap();
    assert_eq!(last["skills"].as_array().unwrap().len(), 1);
    assert!(last.get("nextCursor").is_none());
    assert!(
        catalog
            .query("skills/get", &json!({"uri":"skill://acme/hidden/SKILL.md"}))
            .is_ok()
    );
    let other = SkillCatalog::new(vec![definition("other")]).unwrap();
    assert!(
        other
            .query("skills/list", &json!({"cursor":first["nextCursor"]}))
            .is_err()
    );
    for params in [
        json!({"cursor":null}),
        json!({"cursor":"resources:64"}),
        json!({"x":1}),
    ] {
        assert!(catalog.query("skills/list", &params).is_err());
    }
}

#[test]
fn snapshot_cursors_refuse_visibility_only_changes() {
    let definitions: Vec<_> = (0..34).map(|i| definition(&format!("skill-{i}"))).collect();
    let catalog = SkillCatalog::new(definitions.clone()).unwrap();
    let first = catalog.query("skills/list", &json!({})).unwrap();
    let params = json!({"cursor":first["nextCursor"]});
    assert!(catalog.query("skills/list", &params).is_ok());

    let mut changed = definitions;
    changed[0].listed = false;
    let reloaded = SkillCatalog::new(changed).unwrap();
    let lookup = json!({"uri":"skill://acme/skill-0/SKILL.md"});
    assert_eq!(
        catalog.query("skills/get", &lookup).unwrap(),
        reloaded.query("skills/get", &lookup).unwrap()
    );
    assert_eq!(
        reloaded.query("skills/list", &params).unwrap_err().code,
        rpc::INVALID_PARAMS
    );
    let fresh = reloaded.query("skills/list", &json!({})).unwrap();
    let fresh_params = json!({"cursor":fresh["nextCursor"]});
    let last = reloaded.query("skills/list", &fresh_params).unwrap();
    assert_eq!(last["skills"][0]["uri"], "skill://acme/skill-33/SKILL.md");
    assert_eq!(
        catalog
            .query("skills/list", &fresh_params)
            .unwrap_err()
            .code,
        rpc::INVALID_PARAMS
    );
}

#[test]
fn nested_manifest_completeness_and_resource_ownership() {
    let nested = definition("nested");
    let mut nested = SkillDefinition {
        root: "skill://acme/status/nested".into(),
        ..nested
    };
    let mut parent = definition("status");
    assert!(matches!(
        SkillCatalog::new(vec![parent.clone(), nested.clone()]),
        Err(SkillError::Incomplete(_))
    ));
    parent
        .files
        .extend(nested.files.iter().cloned().map(|mut file| {
            file.path = format!("nested/{}", file.path);
            file
        }));
    let catalog = SkillCatalog::new(vec![parent.clone(), nested.clone()]).unwrap();
    assert!(catalog.owns("skill://acme/status/missing"));
    assert!(!catalog.owns("skill://acme/status-other/file"));
    let resources = ResourceCatalog::new(
        vec![Resource::new("skill://acme/status/missing", "x")],
        vec![],
    )
    .unwrap();
    assert!(catalog.compose(Some(&resources)).is_err());
    nested.files[1].bytes = Arc::from(&b"changed"[..]);
    assert!(matches!(
        SkillCatalog::new(vec![parent, nested]),
        Err(SkillError::Conflict(_))
    ));
}

#[test]
fn exact_file_and_manifest_count_boundaries() {
    let mut source = definition("status");
    while source.files.len() < MAX_FILES {
        source
            .files
            .push(SkillFile::text(format!("empty-{}", source.files.len()), ""));
    }
    assert!(SkillCatalog::new(vec![source.clone()]).is_ok());
    source.files.push(SkillFile::text("one-too-many", ""));
    assert!(SkillCatalog::new(vec![source]).is_err());
    let mut source = definition("status");
    source.files.push(SkillFile::binary(
        "one-mib",
        Arc::<[u8]>::from(vec![0; resource::MAX_BYTES]),
    ));
    assert!(SkillCatalog::new(vec![source]).is_ok());
    let mut sources = Vec::new();
    for i in 0..8 {
        let mut source = definition(&format!("skill-{i}"));
        while source.files.len() < MAX_FILES {
            source
                .files
                .push(SkillFile::text(format!("empty-{}", source.files.len()), ""));
        }
        sources.push(source);
    }
    assert!(SkillCatalog::new(sources.clone()).is_ok());
    sources.push(definition("extra"));
    assert!(SkillCatalog::new(sources).is_err());
}

#[test]
fn parser_depth_precision_frontmatter_and_byte_aware_page_budgets() {
    let yaml = format!(
        "---\nname: status\ndescription: x\nextra: {}0{}\n---\n",
        "[".repeat(32),
        "]".repeat(32)
    );
    assert!(source::frontmatter(yaml.as_bytes()).is_err());
    let yaml = format!(
        "---\nname: status\ndescription: x\nextra: {}0{}\n---\n",
        "[".repeat(31),
        "]".repeat(31)
    );
    assert!(source::frontmatter(yaml.as_bytes()).is_ok());
    for value in [".5", "1.25", "1e3", "-0.0"] {
        assert!(
            source::frontmatter(
                format!("---\nname: status\ndescription: x\nextra: {value}\n---\n").as_bytes()
            )
            .is_ok()
        );
    }
    assert!(
        source::frontmatter(
            b"---\nname: status\ndescription: x\nextra: 0.123456789012345678901\n---\n"
        )
        .is_err()
    );
    assert!(
        source::frontmatter(
            format!(
                "---\nname: status\ndescription: x\nextra: '{}'\n---\n",
                "x".repeat(65536)
            )
            .as_bytes()
        )
        .is_err()
    );
    let mut definitions = Vec::new();
    for i in 0..12 {
        let mut source = definition(&format!("skill-{i}"));
        for file in 0..50 {
            source
                .files
                .push(SkillFile::text(format!("{}-{file}", "a".repeat(3000)), ""));
        }
        definitions.push(source);
    }
    let catalog = SkillCatalog::new(definitions).unwrap();
    assert!(catalog.pages.len() > 1);
    assert!(
        catalog
            .pages
            .iter()
            .all(|page| json::measure(page, PAGE_BYTES - 16384, &|| Ok(())).is_ok())
    );
    assert!(catalog.pages[0]["skills"].as_array().unwrap().len() < 32);
}

fn source_with_bytes(name: &str, target: usize) -> SkillDefinition {
    let mut source = definition(name);
    let mut bytes: usize = source.files.iter().map(|file| file.bytes.len()).sum();
    let block = Arc::<[u8]>::from(vec![0; target.min(resource::MAX_BYTES)]);
    while bytes < target {
        let length = (target - bytes).min(resource::MAX_BYTES);
        let raw = if length == block.len() {
            block.clone()
        } else {
            Arc::from(&block[..length])
        };
        source.files.push(SkillFile::binary(
            format!("chunk-{}", source.files.len()),
            raw,
        ));
        bytes += length;
    }
    source
}

#[test]
fn raw_skill_and_catalog_storage_budgets_refuse_atomically() {
    let budgets = ByteBudgets {
        skill: 1024,
        stored: 4096,
        ..ByteBudgets::DEFAULT
    };
    assert!(SkillCatalog::with_budgets(vec![source_with_bytes("status", 1024)], budgets).is_ok());
    assert_eq!(
        SkillCatalog::with_budgets(vec![source_with_bytes("status", 1025)], budgets),
        Err(SkillError::Limit("skill raw bytes"))
    );
    let mut definitions: Vec<_> = (0..4)
        .map(|i| source_with_bytes(&format!("skill-{i}"), 1024))
        .collect();
    let budgets = ByteBudgets {
        skill: 1025,
        ..budgets
    };
    assert!(SkillCatalog::with_budgets(definitions.clone(), budgets).is_ok());
    definitions[3]
        .files
        .push(SkillFile::binary("overflow", Arc::<[u8]>::from(&b"x"[..])));
    assert_eq!(
        SkillCatalog::with_budgets(definitions, budgets),
        Err(SkillError::Limit("stored bytes"))
    );
}

fn entry_sized(name: &str, target: usize) -> SkillDefinition {
    let mut source = definition(name);
    for i in 0..64 {
        source
            .files
            .push(SkillFile::text(format!("{}-{i}", "a".repeat(3900)), ""));
    }
    source.files.push(SkillFile::text("p", ""));
    let catalog = SkillCatalog::new(vec![source.clone()]).expect("base entry fits");
    let entry = catalog.entries.values().next().expect("entry");
    let size = json::measure(entry, ENTRY_BYTES, &|| Ok(())).expect("entry bound");
    assert!(size < target && target - size < 4000);
    source
        .files
        .last_mut()
        .expect("padding file")
        .path
        .push_str(&"x".repeat(target - size));
    source
}

#[test]
fn entry_and_combined_manifest_metadata_budgets() {
    let mut source = entry_sized("status", ENTRY_BYTES);
    let catalog = SkillCatalog::new(vec![source.clone()]).unwrap();
    assert_eq!(
        json::measure(
            catalog.entries.values().next().unwrap(),
            ENTRY_BYTES,
            &|| Ok(())
        )
        .unwrap(),
        ENTRY_BYTES
    );
    source.files.last_mut().unwrap().path.push('x');
    assert!(SkillCatalog::new(vec![source]).is_err());
    let base = definition("skill-0");
    let catalog = SkillCatalog::new(vec![base]).unwrap();
    let entry_bytes = json::measure(
        catalog.entries.values().next().unwrap(),
        ENTRY_BYTES,
        &|| Ok(()),
    )
    .unwrap();
    let budgets = ByteBudgets {
        metadata: entry_bytes * 4,
        ..ByteBudgets::DEFAULT
    };
    let definitions: Vec<_> = (0..4).map(|i| definition(&format!("skill-{i}"))).collect();
    let catalog = SkillCatalog::with_budgets(definitions.clone(), budgets).unwrap();
    assert_eq!(
        catalog
            .entries
            .values()
            .map(|entry| json::measure(entry, ENTRY_BYTES, &|| Ok(())).unwrap())
            .sum::<usize>(),
        budgets.metadata
    );
    assert_eq!(
        SkillCatalog::with_budgets(
            definitions,
            ByteBudgets {
                metadata: budgets.metadata - 1,
                ..budgets
            }
        ),
        Err(SkillError::Limit("catalog metadata bytes"))
    );
}

//! Native CLI behavior on isolated projects, without any evaluator process.
mod serve;

use assert_cmd::Command;
use assert_fs::{TempDir, prelude::*};
use predicates::prelude::*;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn request(root: &Path) -> Value {
    json!({"root":root,"saved":root.with_extension("postcard"),"identity":"test","evaluator":"unused","node":"","options":{}})
}
fn invoke(request: &Value) -> Value {
    let output = Command::new(assert_cmd::cargo::cargo_bin!("workbench-server"))
        .arg("index")
        .write_stdin(request.to_string())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"records\""));
    serde_json::from_slice(&output.get_output().stdout).expect("native index fixture")
}
fn run(root: &Path) -> Value {
    invoke(&request(root))
}

fn fixture() -> (TempDir, PathBuf) {
    let temp = TempDir::new().expect("native index fixture");
    let project = temp.child("acme");
    project.create_dir_all().expect("native index fixture");
    let root = project.path().to_path_buf();
    fs::write(root.join("page.html"), "<p>Acme</p>").expect("native index fixture");
    page(&root, "a.workbench.ts", "First");
    (temp, root)
}
fn page(root: &Path, file: &str, title: &str) {
    fs::write(root.join(file), format!("import {{definePreview}} from '@canonic2/workbench'; export default definePreview({{id:'page',title:'{title}',adapter:'html',source:{{entry:'./page.html'}}}});")).expect("native index fixture");
}
#[test]
fn refreshes_directory_and_file_changes_and_ignores_corrupt_saves() {
    let (temp, root) = fixture();
    assert_eq!(run(&root)["timings"]["parsed"], 1);
    assert_eq!(run(&root)["timings"]["reused"], 1);
    page(&root, "a.workbench.ts", "Retitled page");
    assert_eq!(run(&root)["timings"]["parsed"], 1);
    fs::rename(root.join("a.workbench.ts"), root.join("b.workbench.ts"))
        .expect("native index fixture");
    assert_eq!(run(&root)["records"][0]["file"], "b.workbench.ts");
    page(&root, "c.workbench.ts", "Added");
    assert_eq!(
        run(&root)["records"]
            .as_array()
            .expect("native index fixture")
            .len(),
        2
    );
    for folder in [".hidden", "node_modules", "dist", "build", "coverage"] {
        fs::create_dir_all(root.join(folder)).expect("native index fixture");
        page(&root.join(folder), "hidden.workbench.ts", "Hidden");
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        root.join("b.workbench.ts"),
        root.join("linked.workbench.ts"),
    )
    .expect("native index fixture");
    assert_eq!(
        run(&root)["records"]
            .as_array()
            .expect("native index fixture")
            .len(),
        2
    );
    fs::write(root.with_extension("postcard"), "corrupt").expect("native index fixture");
    assert_eq!(run(&root)["timings"]["reused"], 0);
    fs::remove_file(root.join("c.workbench.ts")).expect("native index fixture");
    assert_eq!(
        run(&root)["records"]
            .as_array()
            .expect("native index fixture")
            .len(),
        1
    );
    temp.child("acme.postcard")
        .assert(predicate::path::exists());
    temp.close().expect("native index fixture");
}

#[test]
fn only_a_matching_generation_reuses_the_complete_answer() {
    let (_temp, root) = fixture();
    let mut input = request(&root);
    let first = invoke(&input);
    assert_ne!(first["generation"].as_str().unwrap(), "");
    input["cached_generation"] = first["generation"].clone();
    let reused = invoke(&input);
    assert_eq!(reused["reuse_answer"], true);
    assert_eq!(reused["records"], json!([]));
    assert_eq!(reused["timings"]["parsed"], 0);
    input["cached_generation"] = json!("another-generation");
    let rebuilt = invoke(&input);
    assert_eq!(rebuilt["reuse_answer"], false);
    assert_eq!(rebuilt["records"], first["records"]);
    assert_eq!(rebuilt["generation"], first["generation"]);
}

#[test]
fn runtime_handoff_preserves_literals_without_publishing_partial_custom_selection() {
    let (_temp, root) = fixture();
    fs::write(
        root.join("computed.workbench.ts"),
        "export default definePreview({title: title()});",
    )
    .unwrap();
    let mut input = request(&root);
    input["options"] = json!({"include": ["**/*.{workbench,preview}.ts"]});
    assert_eq!(invoke(&input)["needs_evaluator"], true);
    assert!(!root.with_extension("postcard").exists());
    input["options"] = json!({});
    let first = invoke(&input);
    assert_eq!(first["needs_evaluator"], true);
    assert_eq!(first["timings"]["parsed"], 1);
    let resumed = invoke(&input);
    assert_eq!(resumed["needs_evaluator"], true);
    assert_eq!(resumed["timings"]["parsed"], 0);
    assert_eq!(resumed["timings"]["reused"], 1);
    assert_eq!(resumed["records"], json!([]));
    assert_eq!(resumed["generation"], "");
}

#[test]
fn failed_cache_writes_leave_the_successful_list_available() {
    let (temp, root) = fixture();
    let blocked = temp.child("blocked");
    blocked.write_str("keep this file").unwrap();
    let mut input = request(&root);
    input["saved"] = json!(blocked.path().join("index.postcard"));
    let answer = invoke(&input);
    assert_eq!(answer["records"].as_array().unwrap().len(), 1);
    assert!(answer["timings"]["saveError"].is_string());
    assert_eq!(invoke(&input)["records"], answer["records"]);
    blocked.assert("keep this file");
}

#[test]
fn invalid_request_and_unavailable_evaluator_fail_at_the_cli_boundary() {
    Command::new(assert_cmd::cargo::cargo_bin!("workbench-server"))
        .arg("index")
        .write_stdin("{partial")
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid index request"));
    let (_temp, root) = fixture();
    fs::write(
        root.join("computed.workbench.ts"),
        "export default definePreview({title: title()});",
    )
    .unwrap();
    let mut input = request(&root);
    input["node"] = json!(root.join("missing-runtime"));
    Command::new(assert_cmd::cargo::cargo_bin!("workbench-server"))
        .arg("index")
        .write_stdin(input.to_string())
        .assert()
        .failure()
        .stderr(predicate::str::contains("start definition evaluator"));
}

#[cfg(unix)]
#[test]
fn an_evaluator_that_never_answers_is_stopped_at_the_deadline() {
    let (temp, root) = fixture();
    fs::write(
        root.join("a.workbench.ts"),
        "import {definePreview} from '@canonic2/workbench'; export default definePreview({id:'page',title:'Acme' + ' page',adapter:'html',source:{entry:'./page.html'}});",
    )
    .expect("native index fixture");
    let script = temp.child("hang.sh");
    script
        .write_str("exec sleep 30\n")
        .expect("native index fixture");
    let mut request = request(&root);
    request["node"] = json!("/bin/sh");
    request["evaluator"] = json!(script.path());
    request["deadline_ms"] = json!(300);
    let started = std::time::Instant::now();
    Command::new(assert_cmd::cargo::cargo_bin!("workbench-server"))
        .arg("index")
        .write_stdin(request.to_string())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "did not finish within 0.3 seconds",
        ));
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_gitignore_edit_changes_the_listing_without_a_folder_change() {
    let (_temp, root) = fixture();
    page(&root, "b.workbench.ts", "Second");
    fs::write(root.join(".gitignore"), "# nothing yet\n").expect("native index fixture");
    let first = run(&root);
    assert_eq!(first["records"].as_array().map(Vec::len), Some(2));
    // The same size, so only the stamp's time can tell the edit.
    std::thread::sleep(std::time::Duration::from_millis(20));
    fs::write(root.join(".gitignore"), "b.workbench.ts\n").expect("native index fixture");
    let mut cached = request(&root);
    cached["cached_generation"] = first["generation"].clone();
    let second = invoke(&cached);
    assert_eq!(second["reuse_answer"], false, "{second}");
    assert_eq!(second["records"][0]["file"], "a.workbench.ts");
    assert_eq!(second["records"].as_array().map(Vec::len), Some(1));
}

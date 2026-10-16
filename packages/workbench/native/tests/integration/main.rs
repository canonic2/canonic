//! Native CLI behavior on isolated projects, without any evaluator process.
use assert_cmd::Command;
use assert_fs::{TempDir, prelude::*};
use predicates::prelude::*;
use serde_json::{Value, json};
use std::{fs, path::Path};

fn run(root: &Path) -> Value {
    let request = json!({"root":root,"saved":root.with_extension("postcard"),"identity":"test","evaluator":"unused","node":"","options":{}});
    let output = Command::new(assert_cmd::cargo::cargo_bin!("workbench-native"))
        .arg("index")
        .write_stdin(request.to_string())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"records\""));
    serde_json::from_slice(&output.get_output().stdout).expect("native index fixture")
}
fn page(root: &Path, file: &str, title: &str) {
    fs::write(root.join(file), format!("import {{definePreview}} from '@canonic2/workbench'; export default definePreview({{id:'page',title:'{title}',adapter:'html',source:{{entry:'./page.html'}}}});")).expect("native index fixture");
}
#[test]
fn refreshes_directory_and_file_changes_and_ignores_corrupt_saves() {
    let temp = TempDir::new().expect("native index fixture");
    let project = temp.child("acme");
    project.create_dir_all().expect("native index fixture");
    let root = project.path().to_path_buf();
    fs::write(root.join("page.html"), "<p>Acme</p>").expect("native index fixture");
    page(&root, "a.workbench.ts", "First");
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

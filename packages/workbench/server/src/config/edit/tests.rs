//! Editing real files in temporary folders, read back through the reader.
use super::*;
use crate::refused::status_of;
use assert_fs::TempDir;

const MULTI: &str = "# Two spaces in one file
implementations:
  dev:
    kind: url
    base: http://localhost:3000
spaces:
  web:
    name: Acme Web
    collections:
      - name: Pages
        items:
          - label: Home
            src: index.html
  ui:
    name: Acme UI
    root: packages/ui
    # its own collections come later

previews: false
";

const SIZED: &str = "name: Acme # keep this comment
sizes:
  fit: true
  sidebar:
    width: 340
    height: fill
  tablet:
    width: 1024
    height: 1366
collections:
  - name: Interface
    items:
      # the sidebar is 340 wide
      - label: Sidebar
        src: sidebar.html
        sizes:
          - sidebar
          - laptop
      - label: Menu
        src: menu.html
        sizes:
          - popover:
              width: 280
              height: 360
          - fit
      - label: Card
        src: card.md
        sizes:
          - fit
      - label: Dashboard
        src: dashboard.html
";

fn project(body: &str) -> TempDir {
    let dir = TempDir::new().expect("temporary folder");
    std::fs::write(dir.path().join(FILE), body).expect("written");
    dir
}

fn at(dir: &TempDir) -> Location<'_> {
    Location {
        dir: dir.path(),
        key: None,
    }
}

fn space<'a>(dir: &'a TempDir, key: &'a str) -> Location<'a> {
    Location {
        dir: dir.path(),
        key: Some(key),
    }
}

fn config(at: Location<'_>) -> Value {
    read::resolved(at.dir, at.key)
        .expect("reads")
        .expect("a workbench.yaml")
}

fn body(dir: &TempDir) -> String {
    std::fs::read_to_string(dir.path().join(FILE)).expect("readable")
}

fn keys(config: &Value, field: &str) -> Vec<String> {
    config["sizes"]
        .as_array()
        .expect("sizes")
        .iter()
        .map(|size| size[field].as_str().expect("text").to_owned())
        .collect()
}

#[test]
fn icon_only_collections_survive_an_editor_save() {
    let dir = project("name: Acme\ncollections:\n  - name: Web App\n    icon: app-window\n");
    let authored = source(at(&dir))
        .expect("reads")
        .expect("source")
        .collections;
    update_collections(at(&dir), &authored).expect("saved");
    assert_eq!(
        source(at(&dir))
            .expect("reads")
            .expect("source")
            .collections,
        authored
    );
    assert_eq!(config(at(&dir))["collections"][0]["icon"], "app-window");
}

#[test]
fn editor_saves_keep_authored_lens_labels() {
    let dir = project(
        "name: Acme\npreviews:\n  lensLabel: Design\ncollections:\n  - name: Pages\n    items:\n      - label: Button\n        src: button.workbench.ts\n        lensLabel: Reference\n",
    );
    let authored = source(at(&dir))
        .expect("reads")
        .expect("source")
        .collections;
    update_collections(at(&dir), &authored).expect("saved");
    assert_eq!(
        config(at(&dir))["collections"][0]["items"][0]["lensLabel"],
        "Reference"
    );
}

#[test]
fn saving_pages_replaces_only_the_collections_block() {
    let dir = project(
        "name: Acme # keep this comment\nimplementations:\n  dev:\n    kind: url\n    base: http://localhost:3000\ncollections:\n  - name: Old\n    items:\n      - label: Old page\n        src: old.html\n",
    );
    update_collections(
        at(&dir),
        &json!([{ "name": "Pages", "icon": "file-text", "items": [{ "label": "Sign in", "src": "pages/sign-in.html", "sizes": ["laptop", "mobile"] }] }]),
    )
    .expect("saved");
    let written = body(&dir);
    assert!(written.contains("name: Acme # keep this comment"));
    assert!(written.contains("base: http://localhost:3000"));
    assert!(
        written.contains("sizes:\n          - laptop\n          - mobile"),
        "{written}"
    );
    assert_eq!(
        config(at(&dir))["collections"][0]["items"][0]["sizes"],
        json!(["laptop", "mobile"])
    );
}

#[test]
fn saving_pages_for_one_space_rewrites_only_that_space() {
    let dir = project(MULTI);
    let collections =
        json!([{ "name": "Components", "items": [{ "label": "Button", "src": "button.html" }] }]);
    let saved = update_collections(space(&dir, "ui"), &collections).expect("saved");
    assert_eq!(saved.collections, collections);
    let written = body(&dir);
    assert!(written.starts_with("# Two spaces in one file"));
    assert!(
        written.contains(
            "    # its own collections come later\n    collections:\n      - name: Components\n"
        ),
        "{written}"
    );
    assert!(written.ends_with("\n\npreviews: false\n"));
    assert_eq!(
        config(space(&dir, "web"))["collections"][0]["items"][0]["src"],
        "index.html"
    );
    assert_eq!(
        config(space(&dir, "ui"))["collections"][0]["items"][0]["src"],
        "button.html"
    );

    update_collections(
        space(&dir, "web"),
        &json!([{ "name": "Pages", "items": [{ "label": "About", "src": "about.html" }] }]),
    )
    .expect("saved");
    assert_eq!(
        config(space(&dir, "web"))["collections"][0]["items"][0]["label"],
        "About"
    );
    assert_eq!(
        config(space(&dir, "ui"))["collections"][0]["items"][0]["label"],
        "Button"
    );
    assert_eq!(
        config(space(&dir, "web"))["implementations"]["dev"]["base"],
        "http://localhost:3000"
    );
}

#[test]
fn a_save_that_would_not_parse_leaves_the_file_alone() {
    let dir = project("name: Acme\n");
    let error =
        update_collections(at(&dir), &json!([{ "name": "Two\nlines" }])).expect_err("refused");
    assert!(error.contains("one line"));
    assert_eq!(
        update_collections(at(&dir), &json!({})).expect_err("refused"),
        "collections must be a list."
    );
    assert_eq!(body(&dir), "name: Acme\n");
    let empty = TempDir::new().expect("temporary folder");
    assert_eq!(
        update_collections(at(&empty), &json!([])).expect_err("refused"),
        "There is no workbench.yaml at the project root."
    );
}

#[test]
fn saving_sizes_replaces_the_sizes_block_and_collections_only_when_a_page_changes() {
    let dir = project(SIZED);
    update_space_sizes(at(&dir), |sizes, collections| {
        add_size(
            sizes,
            collections,
            &json!({ "name": "Wide", "width": "fill", "height": 200 }),
        )
    })
    .expect("added");
    let written = body(&dir);
    assert!(written.contains("name: Acme # keep this comment"));
    assert!(
        written.contains("# the sidebar is 340 wide"),
        "collections were not rewritten"
    );
    assert_eq!(
        keys(&config(at(&dir)), "key"),
        ["fit", "sidebar", "tablet", "wide"]
    );

    update_space_sizes(at(&dir), |sizes, collections| {
        add_size(
            sizes,
            collections,
            &json!({ "name": "Narrow", "width": 200, "height": "fill", "page": "sidebar.html" }),
        )
    })
    .expect("added");
    assert!(body(&dir).contains("name: Acme # keep this comment"));
    assert_eq!(
        config(at(&dir))["collections"][0]["items"][0]["sizes"],
        json!(["sidebar", "narrow"])
    );

    update_space_sizes(at(&dir), |sizes, collections| {
        let narrow = sizes.expect("sizes")["narrow"].clone();
        update_sizes(
            sizes,
            collections,
            &json!([{ "key": "fit", "value": true }, { "key": "narrow", "value": narrow }]),
            &[],
        )
    })
    .expect("updated");
    let read = config(at(&dir));
    assert_eq!(keys(&read, "key"), ["fit", "narrow"]);
    assert_eq!(
        read["collections"][0]["items"][0]["sizes"],
        json!(["narrow"])
    );
}

#[test]
fn saving_sizes_for_one_space_of_several_writes_only_that_space() {
    let dir = project(MULTI);
    update_space_sizes(space(&dir, "ui"), |sizes, collections| {
        add_size(
            sizes,
            collections,
            &json!({ "name": "Tablet", "width": 1024, "height": 1366 }),
        )
    })
    .expect("added");
    let written = body(&dir);
    assert!(written.starts_with("# Two spaces in one file"));
    assert!(written.ends_with("\n\npreviews: false\n"));
    assert_eq!(
        keys(&config(space(&dir, "ui")), "key"),
        ["fit", "laptop", "mobile", "resizable", "tablet"]
    );
    assert_eq!(
        keys(&config(space(&dir, "web")), "key"),
        ["fit", "laptop", "mobile", "resizable"]
    );
    assert_eq!(
        config(space(&dir, "web"))["collections"][0]["items"][0]["src"],
        "index.html"
    );
}

#[test]
fn the_form_routes_read_and_save_the_pages() {
    let dir = project(
        "name: Acme\ncollections:\n  - name: Pages\n    items:\n      - label: Sign in\n        src: pages/sign-in.html\n",
    );
    let before = form(at(&dir)).expect("answers");
    assert_eq!(before["collections"][0]["items"][0]["label"], "Sign in");
    assert_eq!(
        before["sizes"],
        json!(["fit", "laptop", "mobile", "resizable"])
    );
    let saved = save_form(
        at(&dir),
        br#"{ "collections": [{ "name": "Pages", "icon": "file-text", "items": [{ "label": "Home", "src": "pages/home.html", "sizes": ["laptop", "mobile"] }] }] }"#,
    )
    .expect("saved");
    assert_eq!(saved["ok"], true);
    assert_eq!(
        config(at(&dir))["collections"][0]["items"][0]["sizes"],
        json!(["laptop", "mobile"])
    );
    assert!(body(&dir).contains("label: Home"));

    let empty = TempDir::new().expect("temporary folder");
    let missing = form(at(&empty)).expect_err("refused");
    assert_eq!(status_of(&missing, 400), 404);
    assert_eq!(
        missing.to_string(),
        "There is no workbench.yaml at the project root."
    );
    assert_eq!(
        status_of(&save_form(at(&dir), b"{").expect_err("refused"), 400),
        400
    );
}

#[test]
fn the_sizes_route_adds_and_updates_sizes_and_answers_with_them() {
    let dir = project(
        "name: Acme\ncollections:\n  - name: Interface\n    items:\n      - label: Sidebar\n        src: sidebar.html\n        sizes:\n          - fit\n",
    );
    let added = save_sizes(
        at(&dir),
        br#"{ "action": "add", "size": { "name": "Sidebar", "width": 340, "height": "fill", "icon": "panel-left", "button": true, "page": "sidebar.html" } }"#,
    )
    .expect("added");
    assert_eq!(added["key"], "sidebar");
    assert_eq!(
        keys(&added, "key"),
        ["fit", "laptop", "mobile", "resizable", "sidebar"]
    );
    assert_eq!(added["sizes"][4]["height"], "fill");
    assert_eq!(
        config(at(&dir))["pages"]["sidebar.html"]["sizes"],
        json!(["fit", "sidebar"])
    );

    let updated = save_sizes(
        at(&dir),
        br#"{ "action": "update", "sizes": [
            { "key": "sidebar", "value": { "label": "Panel", "width": 340, "height": "fill", "icon": "panel-left", "button": true } },
            { "key": "resizable", "value": true } ] }"#,
    )
    .expect("updated");
    assert_eq!(updated["key"], Value::Null);
    assert_eq!(keys(&updated, "label"), ["Panel", "Resizable"]);
    assert_eq!(
        config(at(&dir))["pages"]["sidebar.html"]["sizes"],
        json!(["sidebar"])
    );

    let refused =
        save_sizes(at(&dir), br#"{ "action": "update", "sizes": [] }"#).expect_err("refused");
    assert_eq!(status_of(&refused, 400), 400);
    assert!(refused.to_string().contains("at least one size"));
    assert_eq!(
        save_sizes(at(&dir), br#"{ "action": "rename" }"#)
            .expect_err("refused")
            .to_string(),
        "Ask to add a size or update the sizes."
    );
    assert_eq!(
        form(at(&dir)).expect("answers")["sizes"],
        json!(["sidebar", "resizable"])
    );
}

#[test]
fn sizes_the_local_file_sets_keep_their_committed_value() {
    let dir = project(SIZED);
    std::fs::write(
        dir.path().join("workbench.local.yaml"),
        "sizes:\n  sidebar:\n    width: 360\n",
    )
    .expect("written");
    save_sizes(
        at(&dir),
        br#"{ "action": "update", "sizes": [
            { "key": "sidebar", "value": { "width": 500, "height": "fill" } },
            { "key": "fit", "value": true } ] }"#,
    )
    .expect("updated");
    let written = body(&dir);
    assert!(
        written.contains(
            "sizes:\n  sidebar:\n    width: 340\n    height: fill\n  fit: true\ncollections:"
        ),
        "{written}"
    );
}

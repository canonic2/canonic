use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

fn sign_in() -> Value {
    json!({ "text": "- Page: Sign in — `pages/sign-in.html`\n- Scenario: Error — `error`\n- Lens: Design",
        "src": "pages/sign-in.html", "scenario": "error" })
}

fn home() -> Value {
    json!({ "text": "- Page: Home — `pages/home.html`\n- Scenario: Default — `default`\n- Lens: Design",
        "src": "pages/home.html" })
}

/// A clock the test moves by hand, in whole milliseconds.
struct Manual(Arc<AtomicU64>);

impl Manual {
    fn starting(at: u64) -> (Self, Clock) {
        let time = Arc::new(AtomicU64::new(at));
        let reading = time.clone();
        #[expect(clippy::cast_precision_loss, reason = "test times are small")]
        let clock: Clock = Arc::new(move || reading.load(Ordering::SeqCst) as f64);
        (Self(time), clock)
    }

    fn tick(&self, ms: u64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }
}

/// A fresh folder under the system temporary folder, removed when dropped.
struct Folder(PathBuf);

impl Folder {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "canonic-view-{name}-{}-{}",
            std::process::id(),
            system_clock()
        ));
        std::fs::create_dir_all(&path).expect("a temporary folder");
        Self(path)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn reports_no_view_until_a_canvas_says_what_it_shows() {
    let folder = Folder::new("none");
    let shown = AgentView::new(&folder.0, None);
    let root = std::path::absolute(&folder.0).expect("absolute");
    assert_eq!(
        shown.current(),
        json!({ "root": root.to_string_lossy(), "open": false, "view": null, "changedAt": null })
    );
    shown
        .report(&json!({ "client": "tab", "view": null }))
        .expect("reported");
    let current = shown.current();
    assert_eq!(current["open"], json!(true));
    assert_eq!(current["view"], Value::Null);
    assert!(current["changedAt"].is_string());
}

#[test]
fn follows_the_canvas_changed_most_recently_and_keeps_its_change_time_across_heartbeats() {
    // Date.UTC(2026, 9, 3, 12)
    let start = 1_791_028_800_000;
    let (time, clock) = Manual::starting(start);
    let shown = AgentView::with_clock(&std::env::temp_dir(), clock, None);
    shown
        .report(&json!({ "client": "tab", "view": sign_in() }))
        .expect("reported");
    time.tick(1000);
    shown
        .report(&json!({ "client": "browser", "view": home() }))
        .expect("reported");
    assert_eq!(shown.current()["view"]["src"], json!("pages/home.html"));

    time.tick(1000);
    shown
        .report(&json!({ "client": "tab", "view": sign_in() }))
        .expect("reported");
    assert_eq!(
        shown.current()["view"]["src"],
        json!("pages/home.html"),
        "a heartbeat is not a change"
    );

    time.tick(1000);
    let mut locked = sign_in();
    locked["scenario"] = json!("locked");
    shown
        .report(&json!({ "client": "tab", "view": locked }))
        .expect("reported");
    assert_eq!(shown.current()["view"]["scenario"], json!("locked"));
    assert_eq!(
        shown.current()["changedAt"],
        json!("2026-10-03T12:00:03.000Z")
    );
}

#[test]
fn reported_activity_dates_a_change_but_never_into_the_future() {
    let (_time, clock) = Manual::starting(10_000);
    let shown = AgentView::with_clock(&std::env::temp_dir(), clock, None);
    shown
        .report(&json!({ "client": "tab", "view": home(), "activity": 4000 }))
        .expect("reported");
    assert_eq!(
        shown.current()["changedAt"],
        json!("1970-01-01T00:00:04.000Z")
    );
    shown
        .report(&json!({ "client": "tab", "view": home(), "activity": 99_000 }))
        .expect("reported");
    assert_eq!(
        shown.current()["changedAt"],
        json!("1970-01-01T00:00:10.000Z")
    );
}

#[test]
fn drops_a_canvas_that_closes_or_stops_reporting() {
    let (time, clock) = Manual::starting(0);
    let shown = AgentView::with_clock(&std::env::temp_dir(), clock, Some(1000.0));
    shown
        .report(&json!({ "client": "tab", "view": sign_in() }))
        .expect("reported");
    shown
        .report(&json!({ "client": "browser", "view": home() }))
        .expect("reported");
    shown
        .report(&json!({ "client": "browser", "closed": true }))
        .expect("reported");
    assert_eq!(shown.current()["view"]["src"], json!("pages/sign-in.html"));
    time.tick(1001);
    assert_eq!(shown.current()["open"], json!(false));
}

#[test]
fn keeps_only_the_reference_and_its_ids() {
    let shown = AgentView::new(&std::env::temp_dir(), None);
    let mut noisy = home();
    noisy["html"] = json!("<p>secret</p>");
    noisy["scenario"] = json!(5);
    shown
        .report(&json!({ "client": "tab", "view": noisy }))
        .expect("reported");
    assert_eq!(shown.current()["view"], home());
    let error = shown
        .report(&json!({ "view": home() }))
        .expect_err("refused");
    assert!(error.to_string().contains("client id"));
}

#[test]
fn refuses_more_canvases_than_it_keeps() {
    let shown = AgentView::new(&std::env::temp_dir(), None);
    for client in 0..MAX_CLIENTS {
        shown
            .report(&json!({ "client": format!("tab-{client}"), "view": null }))
            .expect("reported");
    }
    let error = shown
        .report(&json!({ "client": "one-more", "view": null }))
        .expect_err("refused");
    assert_eq!(error.to_string(), "Too many open canvases.");
}

fn canvas_report(selected: &Value) -> Value {
    json!({
        "text": "- Canvas: two artboards", "version": 2, "canvas": "acme", "selected": selected,
        "artboards": [
            { "id": "a", "space": { "id": "s", "name": "Acme", "extra": 1 }, "src": "pages/home.html",
              "size": { "width": 393, "height": 852 }, "lens": "dev", "html": "<p>dropped</p>" },
            { "id": "b", "space": { "id": "s", "name": "Acme" }, "src": "pages/home.html", "scenario": "error",
              "artboard": "wide", "size": { "width": 1440, "height": 900 }, "status": "ready" },
        ],
    })
}

#[test]
fn a_canvas_report_keeps_each_artboard_without_truncating() {
    let cleaned = clean_view(&canvas_report(&json!("b")))
        .expect("clean")
        .expect("a view");
    assert_eq!(
        cleaned,
        json!({
            "text": "- Canvas: two artboards",
            "artboards": [
                { "id": "a", "space": { "id": "s", "name": "Acme" }, "src": "pages/home.html", "scenario": null,
                  "lens": "dev", "size": { "width": 393, "height": 852 }, "status": "loading" },
                { "id": "b", "space": { "id": "s", "name": "Acme" }, "src": "pages/home.html", "scenario": "error",
                  "artboard": "wide", "lens": null, "size": { "width": 1440, "height": 900 }, "status": "ready" },
            ],
            "version": 2, "canvas": "acme", "selected": "b",
        })
    );
    let error = |value: &Value| clean_view(value).expect_err("refused").to_string();
    assert!(error(&canvas_report(&json!("missing"))).contains("selected"));
    assert!(error(&json!({ "text": "x".repeat(MAX_TEXT + 1) })).contains("exceeds 100000"));
    assert_eq!(clean_view(&json!({ "text": "  " })).expect("clean"), None);
    let mut twice = canvas_report(&json!(null));
    twice["artboards"][1]["id"] = json!("a");
    assert_eq!(error(&twice), "Invalid artboard context.");
}

#[test]
fn announces_its_address_only_in_a_canonic_project_and_removes_only_its_own_file() {
    let plain = Folder::new("plain");
    let project = Folder::new("project");
    std::fs::create_dir_all(project.0.join(".canonic")).expect("a Canonic folder");
    let file = project.0.join(ANNOUNCE_FILE);
    let url_in = |file: &Path| -> Value {
        serde_json::from_str::<Value>(&std::fs::read_to_string(file).expect("announced"))
            .expect("JSON")["url"]
            .clone()
    };

    AgentView::new(&plain.0, None)
        .announce("http://127.0.0.1:3579/")
        .expect("nothing to do");
    assert!(!plain.0.join(".canonic").exists());

    let first = AgentView::new(&project.0, None);
    first.announce("http://127.0.0.1:3579/").expect("announced");
    assert_eq!(url_in(&file), json!("http://127.0.0.1:3579/"));
    let written = std::fs::read_to_string(&file).expect("announced");
    assert!(written.starts_with("{\n  \"url\": ") && written.ends_with("}\n"));
    let second = AgentView::new(&project.0, None);
    second
        .announce("http://127.0.0.1:3580/")
        .expect("announced");
    first.close();
    assert_eq!(url_in(&file), json!("http://127.0.0.1:3580/"));
    second.close();
    assert!(!file.exists());
}

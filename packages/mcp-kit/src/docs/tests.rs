//! Bundle building, staged preparation and the documentation tools' contracts.

use super::*;
use crate::Catalog;
use build::BuildError;
use format::{Index, Page};
use miniz_oxide::deflate::compress_to_vec;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

/// How many times each preparation stage ran.
#[derive(Debug)]
pub(super) struct Stages([AtomicUsize; 3]);

impl Stages {
    pub(super) const fn new() -> Self {
        Self([
            AtomicUsize::new(0),
            AtomicUsize::new(0),
            AtomicUsize::new(0),
        ])
    }
    pub(super) fn index(&self) {
        self.0[0].fetch_add(1, Ordering::Relaxed);
    }
    pub(super) fn text(&self) {
        self.0[1].fetch_add(1, Ordering::Relaxed);
    }
    pub(super) fn lowered(&self) {
        self.0[2].fetch_add(1, Ordering::Relaxed);
    }
    fn counts(&self) -> [usize; 3] {
        self.0.each_ref().map(|count| count.load(Ordering::Relaxed))
    }
}

/// A temporary guide folder, removed when dropped.
struct Folder(PathBuf);

impl Folder {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "acme-docs-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, relative: &str, content: impl AsRef<[u8]>) -> &Self {
        let path = self.0.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
        self
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const GUIDE: &str = "# Acme guide\r\n\r\nInstall Acme.\r\nRun `acme start`.";
const SETUP: &str = "Intro without a title line\n# Setup\n\nConfigure the Acme café.\n";
const FAQ: &str = "# FAQ\n\nWhy [brackets] and .* stay literal?\n  Spaced  text\n";

fn guides() -> Folder {
    let folder = Folder::new();
    folder
        .write("guide.md", GUIDE)
        .write("guides/setup.md", SETUP)
        .write("faq.md", FAQ)
        .write("notes.txt", "not published")
        .write(".draft.md", "# Hidden")
        .write(".cache/page.md", "# Hidden");
    folder
}

fn leak(bytes: Vec<u8>) -> &'static Bundle {
    Box::leak(Box::new(Bundle::new(Box::leak(bytes.into_boxed_slice()))))
}

fn docs_for(bytes: Vec<u8>) -> (Documentation, &'static Bundle) {
    let bundle = leak(bytes);
    (Documentation::new(bundle, "acme", "Acme", "1.2.3"), bundle)
}

fn sample() -> (Documentation, &'static Bundle) {
    docs_for(build::bundle(guides().path()).unwrap())
}

/// An interruption check that never interrupts; `call` takes this signature.
#[expect(
    clippy::unnecessary_wraps,
    reason = "matches the interruption-check signature"
)]
fn go() -> Result<(), Interrupted> {
    Ok(())
}

fn call(docs: &Documentation, tool: &str, arguments: &Value) -> Result<Value, DocsError> {
    docs.call(&format!("acme_docs_{tool}"), arguments, &go)
        .unwrap()
}

/// Every page's bytes, reassembled from reads of `limit` lines.
fn reassemble(docs: &Documentation, path: &str, limit: u64) -> String {
    let mut content = String::new();
    let mut start = 1;
    loop {
        let read = call(
            docs,
            "read",
            &json!({"path":path,"start_line":start,"limit":limit}),
        )
        .unwrap();
        content.push_str(read["content"].as_str().unwrap());
        match read["next_start_line"].as_u64() {
            Some(next) => start = next,
            None => return content,
        }
    }
}

#[test]
fn build_selects_markdown_in_bytewise_order_and_skips_dot_entries() {
    let folder = guides();
    let sources = build::sources(folder.path()).unwrap();
    let paths: Vec<_> = sources.iter().map(|source| source.path.as_str()).collect();
    assert_eq!(paths, ["faq.md", "guide.md", "guides/setup.md"]);
    assert!(
        sources
            .iter()
            .all(|source| source.file.starts_with(folder.path()))
    );
}

#[test]
fn build_output_is_deterministic_and_embedded_through_the_output_folder() {
    let folder = guides();
    let first = build::bundle(folder.path()).unwrap();
    assert_eq!(first, build::bundle(folder.path()).unwrap());
    let out = Folder::new();
    let sources = build::embed_into(folder.path(), out.path()).unwrap();
    assert_eq!(sources.len(), 3);
    assert_eq!(std::fs::read(out.path().join(FILE_NAME)).unwrap(), first);
}

#[test]
fn build_refuses_unusable_folders_and_pages() {
    let missing = Folder::new();
    assert!(matches!(
        build::sources(&missing.path().join("absent")),
        Err(BuildError::Io { path, source })
            if path == "." && source.kind() == std::io::ErrorKind::NotFound
    ));
    let file = Folder::new();
    file.write("guide.md", "# Guide");
    assert!(matches!(
        build::sources(&file.path().join("guide.md")),
        Err(BuildError::Root)
    ));
    assert!(matches!(
        build::sources(Folder::new().path()),
        Err(BuildError::Empty)
    ));
    let only_hidden = Folder::new();
    only_hidden
        .write(".hidden.md", "# Hidden")
        .write("notes.txt", "text");
    assert!(matches!(
        build::sources(only_hidden.path()),
        Err(BuildError::Empty)
    ));

    let text = Folder::new();
    text.write("bad.md", [0xff, 0xfe]);
    assert!(
        matches!(build::bundle(text.path()), Err(BuildError::Text { path }) if path == "bad.md")
    );

    let line = Folder::new();
    line.write(
        "long.md",
        format!("ok\n{}\n", "a".repeat(MAX_LINE_BYTES + 1)),
    );
    assert!(matches!(
        build::bundle(line.path()),
        Err(BuildError::Line { line: 2, .. })
    ));
    let at_limit = Folder::new();
    at_limit.write("long.md", format!("{}\r\n", "a".repeat(MAX_LINE_BYTES)));
    assert!(build::bundle(at_limit.path()).is_ok());

    let page = Folder::new();
    let lines = "a".repeat(1023) + "\n";
    page.write("big.md", lines.repeat(MAX_PAGE_BYTES / 1024) + "a");
    assert!(matches!(
        build::bundle(page.path()),
        Err(BuildError::Page { .. })
    ));

    let path = Folder::new();
    // Folder names stay under filesystem limits while the path exceeds the bound.
    let deep = format!(
        "{}/{}/{}.md",
        "a".repeat(200),
        "b".repeat(200),
        "c".repeat(200)
    );
    path.write(&deep, "# Deep");
    assert!(matches!(
        build::bundle(path.path()),
        Err(BuildError::Path { .. })
    ));

    let many = Folder::new();
    for number in 0..=MAX_PAGES {
        many.write(&format!("page-{number:04}.md"), "# Page");
    }
    assert!(
        matches!(build::bundle(many.path()), Err(BuildError::Pages { count }) if count == MAX_PAGES + 1)
    );
}

#[test]
fn build_refuses_more_than_the_total_text_budget() {
    let folder = Folder::new();
    let page = ("a".repeat(1023) + "\n").repeat(MAX_PAGE_BYTES / 1024);
    for number in 0..=MAX_TOTAL_BYTES / MAX_PAGE_BYTES {
        folder.write(&format!("page-{number:02}.md"), &page);
    }
    assert!(matches!(
        build::bundle(folder.path()),
        Err(BuildError::Total)
    ));
}

#[cfg(unix)]
#[test]
fn build_refuses_symbolic_links_and_unsafe_names() {
    use std::os::unix::fs::symlink;
    let target = guides();
    let root = Folder::new();
    let linked_root = root.path().join("docs");
    symlink(target.path(), &linked_root).unwrap();
    assert!(matches!(
        build::sources(&linked_root),
        Err(BuildError::Root)
    ));

    let file = guides();
    symlink(file.path().join("faq.md"), file.path().join("linked.md")).unwrap();
    assert!(
        matches!(build::sources(file.path()), Err(BuildError::Symlink { path }) if path == "linked.md")
    );

    let folder = guides();
    symlink(target.path(), folder.path().join("more")).unwrap();
    assert!(
        matches!(build::sources(folder.path()), Err(BuildError::Symlink { path }) if path == "more")
    );

    for name in ["back\\slash.md", "bell\u{7}.md"] {
        let unsafe_name = Folder::new();
        unsafe_name.write(name, "# Name");
        assert!(
            matches!(
                build::sources(unsafe_name.path()),
                Err(BuildError::Name { .. })
            ),
            "{name}"
        );
    }
}

#[test]
fn preparation_is_lazy_staged_and_happens_once() {
    let (docs, bundle) = sample();
    Catalog::new(docs.tools()).unwrap();
    assert!(docs.call("acme_status", &json!({}), &go).is_none());
    assert_eq!(bundle.stages.counts(), [0, 0, 0]);

    call(&docs, "list", &json!({})).unwrap();
    call(&docs, "list", &json!({"limit":1})).unwrap();
    assert_eq!(bundle.stages.counts(), [1, 0, 0]);
    call(&docs, "read", &json!({"path":"faq.md"})).unwrap();
    assert_eq!(bundle.stages.counts(), [1, 1, 0]);
    for _ in 0..3 {
        call(&docs, "search", &json!({"query":"acme"})).unwrap();
        call(&docs, "read", &json!({"path":"guide.md"})).unwrap();
    }
    assert_eq!(bundle.stages.counts(), [1, 1, 1]);
}

#[test]
fn concurrent_first_calls_prepare_each_stage_once() {
    let (docs, bundle) = sample();
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| call(&docs, "search", &json!({"query":"acme"})).unwrap());
        }
    });
    assert_eq!(bundle.stages.counts(), [1, 1, 1]);
}

#[test]
fn invalid_arguments_are_refused_before_preparation() {
    let (docs, bundle) = sample();
    for (tool, arguments) in [
        ("list", json!({"offset":-1})),
        ("list", json!({"limit":0})),
        ("list", json!({"limit":101})),
        ("list", json!({"offset":1.5})),
        ("list", json!({"limit":null})),
        ("list", json!({"offset":MAX_POSITION + 1})),
        ("list", json!({"project":"acme"})),
        ("list", json!([])),
        ("read", json!({})),
        ("read", json!({"path":""})),
        ("read", json!({"path":"faq.md","start_line":0})),
        ("read", json!({"path":"faq.md","limit":401})),
        ("search", json!({})),
        ("search", json!({"query":" \t "})),
        ("search", json!({"query":"acme\nguide"})),
        ("search", json!({"query":"acme\r"})),
        ("search", json!({"query":"a".repeat(MAX_QUERY_BYTES + 1)})),
        ("search", json!({"query":"acme","path":null})),
        ("search", json!({"query":"acme","limit":101})),
    ] {
        let error = call(&docs, tool, &arguments).unwrap_err();
        assert!(
            matches!(error, DocsError::InvalidArguments(_)),
            "{tool} {arguments}"
        );
    }
    assert_eq!(bundle.stages.counts(), [0, 0, 0]);
    assert!(
        call(
            &docs,
            "search",
            &json!({"query":"a".repeat(MAX_QUERY_BYTES)})
        )
        .is_ok()
    );
}

#[test]
fn defaults_equal_their_explicit_values() {
    let (docs, _) = sample();
    for (tool, implicit, explicit) in [
        ("list", json!({}), json!({"offset":0,"limit":50})),
        (
            "search",
            json!({"query":"a"}),
            json!({"query":"a","offset":0,"limit":50}),
        ),
        (
            "read",
            json!({"path":"faq.md"}),
            json!({"path":"faq.md","start_line":1,"limit":100}),
        ),
    ] {
        assert_eq!(call(&docs, tool, &implicit), call(&docs, tool, &explicit));
    }
}

#[test]
fn list_pages_through_titles_line_counts_and_version() {
    let (docs, _) = sample();
    let all = call(&docs, "list", &json!({})).unwrap();
    assert_eq!(
        all,
        json!({"version":"1.2.3","total":3,"next_offset":null,"documents":[
            {"path":"faq.md","title":"FAQ","total_lines":4},
            {"path":"guide.md","title":"Acme guide","total_lines":4},
            {"path":"guides/setup.md","title":"Setup","total_lines":4},
        ]})
    );
    let mut rows = Vec::new();
    let mut offset = Some(0);
    while let Some(next) = offset {
        let page = call(&docs, "list", &json!({"offset":next,"limit":1})).unwrap();
        rows.extend(page["documents"].as_array().unwrap().iter().cloned());
        offset = page["next_offset"].as_u64();
    }
    assert_eq!(Value::Array(rows), all["documents"]);
    let past = call(&docs, "list", &json!({"offset":MAX_POSITION})).unwrap();
    assert_eq!(
        (past["documents"].clone(), past["next_offset"].clone()),
        (json!([]), Value::Null)
    );
}

#[test]
fn reads_reproduce_every_page_byte_for_byte() {
    let (docs, _) = sample();
    for (path, source) in [
        ("faq.md", FAQ),
        ("guide.md", GUIDE),
        ("guides/setup.md", SETUP),
    ] {
        for limit in [1, 2, 3, 400] {
            assert_eq!(reassemble(&docs, path, limit), source, "{path} {limit}");
        }
    }
    let read = call(
        &docs,
        "read",
        &json!({"path":"guide.md","start_line":2,"limit":2}),
    )
    .unwrap();
    assert_eq!(
        read,
        json!({"version":"1.2.3","path":"guide.md","title":"Acme guide","start_line":2,
            "total_lines":4,"content":"\r\nInstall Acme.\r\n","next_start_line":4})
    );
    let past = call(&docs, "read", &json!({"path":"guide.md","start_line":5})).unwrap();
    assert_eq!(
        (past["content"].clone(), past["next_start_line"].clone()),
        (json!(""), Value::Null)
    );
}

#[test]
fn empty_pages_have_zero_lines_beside_others_and_alone() {
    let mixed = Folder::new();
    mixed.write("empty.md", "").write("faq.md", FAQ);
    let alone = Folder::new();
    alone.write("a.md", "").write("b.md", "");
    for (folder, others) in [(mixed, 1), (alone, 0)] {
        let (docs, _) = docs_for(build::bundle(folder.path()).unwrap());
        let list = call(&docs, "list", &json!({})).unwrap();
        let empty = &list["documents"][0];
        assert_eq!(
            (empty["title"].clone(), empty["total_lines"].clone()),
            (json!(""), json!(0))
        );
        let path = empty["path"].as_str().unwrap();
        let read = call(&docs, "read", &json!({"path":path})).unwrap();
        assert_eq!(
            (read["content"].clone(), read["next_start_line"].clone()),
            (json!(""), Value::Null)
        );
        let found = call(&docs, "search", &json!({"query":"faq"})).unwrap();
        assert_eq!(found["total"], json!(others));
        let scoped = call(&docs, "search", &json!({"query":"a","path":path})).unwrap();
        assert_eq!(scoped["total"], json!(0));
    }
}

#[test]
fn paths_must_match_a_bundled_page_exactly() {
    let (docs, _) = sample();
    for path in [
        "../faq.md",
        "/etc/passwd",
        "./faq.md",
        "docs/faq.md",
        "FAQ.md",
        "guides\\setup.md",
        "guides",
    ] {
        for (tool, arguments) in [
            ("read", json!({"path":path})),
            ("search", json!({"path":path,"query":"acme"})),
        ] {
            let error = call(&docs, tool, &arguments).unwrap_err();
            assert_eq!(
                error,
                DocsError::NotFound {
                    list_tool: "acme_docs_list".into()
                }
            );
            assert_eq!(
                error.to_string(),
                "Unknown documentation page; call acme_docs_list."
            );
        }
    }
}

#[test]
fn search_is_literal_case_insensitive_and_keeps_surrounding_spaces() {
    let (docs, _) = sample();
    let acme = call(&docs, "search", &json!({"query":"ACME"})).unwrap();
    assert_eq!(acme["total"], 4);
    assert_eq!(
        acme["matches"],
        json!([
            {"path":"guide.md","line":1,"text":"# Acme guide"},
            {"path":"guide.md","line":3,"text":"Install Acme."},
            {"path":"guide.md","line":4,"text":"Run `acme start`."},
            {"path":"guides/setup.md","line":4,"text":"Configure the Acme café."},
        ])
    );
    for (query, total) in [
        ("[brackets]", 1),
        (".*", 1),
        ("  spaced  ", 1),
        (" spaced  text ", 0),
        ("CAFÉ", 1),
    ] {
        assert_eq!(
            call(&docs, "search", &json!({"query":query})).unwrap()["total"],
            total,
            "{query}"
        );
    }
    let filtered = call(
        &docs,
        "search",
        &json!({"query":"acme","path":"guides/setup.md"}),
    )
    .unwrap();
    assert_eq!(filtered["total"], 1);

    let mut rows = Vec::new();
    let mut offset = Some(0);
    while let Some(next) = offset {
        let page = call(
            &docs,
            "search",
            &json!({"query":"acme","offset":next,"limit":1}),
        )
        .unwrap();
        assert_eq!(page["total"], 4);
        rows.extend(page["matches"].as_array().unwrap().iter().cloned());
        offset = page["next_offset"].as_u64();
    }
    assert_eq!(Value::Array(rows), acme["matches"]);
}

#[test]
fn large_lines_end_results_early_with_continuation() {
    let folder = Folder::new();
    // Sixteen lines of about 60 KiB fit one page but not one result.
    let line = format!("acme {}\n", "a".repeat(60 * 1024));
    folder.write("wide.md", line.repeat(16));
    let (docs, _) = docs_for(build::bundle(folder.path()).unwrap());
    let read = call(&docs, "read", &json!({"path":"wide.md","limit":400})).unwrap();
    let returned = read["content"].as_str().unwrap();
    assert!(returned.len() <= MAX_RESULT_TEXT_BYTES && returned.ends_with('\n'));
    let taken = returned.matches('\n').count();
    assert!(taken < 16);
    assert_eq!(read["next_start_line"], taken + 1);
    assert_eq!(reassemble(&docs, "wide.md", 400), line.repeat(16));

    let search = call(&docs, "search", &json!({"query":"acme","limit":100})).unwrap();
    let rows = search["matches"].as_array().unwrap().len();
    assert!(rows < 16);
    assert_eq!(
        (search["total"].clone(), search["next_offset"].clone()),
        (json!(16), json!(rows))
    );
}

#[test]
fn interruption_partway_through_list_discards_rows_and_reuses_index() {
    let (docs, bundle) = sample();
    let calls = AtomicUsize::new(0);
    let check = || {
        if calls.fetch_add(1, Ordering::Relaxed) < 2 {
            Ok(())
        } else {
            Err(Interrupted)
        }
    };
    assert_eq!(
        docs.call("acme_docs_list", &json!({}), &check).unwrap(),
        Err(DocsError::Cancelled)
    );
    assert_eq!(calls.load(Ordering::Relaxed), 3);
    assert_eq!(call(&docs, "list", &json!({})).unwrap()["total"], 3);
    assert_eq!(bundle.stages.counts(), [1, 0, 0]);
}

#[test]
fn interruption_after_preparation_cancels_a_read_past_the_first_line() {
    let (docs, bundle) = sample();
    let calls = AtomicUsize::new(0);
    let once = || {
        if calls.fetch_add(1, Ordering::Relaxed) == 0 {
            Ok(())
        } else {
            Err(Interrupted)
        }
    };
    let arguments = json!({"path":"faq.md","start_line":2,"limit":1});
    assert_eq!(
        docs.call("acme_docs_read", &arguments, &once).unwrap(),
        Err(DocsError::Cancelled)
    );
    assert_eq!(bundle.stages.counts(), [1, 1, 0]);

    let lines = "acme\n".repeat(CHECK_INTERVAL * 2);
    let long = Folder::new();
    long.write("long.md", &lines);
    let (docs, _) = docs_for(build::bundle(long.path()).unwrap());
    let calls = AtomicUsize::new(0);
    let twice = || {
        if calls.fetch_add(1, Ordering::Relaxed) < 2 {
            Ok(())
        } else {
            Err(Interrupted)
        }
    };
    let skipped = json!({"path":"long.md","start_line":CHECK_INTERVAL * 2,"limit":1});
    assert_eq!(
        docs.call("acme_docs_read", &skipped, &twice).unwrap(),
        Err(DocsError::Cancelled)
    );
    assert_eq!(calls.load(Ordering::Relaxed), 3);
}

#[test]
fn interruption_cancels_without_partial_results_and_keeps_prepared_state() {
    let (docs, bundle) = sample();
    let stop = || Err(Interrupted);
    for (tool, arguments) in [
        ("list", json!({})),
        ("search", json!({"query":"acme"})),
        ("read", json!({"path":"faq.md"})),
    ] {
        let outcome = docs
            .call(&format!("acme_docs_{tool}"), &arguments, &stop)
            .unwrap();
        assert_eq!(outcome, Err(DocsError::Cancelled), "{tool}");
    }
    call(&docs, "search", &json!({"query":"acme"})).unwrap();
    let calls = AtomicUsize::new(0);
    let later = || {
        if calls.fetch_add(1, Ordering::Relaxed) == 0 {
            Ok(())
        } else {
            Err(Interrupted)
        }
    };
    let outcome = docs
        .call("acme_docs_search", &json!({"query":"acme"}), &later)
        .unwrap();
    assert_eq!(outcome, Err(DocsError::Cancelled));
    call(&docs, "read", &json!({"path":"faq.md"})).unwrap();
    assert_eq!(bundle.stages.counts(), [1, 1, 1]);
}

/// A hand-built bundle with an arbitrary index and compressed text.
fn crafted(index: &Index, text: &[u8]) -> Vec<u8> {
    let index = serde_json::to_vec(index).unwrap();
    let mut bytes = MAGIC.to_vec();
    bytes.extend_from_slice(&u32::try_from(index.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&index);
    bytes.extend_from_slice(&compress_to_vec(text, 10));
    bytes
}

fn page(path: &str, start: usize, end: usize, lines: usize, title: &str) -> Page {
    Page {
        path: path.into(),
        start,
        end,
        lines,
        title: title.into(),
    }
}

#[test]
fn corrupt_bundles_fail_every_call_without_affecting_other_tools() {
    let good = build::bundle(guides().path()).unwrap();
    let mut wrong_marker = good.clone();
    wrong_marker[0] ^= 1;
    let truncated = good[..good.len() - 8].to_vec();
    let text = "é\n# Acme\n";
    let corrupt_text = [
        // Index-level: empty, misordered and noncontiguous pages.
        crafted(
            &Index {
                size: 0,
                pages: vec![],
            },
            b"",
        ),
        crafted(
            &Index {
                size: 2,
                pages: vec![page("b.md", 0, 1, 1, ""), page("a.md", 1, 2, 1, "")],
            },
            b"ab",
        ),
        crafted(
            &Index {
                size: 2,
                pages: vec![page("a.md", 1, 2, 1, "")],
            },
            b"ab",
        ),
        // Text-level: a range off a character boundary, wrong size, invalid UTF-8, stale metadata.
        crafted(
            &Index {
                size: text.len(),
                pages: vec![
                    page("a.md", 0, 1, 1, ""),
                    page("b.md", 1, text.len(), 2, "Acme"),
                ],
            },
            text.as_bytes(),
        ),
        crafted(
            &Index {
                size: text.len() + 1,
                pages: vec![page("a.md", 0, text.len() + 1, 2, "Acme")],
            },
            text.as_bytes(),
        ),
        crafted(
            &Index {
                size: 2,
                pages: vec![page("a.md", 0, 2, 1, "")],
            },
            &[0xff, 0xfe],
        ),
        crafted(
            &Index {
                size: text.len(),
                pages: vec![page("a.md", 0, text.len(), 2, "Other")],
            },
            text.as_bytes(),
        ),
    ];
    for bytes in [b"".to_vec(), wrong_marker, truncated]
        .into_iter()
        .chain(corrupt_text)
    {
        let (docs, _) = docs_for(bytes);
        for _ in 0..2 {
            let search = call(&docs, "search", &json!({"query":"acme"}));
            let read = call(&docs, "read", &json!({"path":"a.md"}));
            assert_eq!(search, Err(DocsError::Unavailable));
            assert!(matches!(
                read,
                Err(DocsError::Unavailable | DocsError::NotFound { .. })
            ));
        }
        assert!(docs.call("acme_status", &json!({}), &go).is_none());
    }
    let (valid, _) = docs_for(crafted(
        &Index {
            size: text.len(),
            pages: vec![page("a.md", 0, text.len(), 2, "Acme")],
        },
        text.as_bytes(),
    ));
    assert_eq!(
        call(&valid, "read", &json!({"path":"a.md"})).unwrap()["content"],
        text
    );
}

#[test]
fn tools_publish_strict_read_only_schemas() {
    let (docs, _) = sample();
    let catalog = Catalog::new(docs.tools()).unwrap();
    for name in ["acme_docs_list", "acme_docs_search", "acme_docs_read"] {
        let tool = catalog.get(name).unwrap();
        assert!(docs.handles(name));
        assert!(tool.read_only());
        assert_eq!(tool.annotations.destructive_hint, Some(false));
        assert_eq!(tool.annotations.open_world_hint, Some(false));
        assert_eq!(tool.input_schema["additionalProperties"], false);
        assert!(tool.description.contains("Acme"));
    }
    assert!(!docs.handles("acme_docs_open"));
    assert_eq!(docs.list_tool(), "acme_docs_list");
}

#[test]
fn default_results_carry_structured_data_or_recovery_text() {
    let (docs, _) = sample();
    let success = Documentation::result(call(&docs, "list", &json!({"limit":1})));
    assert!(!success.is_error);
    assert_eq!(success.structured_content.unwrap()["total"], 3);
    let failure = Documentation::result(call(&docs, "read", &json!({"path":"missing.md"})));
    assert!(failure.is_error);
    assert_eq!(
        serde_json::to_value(&failure.content).unwrap(),
        json!([{"type":"text","text":"Unknown documentation page; call acme_docs_list."}])
    );
}

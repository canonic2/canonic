//! Print ordering, exact acceptance and ambiguity boundaries without terminal ownership.
use canonic_tui::inline_composition::{Invalid, Part, PrintLedger};
fn part(id: &str, revision: u64, settled: bool) -> Part {
    Part {
        id: id.into(),
        revision,
        label: format!("card {id}"),
        text: format!("{id} revision {revision}"),
        settled,
    }
}
#[test]
fn settled_parts_wait_for_predecessors_and_updates_append_named_corrections() {
    let mut ledger = PrintLedger::new();
    ledger
        .replace(vec![
            part("a", 1, true),
            part("b", 1, false),
            part("c", 1, true),
        ])
        .unwrap();
    let first = ledger.ready().unwrap();
    assert_eq!(first.lines, ["a revision 1"]);
    assert!(ledger.ready().is_none());
    assert!(ledger.acknowledge(&first));
    assert_eq!(
        ledger
            .live()
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["b", "c"]
    );
    ledger
        .replace(vec![
            part("a", 1, true),
            part("b", 2, true),
            part("c", 1, true),
        ])
        .unwrap();
    let next = ledger.ready().unwrap();
    assert_eq!(next.lines, ["b revision 2", "c revision 1"]);
    assert!(ledger.acknowledge(&next));
    ledger
        .replace(vec![
            part("a", 1, true),
            part("b", 3, true),
            part("c", 1, true),
        ])
        .unwrap();
    let correction = ledger.ready().unwrap();
    assert_eq!(correction.lines, ["↳ card b → b revision 3"]);
    assert!(ledger.acknowledge(&correction));
    assert!(ledger.ready().is_none());
}
#[test]
fn pending_versions_are_frozen_and_receipts_are_exact_and_single_use() {
    let mut ledger = PrintLedger::new();
    ledger.replace(vec![part("a", 1, true)]).unwrap();
    let first = ledger.ready().unwrap();
    ledger.replace(vec![part("a", 2, true)]).unwrap();
    let mut altered = first.clone();
    altered.lines[0] = "wrong output".into();
    assert!(!ledger.acknowledge(&altered));
    let mut foreign = PrintLedger::new();
    foreign.replace(vec![part("a", 1, true)]).unwrap();
    assert!(!ledger.acknowledge(&foreign.ready().unwrap()));
    assert!(ledger.acknowledge(&first));
    assert!(!ledger.acknowledge(&first));
    assert_eq!(ledger.ready().unwrap().lines, ["↳ card a → a revision 2"]);
}
#[test]
fn invalid_replacement_is_atomic_and_cannot_forget_an_outstanding_revision() {
    let mut ledger = PrintLedger::new();
    ledger.replace(vec![part("a", 2, true)]).unwrap();
    let batch = ledger.ready().unwrap();
    ledger.replace(vec![]).unwrap();
    assert_eq!(
        ledger.replace(vec![part("a", 1, true)]),
        Err(Invalid::Revision)
    );
    assert_eq!(
        ledger.replace(vec![part("b", 1, true), part("b", 1, true)]),
        Err(Invalid::Identity)
    );
    let mut big = part("b", 1, true);
    big.text = "x".repeat(61 * 1024);
    assert_eq!(ledger.replace(vec![big]), Err(Invalid::Limit));
    assert!(ledger.acknowledge(&batch));
    assert_eq!(ledger.live(), [] as [&Part; 0]);
    assert_eq!(
        ledger.replace(vec![part("a", 1, true)]),
        Err(Invalid::Revision)
    );
}
#[test]
fn ambiguous_partial_write_stops_automatic_replay() {
    let mut ledger = PrintLedger::new();
    ledger.replace(vec![part("a", 1, true)]).unwrap();
    let batch = ledger.ready().unwrap();
    let mut altered = batch.clone();
    altered.lines.clear();
    assert!(!ledger.abort(&altered));
    assert!(ledger.abort(&batch));
    assert!(ledger.stopped());
    assert!(!ledger.acknowledge(&batch));
    ledger.replace(vec![part("a", 2, true)]).unwrap();
    assert!(ledger.ready().is_none());
}

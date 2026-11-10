//! Exit capture charges current, successful and failed directory alternatives.
use super::*;

#[test]
fn control_capture_charges_long_paths_in_every_directory_endpoint() {
    let mut state = State {
        cwd_paths: (0..8)
            .map(|number| super::super::cwd::Directory {
                previous: Some(format!("/previous/{number}/{}", "z".repeat(4000))),
                physical: format!("/{number}/{}", "x".repeat(4000)),
                logical: Some(format!("/alias/{number}/{}", "y".repeat(4000))),
                derived: true,
            })
            .collect(),
        ..State::default()
    };
    state.command_outcomes();
    let payload: usize = state
        .cwd_paths
        .iter()
        .map(|directory| {
            directory.physical.len()
                + directory.logical.as_ref().unwrap().len()
                + directory.previous.as_ref().unwrap().len()
        })
        .sum();
    let mut frame = Frame::new(Kind::Function);
    let mut bytes = 0;
    capture(&mut frame, &state, &mut bytes, false).unwrap();
    assert!(
        bytes >= payload * 3,
        "each endpoint owns its cloned path strings"
    );
    assert_eq!(frame.exits.len(), 1);
    let mut frame = Frame::new(Kind::Function);
    let mut bytes = super::super::budget::MAX_EVIDENCE_BYTES - payload * 5 / 2;
    assert!(matches!(
        capture(&mut frame, &state, &mut bytes, false),
        Err(Error::Limit)
    ));
    assert!(
        frame.exits.is_empty(),
        "reject before retaining another state clone"
    );
}

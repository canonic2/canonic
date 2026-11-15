//! Application pagination behavior, including exhaustive bounded codec properties.
#![allow(clippy::unwrap_used)]
use super::*;

#[test]
fn cursor_round_trip_and_tamper_properties() {
    let scope = Scope::new("records", digest(b"filter"));
    // Every supported key length, all byte values, and every encoded byte mutation.
    for length in 0..=110 {
        let key = Position::Keyset((0..length).map(|n| u8::try_from(n).unwrap()).collect());
        let cursor = scope.encode(&key).unwrap();
        assert!(cursor.len() <= MAX_CURSOR_BYTES);
        assert_eq!(scope.decode(&cursor).unwrap(), key);
        for index in 0..cursor.len() {
            let mut changed = cursor.as_bytes().to_vec();
            changed[index] = if changed[index] == b'A' { b'B' } else { b'A' };
            assert_eq!(
                scope.decode(std::str::from_utf8(&changed).unwrap()),
                Err(Error::InvalidCursor)
            );
        }
    }
    for byte in 0..=255 {
        let key = Position::Keyset(vec![byte; 110]);
        assert_eq!(scope.decode(&scope.encode(&key).unwrap()).unwrap(), key);
    }
    assert!(scope.encode(&Position::Keyset(vec![0; 111])).is_err());
    let offsets = Scope::offset("log", digest(b"query"), digest(b"revision"));
    for offset in [0, 1, 255, u64::MAX] {
        let position = Position::Offset(offset);
        assert_eq!(
            offsets.decode(&offsets.encode(&position).unwrap()).unwrap(),
            position
        );
    }
}

#[test]
fn cursor_refuses_foreign_query_revision_version_and_malformed_input() {
    let scope = Scope::offset("log", digest(b"query"), digest(b"revision"));
    let cursor = scope.encode(&Position::Offset(7)).unwrap();
    for foreign in [
        Scope::offset("other", digest(b"query"), digest(b"revision")),
        Scope::offset("log", digest(b"other"), digest(b"revision")),
        Scope::offset("log", digest(b"query"), digest(b"other")),
        Scope::new("log", digest(b"query")),
    ] {
        assert_eq!(foreign.decode(&cursor), Err(Error::InvalidCursor));
    }
    for text in ["", "A", "{}", "bad=", &"a".repeat(257)] {
        assert_eq!(scope.decode(text), Err(Error::InvalidCursor));
    }
    let mut bytes = cursor::decode(&cursor).unwrap();
    bytes[0] += 1;
    let end = bytes.len() - 16;
    let checksum = digest(&bytes[..end]);
    bytes[end..].copy_from_slice(&checksum[..16]);
    let old = crate::content::base64(&bytes)
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_");
    assert_eq!(scope.decode(&old), Err(Error::InvalidCursor));
}

#[test]
fn limit_admission_and_schema_match() {
    let limits = Limits::STANDARD;
    assert_eq!(
        Request::from_mcp(&json!({}), limits).unwrap().limit.get(),
        50
    );
    assert_eq!(
        Request::from_cli(None, Some(1), limits)
            .unwrap()
            .limit
            .get(),
        1
    );
    for value in [
        json!(0),
        json!(101),
        json!(-1),
        json!(1.5),
        json!(null),
        json!("1"),
    ] {
        assert!(matches!(
            Request::from_mcp(&json!({"limit":value}), limits),
            Err(Error::InvalidArguments(_))
        ));
    }
    assert!(matches!(
        Request::from_mcp(&json!({"cursor":null}), limits),
        Err(Error::InvalidCursor)
    ));
    assert!(
        Limits {
            default: 51,
            ..limits
        }
        .validate()
        .is_err()
    );
    assert!(Limits { max: 101, ..limits }.validate().is_err());
    assert!(
        Limits {
            default: 0,
            ..limits
        }
        .validate()
        .is_err()
    );
    assert_eq!(limits.properties()["limit"]["maximum"], 100);
    let lines = Limits {
        default: 100,
        max: 400,
        ..limits
    };
    assert!(matches!(
        Request::from_mcp(&json!({}), lines),
        Err(Error::InvalidArguments(_))
    ));
    assert!(matches!(
        Request::from_cli(None, None, lines),
        Err(Error::InvalidArguments(_))
    ));
    assert!(lines.limit(Some(100)).is_err());
    assert_eq!(lines.unit_limit(Some(400)).unwrap().get(), 400);
    assert_eq!(
        Request::from_mcp_units(&json!({}), lines)
            .unwrap()
            .limit
            .get(),
        100
    );
    assert_eq!(
        Request::from_cli_units(None, Some(400), lines)
            .unwrap()
            .limit
            .get(),
        400
    );
    assert!(Request::from_mcp_units(&json!({"limit":401}), lines).is_err());
    assert_eq!(
        output_properties(Casing::Camel)["nextCursor"]["type"],
        json!(["string", "null"])
    );
}

#[test]
fn builder_stops_at_count_budget_and_transport_and_renders_camel_case() {
    let scope = Scope::new("records", digest(b""));
    let limits = Limits {
        default: 2,
        max: 3,
        max_bytes: 12,
    };
    let key = Position::Keyset(b"a".to_vec());
    let mut builder = Builder::new(
        &scope,
        limits.limit(None).unwrap(),
        limits,
        100,
        "record_read",
    )
    .unwrap();
    assert!(builder.push(json!("é"), &key).unwrap());
    assert!(!builder.push(json!("\\\nlong"), &key).unwrap());
    let page = builder.finish(true).render(Casing::Camel);
    assert_eq!(page["items"], json!(["é"]));
    assert!(page["nextCursor"].is_string());
    assert!(page.get("next_cursor").is_none());
    let mut builder = Builder::new(
        &scope,
        limits.limit(Some(1)).unwrap(),
        limits,
        100,
        "record_read",
    )
    .unwrap();
    assert!(builder.push(json!("a".repeat(50)), &key).unwrap());
    assert!(builder.is_full());
    assert!(!builder.push(json!(0), &key).unwrap());
    assert_eq!(builder.finish(true).items.len(), 1);
    let mut builder = Builder::new(
        &scope,
        limits.limit(None).unwrap(),
        limits,
        12,
        "record_read",
    )
    .unwrap();
    assert!(
        matches!(builder.push(json!("a".repeat(50)), &key), Err(Error::ItemTooLarge { read_tool }) if read_tool == "record_read")
    );
    let builder = Builder::new(
        &scope,
        limits.limit(None).unwrap(),
        limits,
        100,
        "record_read",
    )
    .unwrap();
    assert!(builder.finish(false).next_cursor.is_none());
}

#[test]
fn keyset_pages_survive_insertions_and_deletions() {
    let scope = Scope::new("records", digest(b""));
    let limits = Limits::STANDARD;
    let mut rows = vec![1u8, 3, 5, 7];
    let mut builder = Builder::new(
        &scope,
        limits.limit(Some(2)).unwrap(),
        limits,
        1024 * 1024,
        "record_read",
    )
    .unwrap();
    for row in &rows[..2] {
        builder
            .push(json!(row), &Position::Keyset(vec![*row]))
            .unwrap();
    }
    let first = builder.finish(true);
    assert_eq!(first.items, vec![json!(1), json!(3)]);
    let Position::Keyset(key) = scope.decode(first.next_cursor.as_deref().unwrap()).unwrap() else {
        panic!()
    };
    rows.remove(0);
    rows.extend([2, 4]);
    rows.sort_unstable();
    let remaining: Vec<_> = rows.into_iter().filter(|row| *row > key[0]).collect();
    assert_eq!(remaining, [4, 5, 7]);
}

#[test]
fn pages_follow_count_and_byte_stops_to_known_exhaustion() {
    let scope = Scope::new("records", digest(b"all"));
    for count in [1, 2, 17, 50, 100] {
        for budget in [2, 32, 256 * 1024] {
            let limits = Limits {
                max_bytes: budget,
                ..Limits::STANDARD
            };
            let mut next = 0_u64;
            let mut actual = Vec::new();
            loop {
                let mut builder = Builder::new(
                    &scope,
                    limits.limit(Some(count)).unwrap(),
                    limits,
                    1024 * 1024,
                    "record_read",
                )
                .unwrap();
                while next < 237 {
                    if !builder
                        .push(json!(next), &Position::Keyset(next.to_be_bytes().to_vec()))
                        .unwrap()
                    {
                        break;
                    }
                    next += 1;
                    if builder.is_full() {
                        break;
                    }
                }
                let page = builder.finish(next < 237);
                assert!(page.items.len() as u64 <= count);
                actual.extend(page.items);
                let Some(cursor) = page.next_cursor else {
                    break;
                };
                assert_eq!(
                    scope.decode(&cursor).unwrap(),
                    Position::Keyset((next - 1).to_be_bytes().to_vec())
                );
            }
            assert_eq!(actual, (0..237).map(|n| json!(n)).collect::<Vec<_>>());
        }
    }
}

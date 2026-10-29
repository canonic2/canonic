//! Capped, segmented optional observations, locked independently of durable authority.
//!
//! One OS file lock serializes append, segment rotation and controls. Callbacks
//! append to the active segment and never read earlier records; rotation counts
//! the decisions of the one segment it seals. Readers capture segment handles and
//! the active boundary under the lock, then verify, correlate and retain outside it.
mod correlation;
use super::{Error, State, Store, Temporary, private, sync};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    borrow::Cow,
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    ops::ControlFlow,
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
/// Physical cap on every segment of one generation.
const STORE_LIMIT: usize = 64 * 1024 * 1024;
/// An append that would grow the active segment past this seals it first.
const SEGMENT_LIMIT: usize = 1024 * 1024;
/// Bound on listed segments; the byte cap keeps far fewer.
const SEGMENT_COUNT: usize = 128;
/// Bound on records a retention or clear control rewrites.
const SNAPSHOT_LIMIT: usize = 30 * 1024 * 1024;
const TAIL_BUDGET: usize = 2 * 1024 * 1024;
const LINE_LIMIT: usize = 12 * 1024;
const EVENT_LIMIT: usize = 65_536;
const FORMAT: &str = "shield-history-v2";
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Header {
    format: String,
    project_id: String,
    generation: String,
    segment: u64,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Record {
    event: Value,
    digest: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BorrowedRecord<'a> {
    #[serde(borrow)]
    event: &'a serde_json::value::RawValue,
    digest: &'a str,
}
#[derive(Deserialize)]
struct Fields<'a> {
    #[serde(borrow)]
    kind: Cow<'a, str>,
    #[serde(borrow)]
    links: Links<'a>,
}
#[derive(Deserialize)]
struct Links<'a> {
    #[serde(borrow)]
    request_id: Option<Cow<'a, str>>,
}
/// One segment file: the active one has no decision count until it is sealed.
struct Segment {
    sequence: u64,
    decisions: Option<u64>,
    path: PathBuf,
}
/// Open handles and lengths captured under the lock. Sealed segments never
/// change, and an evicted segment stays readable through its handle.
pub(in crate::review) struct View {
    segments: Vec<(u64, File, u64)>,
    complete: bool,
}

pub(in crate::review) fn lock(store: &Store) -> Result<File, Error> {
    let path = store.directory.join("history.lock");
    if path.exists() || fs::symlink_metadata(&path).is_ok() {
        regular(&path)?;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    private(&mut options);
    let file = options.open(&path)?;
    regular(&path)?;
    for _ in 0..500 {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) => thread::sleep(Duration::from_millis(1)),
            Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
        }
    }
    Err(Error::Locked)
}
fn regular(path: &Path) -> Result<(), Error> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || m.file_type().is_symlink() {
        return Err(Error::Invalid("history must be a regular file"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let parent = fs::metadata(path.parent().ok_or(Error::Invalid("history parent"))?)?;
        if m.mode() & 0o077 != 0 || m.uid() != parent.uid() {
            return Err(Error::Invalid("history must be private"));
        }
    }
    Ok(())
}
pub(in crate::review) fn identifier(kind: &str, project_id: &str) -> Result<String, Error> {
    use ring::rand::{SecureRandom, SystemRandom};
    let mut nonce = [0_u8; 16];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| Error::Invalid("history ID entropy unavailable"))?;
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Invalid("clock before epoch"))?
        .as_nanos();
    let suffix: String = nonce
        .iter()
        .flat_map(|b| {
            let hex = b"0123456789abcdef";
            [
                char::from(hex[usize::from(b >> 4)]),
                char::from(hex[usize::from(b & 15)]),
            ]
        })
        .collect();
    Ok(format!("{kind}-{}-{time:032x}-{suffix}", &project_id[..16]))
}
pub(in crate::review) fn generation() -> Result<String, Error> {
    Ok(format!(
        "journal-{}",
        crate::policy::fingerprint(identifier("generation", &"0".repeat(64))?.as_bytes())
    ))
}
fn digest(state: &State) -> Result<String, Error> {
    let digest = state
        .history_generation
        .strip_prefix("journal-")
        .ok_or(Error::Invalid("history generation invalid"))?;
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Invalid("history generation invalid"));
    }
    Ok(digest.into())
}
fn segment_path(store: &Store, digest: &str, sequence: u64, decisions: Option<u64>) -> PathBuf {
    store.directory.join(match decisions {
        None => format!("history-{digest}.{sequence:010}.jsonl"),
        Some(count) => format!("history-{digest}.{sequence:010}.{count}.jsonl"),
    })
}
/// The generation's preserved failure record, shared by all of its segments.
fn error_path(store: &Store, state: &State) -> Result<PathBuf, Error> {
    Ok(store
        .directory
        .join(format!("history-{}.error", digest(state)?)))
}
fn number(text: &str, width: Option<usize>) -> Result<u64, Error> {
    let shaped = width.map_or(!text.is_empty() && text.len() <= 6, |w| text.len() == w);
    if !shaped || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::Invalid("history segment name"));
    }
    text.parse()
        .map_err(|_| Error::Invalid("history segment name"))
}
/// This generation's segments in sequence order; only the newest may be active.
fn segments(store: &Store, state: &State) -> Result<Vec<Segment>, Error> {
    let prefix = format!("history-{}.", digest(state)?);
    let mut found = Vec::new();
    for entry in fs::read_dir(&store.directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(rest) = name
            .to_str()
            .and_then(|name| name.strip_prefix(&prefix))
            .and_then(|name| name.strip_suffix(".jsonl"))
        else {
            continue;
        };
        let (sequence, decisions) = rest
            .split_once('.')
            .map_or((rest, None), |(s, d)| (s, Some(d)));
        let segment = Segment {
            sequence: number(sequence, Some(10))?,
            decisions: decisions.map(|d| number(d, None)).transpose()?,
            path: entry.path(),
        };
        regular(&segment.path)?;
        found.push(segment);
        if found.len() > SEGMENT_COUNT {
            return Err(Error::Invalid("history segment limit"));
        }
    }
    found.sort_by_key(|s| s.sequence);
    if found.windows(2).any(|w| w[0].sequence == w[1].sequence)
        || found.iter().rev().skip(1).any(|s| s.decisions.is_none())
    {
        return Err(Error::Invalid("inconsistent history segments"));
    }
    Ok(found)
}
/// Captures the current generation's bounded view for reading outside the lock.
pub(in crate::review) fn capture(store: &Store, state: &State) -> Result<View, Error> {
    let list = segments(store, state)?;
    let mut segments = Vec::with_capacity(list.len());
    let mut bytes = 0_u64;
    for segment in &list {
        let file = File::open(&segment.path)?;
        let length = file.metadata()?.len();
        bytes += length;
        segments.push((segment.sequence, file, length));
    }
    if bytes > STORE_LIMIT as u64 {
        return Err(Error::Invalid("history byte limit"));
    }
    let tail = match segments.last_mut() {
        None => true,
        Some((_, _, 0)) => false,
        Some((_, file, length)) => {
            file.seek(SeekFrom::Start(*length - 1))?;
            let mut final_byte = [0_u8; 1];
            file.read_exact(&mut final_byte)?;
            file.seek(SeekFrom::Start(0))?;
            final_byte[0] == b'\n'
        }
    };
    Ok(View {
        segments,
        complete: tail && !error_path(store, state)?.exists(),
    })
}
/// Reads a captured view into the reader's correlated, retained events.
pub(in crate::review) fn finish(view: Result<View, Error>, state: &mut State) {
    let read = view.and_then(|view| {
        let complete = view.complete;
        read(view, state).map(|events| (events, complete))
    });
    if let Ok(((events, evicted), complete)) = read {
        state.events = correlation::view(events, state.history_limit);
        state.trim();
        state.history_available = complete;
        state.history_evicted = evicted && distinct_decisions(&state.events) < state.history_limit;
    } else {
        state.events.clear();
        state.history_available = false;
        state.history_evicted = false;
    }
}
fn distinct_decisions(events: &[Value]) -> usize {
    events
        .iter()
        .filter(|e| e["kind"] == "decision")
        .filter_map(|e| e["links"]["request_id"].as_str())
        .collect::<BTreeSet<_>>()
        .len()
}
/// Verified raw events oldest-first, and whether older segments or events are
/// no longer present. Newest segments are read first so the event bound drops
/// only the oldest records.
fn read(view: View, state: &State) -> Result<(Vec<Value>, bool), Error> {
    let mut evicted = view.segments.first().is_some_and(|(s, _, _)| *s > 0);
    let last = view.segments.len().saturating_sub(1);
    let mut parts = Vec::new();
    let mut count = 0;
    for (index, (sequence, file, length)) in view.segments.into_iter().enumerate().rev() {
        if count >= EVENT_LIMIT {
            evicted = true;
            break;
        }
        let mut reader = BufReader::new(file.take(length));
        header(&mut reader, state, sequence)?;
        let mut events = Vec::new();
        let mut line = Vec::new();
        loop {
            line.clear();
            (&mut reader)
                .take((LINE_LIMIT + 1) as u64)
                .read_until(b'\n', &mut line)?;
            if line.is_empty() {
                break;
            }
            if line.last() != Some(&b'\n') {
                // Only the active segment may end in a torn append.
                if line.len() > LINE_LIMIT || index != last {
                    return Err(Error::Invalid("corrupt history segment"));
                }
                break;
            }
            events.push(decode(&line)?);
        }
        count += events.len();
        parts.push(events);
    }
    let mut events: Vec<_> = parts.into_iter().rev().flatten().collect();
    if events.len() > EVENT_LIMIT {
        events.drain(..events.len() - EVENT_LIMIT);
        evicted = true;
    }
    Ok((events, evicted))
}
/// Visits verified events newest-first for advisory hook replies, until `visit`
/// breaks or the retained window ends. Reads at most the newest `TAIL_BUDGET`
/// bytes across segments, so a reply never pays for the whole retained history.
pub(in crate::review) fn recent(
    store: &Store,
    state: &State,
    mut visit: impl FnMut(&Value) -> ControlFlow<()>,
) -> Result<bool, Error> {
    let view = {
        let _guard = lock(store)?;
        capture(store, state)?
    };
    let complete = view
        .segments
        .first()
        .is_none_or(|(sequence, _, _)| *sequence == 0);
    let mut budget = TAIL_BUDGET as u64;
    let mut requests = BTreeSet::new();
    for (sequence, file, length) in view.segments.into_iter().rev() {
        let mut reader = BufReader::new(file);
        let body = header(&mut reader, state, sequence)? as u64;
        let start = body.max(length.saturating_sub(budget));
        let mut file = reader.into_inner();
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        file.take(length.saturating_sub(start))
            .read_to_end(&mut bytes)?;
        budget -= bytes.len() as u64;
        // A torn final append and, when the window starts mid-record, its first
        // partial line are not records.
        let end = bytes
            .iter()
            .rposition(|b| *b == b'\n')
            .map_or(0, |at| at + 1);
        let begin = if start > body {
            bytes[..end]
                .iter()
                .position(|b| *b == b'\n')
                .map_or(end, |at| at + 1)
        } else {
            0
        };
        for line in bytes[begin..end].split(|b| *b == b'\n').rev() {
            if line.is_empty() {
                continue;
            }
            let event = decode(line)?;
            if event["kind"] == "decision"
                && let Some(request) = event["links"]["request_id"].as_str()
            {
                requests.insert(request.to_owned());
                if requests.len() > state.history_limit {
                    return Ok(false);
                }
            }
            if visit(&event).is_break() {
                return Ok(true);
            }
        }
        if start > body || budget == 0 {
            return Ok(false);
        }
    }
    Ok(complete)
}
/// Checks one segment header and returns its byte length.
fn header(reader: &mut impl BufRead, state: &State, segment: u64) -> Result<usize, Error> {
    let mut line = Vec::new();
    reader.take(1025).read_until(b'\n', &mut line)?;
    if line.last() != Some(&b'\n') || line.len() > 1024 {
        return Err(Error::Invalid("corrupt history header"));
    }
    let h: Header =
        serde_json::from_slice(&line).map_err(|_| Error::Invalid("corrupt history header"))?;
    if h.format != FORMAT
        || h.project_id != state.project_id
        || h.generation != state.history_generation
        || h.segment != segment
    {
        return Err(Error::Invalid("incompatible history header"));
    }
    Ok(line.len())
}
fn decode(line: &[u8]) -> Result<Value, Error> {
    serde_json::from_str(checked_event(line)?.get())
        .map_err(|_| Error::Invalid("corrupt history record"))
}
fn checked_event(line: &[u8]) -> Result<&serde_json::value::RawValue, Error> {
    if line.len() > LINE_LIMIT {
        return Err(Error::Invalid("history line limit"));
    }
    let r: BorrowedRecord<'_> =
        serde_json::from_slice(line).map_err(|_| Error::Invalid("corrupt history record"))?;
    if r.digest != blake3::hash(r.event.get().as_bytes()).to_hex().as_str() {
        // Preserve the original semantic checksum for manually reformatted rows.
        let event: Value = serde_json::from_str(r.event.get())
            .map_err(|_| Error::Invalid("corrupt history record"))?;
        let bytes = serde_json::to_vec(&event).map_err(|_| Error::Invalid("history encoding"))?;
        if r.digest != blake3::hash(&bytes).to_hex().as_str() {
            return Err(Error::Invalid("history record checksum differs"));
        }
    }
    Ok(r.event)
}
fn line(event: &Value) -> Result<Vec<u8>, Error> {
    let bytes = serde_json::to_vec(event).map_err(|_| Error::Invalid("history encoding"))?;
    let r = Record {
        event: event.clone(),
        digest: blake3::hash(&bytes).to_hex().to_string(),
    };
    let mut bytes = serde_json::to_vec(&r).map_err(|_| Error::Invalid("history encoding"))?;
    bytes.push(b'\n');
    if bytes.len() > LINE_LIMIT {
        return Err(Error::Invalid("history line limit"));
    }
    Ok(bytes)
}
/// Distinct decision request IDs among complete record lines. Unreadable lines
/// count as none, which only retains more; checksums remain reader work.
fn count_decisions(bytes: &[u8]) -> u64 {
    let mut requests = BTreeSet::new();
    for line in bytes.split(|b| *b == b'\n') {
        let Ok(record) = serde_json::from_slice::<BorrowedRecord<'_>>(line) else {
            continue;
        };
        if let Ok(fields) = serde_json::from_str::<Fields<'_>>(record.event.get())
            && fields.kind == "decision"
            && let Some(request) = fields.links.request_id
        {
            requests.insert(request.into_owned());
        }
    }
    requests.len() as u64
}
/// One encoded record line and its decision request, if it is a decision.
type Row = (Vec<u8>, Option<String>);
/// Retained rows for a control rewrite, evicting whole oldest commands past the
/// snapshot bound.
fn retained(events: &[Value]) -> Result<Vec<Row>, Error> {
    let mut rows: Vec<_> = events.iter().map(line).collect::<Result<_, _>>()?;
    let mut bytes: usize = rows.iter().map(Vec::len).sum();
    while bytes > SNAPSHOT_LIMIT {
        let Some(id) = events
            .iter()
            .zip(&rows)
            .find(|(e, r)| !r.is_empty() && e["kind"] == "decision")
            .map(|(e, _)| e["links"]["request_id"].clone())
        else {
            return Err(Error::Invalid("history snapshot limit"));
        };
        for (event, row) in events.iter().zip(&mut rows) {
            if event["links"]["request_id"] == id {
                bytes -= row.len();
                row.clear();
            }
        }
    }
    Ok(events
        .iter()
        .zip(rows)
        .filter(|(_, row)| !row.is_empty())
        .map(|(event, row)| {
            let request = (event["kind"] == "decision")
                .then(|| event["links"]["request_id"].as_str().map(str::to_owned))
                .flatten();
            (row, request)
        })
        .collect())
}
/// Publishes one synchronized segment file that must not already exist.
fn write_segment(
    store: &Store,
    state: &State,
    sequence: u64,
    decisions: Option<u64>,
    records: &[u8],
) -> Result<PathBuf, Error> {
    let h = Header {
        format: FORMAT.into(),
        project_id: state.project_id.clone(),
        generation: state.history_generation.clone(),
        segment: sequence,
    };
    let tmp = Temporary {
        path: store.directory.join(format!(
            ".history-{}.tmp",
            identifier("file", &state.project_id)?
        )),
    };
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    private(&mut options);
    let mut file = options.open(&tmp.path)?;
    serde_json::to_writer(&mut file, &h).map_err(|_| Error::Invalid("history header encoding"))?;
    file.write_all(b"\n")?;
    file.write_all(records)?;
    sync(&file)?;
    drop(file);
    let target = segment_path(store, &digest(state)?, sequence, decisions);
    if fs::symlink_metadata(&target).is_ok() {
        return Err(Error::Invalid("history segment exists"));
    }
    fs::rename(&tmp.path, &target)?;
    #[cfg(unix)]
    sync(&File::open(&store.directory)?)?;
    Ok(target)
}
/// Control rewrite: replaces this generation's segments with `events`, sealed
/// in segment-sized chunks with the newest chunk active. Numbering starts at 1
/// when the source was already evicted or the snapshot bound drops commands, so
/// readers keep reporting the eviction.
fn replace(store: &Store, state: &State, events: &[Value], evicted: bool) -> Result<(), Error> {
    let rows = retained(events)?;
    for old in segments(store, state)? {
        fs::remove_file(old.path)?;
    }
    let first = u64::from(evicted || rows.len() < events.len());
    let (mut sequence, mut chunk, mut requests) = (first, Vec::new(), BTreeSet::new());
    for (row, request) in rows {
        if !chunk.is_empty() && chunk.len() + row.len() > SEGMENT_LIMIT {
            write_segment(store, state, sequence, Some(requests.len() as u64), &chunk)?;
            (sequence, chunk, requests) = (sequence + 1, Vec::new(), BTreeSet::new());
        }
        chunk.extend(row);
        requests.extend(request);
    }
    write_segment(store, state, sequence, None, &chunk)?;
    Ok(())
}
pub(in crate::review) fn encode(
    store: &Store,
    state: &State,
    locked: bool,
) -> Result<Vec<u8>, Error> {
    let mut value = json!(state);
    super::sources::checkpoint(store, &mut value, false)?;
    value["events"] = json!([]);
    if let Some(purge) = state.history_rewrite {
        let (mut events, mut evicted) = (Vec::new(), false);
        if !purge {
            let current = store.read_checkpoint()?;
            let view = capture(store, &current)?;
            if !view.complete {
                return Err(Error::Invalid(
                    "preserve corrupt history before changing retention; clear to reset",
                ));
            }
            let raw;
            (raw, evicted) = read(view, &current).map_err(|_| {
                Error::Invalid("preserve corrupt history before changing retention; clear to reset")
            })?;
            // Persist raw records; links stay reader-derived. Segments can hold
            // records the previous limit already expired, so apply that window
            // first: raising the limit never restores them.
            let previous = correlation::retain_raw(raw, current.history_limit);
            events = correlation::retain_raw(previous, state.history_limit);
        }
        replace(store, state, &events, evicted)?;
    } else if locked
        && segments(store, state).is_ok_and(|s| s.is_empty())
        && replace(store, state, &[], false).is_err()
    {
        mark_failure(store, state);
    }
    serde_json::to_vec(&value).map_err(|_| Error::Invalid("review encoding"))
}
/// Appends pending observations under the caller's history lock. Work is
/// bounded by the active segment: tail recovery, and on rotation one count of
/// the sealed segment's decisions and a listing-based prune.
pub(in crate::review) fn publish(store: &Store, state: &State) -> Result<(), Error> {
    if !state.history_enabled || state.pending_history.is_empty() {
        return Ok(());
    }
    let records = state
        .pending_history
        .iter()
        .map(line)
        .collect::<Result<Vec<_>, _>>()?
        .concat();
    if records.len() > SEGMENT_LIMIT {
        return Err(Error::Invalid("history append limit"));
    }
    let mut list = segments(store, state)?;
    if list.last().is_none_or(|s| s.decisions.is_some()) {
        let sequence = list.last().map_or(0, |s| s.sequence + 1);
        let path = write_segment(store, state, sequence, None, &[])?;
        if list.is_empty() {
            cleanup(store, state);
        }
        list.push(Segment {
            sequence,
            decisions: None,
            path,
        });
    }
    let mut file = open_active(&list, state)?;
    recover_tail(&error_path(store, state)?, &mut file.0)?;
    let length = file.0.metadata()?.len();
    if length > file.1 && length + records.len() as u64 > SEGMENT_LIMIT as u64 {
        rotate(store, state, &mut list, &mut file.0, file.1)?;
        file = open_active(&list, state)?;
    }
    file.0.seek(SeekFrom::End(0))?;
    file.0.write_all(&records)?;
    Ok(())
}
/// The active segment opened for append, with its checked header length.
fn open_active(list: &[Segment], state: &State) -> Result<(File, u64), Error> {
    let active = list
        .last()
        .filter(|s| s.decisions.is_none())
        .ok_or(Error::Invalid("history active segment"))?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&active.path)?;
    let body = header(&mut BufReader::new(&mut file), state, active.sequence)? as u64;
    Ok((file, body))
}
/// Seals the full active segment with its decision count, prunes older sealed
/// segments by retention and the byte cap, and starts the next active segment.
fn rotate(
    store: &Store,
    state: &State,
    list: &mut Vec<Segment>,
    file: &mut File,
    body: u64,
) -> Result<(), Error> {
    let active = list.pop().ok_or(Error::Invalid("history active segment"))?;
    file.seek(SeekFrom::Start(body))?;
    let mut bytes = Vec::new();
    file.take(SEGMENT_LIMIT as u64).read_to_end(&mut bytes)?;
    // A sealed segment reaches stable storage before it is renamed.
    sync(file)?;
    let decisions = count_decisions(&bytes);
    let sealed = segment_path(store, &digest(state)?, active.sequence, Some(decisions));
    if fs::symlink_metadata(&sealed).is_ok() {
        return Err(Error::Invalid("history segment exists"));
    }
    fs::rename(&active.path, &sealed)?;
    list.push(Segment {
        sequence: active.sequence,
        decisions: Some(decisions),
        path: sealed,
    });
    prune(list, state.history_limit)?;
    let sequence = active.sequence + 1;
    let path = write_segment(store, state, sequence, None, &[])?;
    list.push(Segment {
        sequence,
        decisions: None,
        path,
    });
    Ok(())
}
/// Removes sealed segments that hold no retained command, then the oldest
/// sealed segments while the generation would exceed its byte cap with a new
/// full active segment. Byte eviction can retain fewer requests than the limit.
fn prune(list: &mut Vec<Segment>, limit: usize) -> Result<(), Error> {
    let mut keep = vec![true; list.len()];
    let mut newer = 0_u64;
    for (index, segment) in list.iter().enumerate().rev() {
        let Some(decisions) = segment.decisions else {
            continue;
        };
        if newer >= limit as u64 {
            keep[index] = false;
        } else {
            newer += decisions;
        }
    }
    let sizes = list
        .iter()
        .map(|s| fs::metadata(&s.path).map(|m| m.len()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut total: u64 = sizes
        .iter()
        .zip(&keep)
        .filter(|(_, kept)| **kept)
        .map(|(size, _)| size)
        .sum();
    for (index, segment) in list.iter().enumerate() {
        if total + SEGMENT_LIMIT as u64 <= STORE_LIMIT as u64 {
            break;
        }
        if keep[index] && segment.decisions.is_some() {
            keep[index] = false;
            total -= sizes[index];
        }
    }
    let mut index = 0;
    let mut removed = Ok(());
    list.retain(|segment| {
        let kept = keep[index];
        index += 1;
        if !kept && removed.is_ok() {
            removed = fs::remove_file(&segment.path);
        }
        kept
    });
    removed.map_err(Error::from)
}
fn recover_tail(recovery: &Path, file: &mut File) -> Result<(), Error> {
    let length = file.metadata()?.len();
    let start = length.saturating_sub((LINE_LIMIT + 1) as u64);
    file.seek(SeekFrom::Start(start))?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail)?;
    if tail.last() == Some(&b'\n') {
        return Ok(());
    }
    let boundary = tail
        .iter()
        .rposition(|b| *b == b'\n')
        .ok_or(Error::Invalid("corrupt history tail"))?
        + 1;
    let fragment = &tail[boundary..];
    let mut options = OpenOptions::new();
    options.append(true);
    if recovery.exists() {
        regular(recovery)?;
        if fs::metadata(recovery)?.len() + fragment.len() as u64 > 1024 * 1024 {
            return Err(Error::Invalid(
                "preserved history failure budget exhausted; clear to reset",
            ));
        }
    } else {
        options.create_new(true);
    }
    private(&mut options);
    let mut preserved = options.open(recovery)?;
    preserved.write_all(fragment)?;
    sync(&preserved)?;
    file.set_len(start + boundary as u64)?;
    Ok(())
}
pub(in crate::review) fn mark_failure(store: &Store, state: &State) {
    if let Ok(path) = error_path(store, state) {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        private(&mut options);
        if let Ok(mut file) = options.open(path) {
            let _ = file.write_all(b"optional history publication failed\n");
        }
    }
}
/// Removes segments and failure records of every other generation, and segment
/// temporaries a crashed writer left. Callers hold the history lock, which every
/// segment writer also holds, so no temporary here is still being written.
pub(in crate::review) fn cleanup(store: &Store, state: &State) {
    let Ok(current) = digest(state) else {
        return;
    };
    let Ok(entries) = fs::read_dir(&store.directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        // Shield writes these names, so their extensions match exactly.
        let extension = Path::new(name).extension().and_then(|e| e.to_str());
        if name.starts_with(".history-") && extension == Some("tmp") {
            let _ = fs::remove_file(entry.path());
            continue;
        }
        let Some(digest) = name
            .strip_prefix("history-")
            .and_then(|s| s.split('.').next())
        else {
            continue;
        };
        if digest.len() == 64
            && digest.bytes().all(|b| b.is_ascii_hexdigit())
            && digest != current
            && matches!(extension, Some("jsonl" | "error"))
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}
pub(in crate::review) fn bound_event(mut event: Value) -> Value {
    if event.to_string().len() <= 8192 {
        return event;
    }
    // Per-command analysis duplicates the decisive explanation. Discard it
    // before reducing the evidence used by suggestions and history display.
    if let Some(evaluation) = event["details"]["evaluation"].as_object_mut()
        && evaluation.remove("analysis_coverage").is_some()
    {
        let mut missing = event["details"]["missing"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        missing.push(json!("shell_analysis"));
        event["details"]["missing"] = json!(missing);
        event["truncated"] = json!(true);
        if event.to_string().len() <= 8192 {
            return event;
        }
    }
    let d = &event["details"];
    let e = &d["evaluation"];
    let explanation = e
        .get("explanation")
        .or_else(|| e.get("evaluation"))
        .unwrap_or(e);
    let decision = e
        .get("decision")
        .or_else(|| explanation.get("decision"))
        .or_else(|| d.get("decision"))
        .cloned()
        .unwrap_or(Value::Null);
    let revision = e
        .get("revision")
        .or_else(|| e.get("policy_revision"))
        .or_else(|| d.get("policy_revision"))
        .cloned()
        .unwrap_or(Value::Null);
    let reason = e
        .get("reason")
        .or_else(|| explanation.get("reason"))
        .cloned()
        .unwrap_or(Value::Null);
    let matches: Vec<_> = explanation["matches"].as_array().into_iter().flatten().take(8).map(|m| {
            json!({"source":m["source"],"scope":m["scope"],"pack_id":m["pack_id"],"rule_id":m["rule_id"],"rule_ref":m["rule_ref"],"profile":m["profile"],"decision":m["decision"],"applied":m["applied"],"reason":m["reason"]})
    }).collect();
    // Keep whole guidance items: shortening a request could propose a rule for
    // different arguments. Bound both their count and total serialized bytes.
    let mut guidance = Vec::new();
    let mut bytes = 2;
    for item in explanation["guidance"].as_array().into_iter().flatten() {
        let length = item.to_string().len() + 1;
        if guidance.len() < 16 && bytes + length <= 4096 {
            guidance.push(item.clone());
            bytes += length;
        }
    }
    let omitted_guidance = guidance.len() < explanation["guidance"].as_array().map_or(0, Vec::len);
    event["details"]["evaluation"] = json!({"decision":decision,"revision":revision,"reason":reason,"matches":matches,"guidance":guidance});
    event["details"]["missing"] = json!(["full_evaluation"]);
    if event.to_string().len() > 8192 {
        event["details"]["request"] = Value::Null;
        event["details"]["missing"] = json!(["full_evaluation", "complete_request"]);
    }
    if event.to_string().len() > 8192 {
        for key in ["command", "operation"] {
            if event["details"][key].to_string().len() > 2048 {
                event["details"][key] = Value::Null;
            }
        }
        event["details"]["missing"] =
            json!(["full_evaluation", "complete_request", "complete_input"]);
    }
    if event.to_string().len() > 8192 {
        let d = &event["details"];
        event["details"] = json!({"evaluation":d["evaluation"],"decision":d["decision"],"policy_revision":d["policy_revision"],"disposition":d["disposition"],"ask_fallback":d["ask_fallback"],"host_decision":d["host_decision"],"status":d["status"],"exit_code":d["exit_code"],"missing":["detail_fields"]});
    }
    event["truncated"] = json!(true);
    if omitted_guidance && let Some(missing) = event["details"]["missing"].as_array_mut() {
        missing.push(json!("guidance"));
    }
    if event.to_string().len() > 8192 {
        // The final text reduction must never alter retained exact requests.
        event["details"]["evaluation"]["guidance"] = json!([]);
        // Attribution fields can themselves exceed the entry budget. Never silently
        // use their shortened form for exact correlation or reconstructing input.
        for key in ["provenance", "reasons", "details"] {
            shorten(&mut event[key]);
        }
        event["details"]["missing"] = json!(["detail_fields", "attribution_fields", "guidance"]);
    }
    event
}
fn shorten(value: &mut Value) {
    match value {
        Value::String(text) if text.len() > 256 => {
            let end = text
                .char_indices()
                .map(|(at, _)| at)
                .take_while(|at| *at <= 256)
                .last()
                .unwrap_or(0);
            text.truncate(end);
        }
        Value::Object(fields) => {
            for child in fields.values_mut() {
                shorten(child);
            }
        }
        Value::Array(rows) => {
            rows.truncate(8);
            for child in rows {
                shorten(child);
            }
        }
        _ => {}
    }
}
#[cfg(test)]
mod tests;

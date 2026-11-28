//! Bounded metadata history for owned page events and pre-action cursor waits.
use super::contract::Reference;
use super::session::Error;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::VecDeque;

const CAPACITY: usize = 512;
pub(crate) const MAX_CURSOR: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    PageCreated,
    PageClosed,
    PageCrashed,
    Dialog,
    FileChooser,
    Download,
    FrameNavigated,
    FrameAttached,
    FrameDetached,
    /// The page's main-frame `load` event.
    Load,
    /// The page's main-frame `DOMContentLoaded` event.
    DomContentLoaded,
    /// A dedicated worker created by the page.
    WorkerCreated,
    /// A service worker of the context (Chrome), recorded without a page.
    ServiceWorkerCreated,
}
/// Every event name, in schema and discovery order.
pub(crate) const EVENT_NAMES: [&str; 13] = [
    "page_created",
    "page_closed",
    "page_crashed",
    "dialog",
    "file_chooser",
    "download",
    "frame_navigated",
    "frame_attached",
    "frame_detached",
    "load",
    "dom_content_loaded",
    "worker_created",
    "service_worker_created",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Condition {
    pub event: Event,
    #[serde(default)]
    pub page: Option<Reference>,
}
impl Condition {
    pub(crate) fn matches(&self, record: &Value) -> bool {
        record["event"] == json!(self.event)
            && self.page.is_none_or(|page| record["page"] == json!(page))
    }
}

#[derive(Default)]
pub(crate) struct History {
    records: VecDeque<Value>,
    cursor: u64,
    dropped_through: u64,
}
impl History {
    pub(crate) fn page(
        &self,
        context: Reference,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<Value, &'static str> {
        use canonic_mcp_kit::page::{Builder, Limits, Position, Request};
        let request = Request::from_cli(cursor, Some(limit as u64), Limits::STANDARD)
            .map_err(|error| error.code())?;
        let scope = crate::pagination::scope("browser_send.events_read", &context)
            .map_err(|_| "invalid_arguments")?;
        let after = match request
            .cursor
            .as_deref()
            .map(|cursor| scope.decode(cursor))
            .transpose()
            .map_err(|_| "invalid_cursor")?
        {
            Some(Position::Keyset(key)) if key.len() == 8 => {
                u64::from_be_bytes(key.try_into().map_err(|_| "invalid_cursor")?)
            }
            None => self.dropped_through,
            _ => return Err("invalid_cursor"),
        };
        if after < self.dropped_through || after > self.cursor {
            return Err("invalid_cursor");
        }
        let mut builder = Builder::new(
            &scope,
            request.limit,
            Limits::STANDARD,
            crate::protocol::MAX_REPLY,
            "browser_send events_read with limit 1",
        )
        .map_err(|error| error.code())?;
        let mut rows = self.records.iter().filter(|record| {
            record["context"] == json!(context)
                && record["sequence"]
                    .as_u64()
                    .is_some_and(|sequence| sequence > after)
        });
        let mut ended = false;
        while !builder.is_full() {
            let Some(row) = rows.next() else {
                ended = true;
                break;
            };
            let sequence = row["sequence"].as_u64().expect("event sequence");
            if !builder
                .push(
                    row.clone(),
                    &Position::Keyset(sequence.to_be_bytes().to_vec()),
                )
                .map_err(|error| error.code())?
            {
                break;
            }
        }
        let page = builder.finish(!ended);
        let next=page.next_cursor.as_ref().map(|cursor|json!({"operation":"events_read","context":context,"cursor":cursor,"limit":limit}));
        Ok(
            json!({"context":context,"events":page.items,"nextCursor":page.next_cursor,"nextRequest":next,"latestSequence":self.cursor,"droppedThrough":self.dropped_through,"contentIncluded":false}),
        )
    }
    /// Scan only unconsumed witnesses. A terminal page removes only that page's
    /// alternatives; context-wide and other-page alternatives remain eligible.
    pub(crate) fn wait(
        &self,
        context: Reference,
        after: u64,
        mut cursor: u64,
        conditions: &[Condition],
    ) -> Result<Value, Error> {
        let result = (|| {
            let mut read = self.read(context, Some(cursor), 100)?;
            let mut ended = conditions
                .iter()
                .map(|condition| {
                    condition.page.and_then(|page| {
                        self.records.iter().find_map(|record| {
                            (record["context"] == json!(context)
                                && record["page"] == json!(page)
                                && record["sequence"]
                                    .as_u64()
                                    .is_some_and(|sequence| sequence <= cursor))
                            .then(|| terminal(record))
                            .flatten()
                        })
                    })
                })
                .collect::<Vec<_>>();
            loop {
                if let Some(code) = ended
                    .first()
                    .copied()
                    .flatten()
                    .filter(|_| ended.iter().all(Option::is_some))
                {
                    return Err(Error::from(code));
                }
                for event in read["events"].as_array().expect("Page events") {
                    if let Some(index) =
                        conditions
                            .iter()
                            .enumerate()
                            .position(|(index, condition)| {
                                ended[index].is_none() && condition.matches(event)
                            })
                    {
                        return Ok(
                            json!({"context":context,"matched":true,"conditionIndex":index,"event":event,"after":after,"nextCursor":event["sequence"],"contentIncluded":false}),
                        );
                    }
                    cursor = event["sequence"].as_u64().expect("Event cursor");
                    if let Some(code) = terminal(event) {
                        for (index, condition) in conditions.iter().enumerate() {
                            if condition
                                .page
                                .is_some_and(|page| event["page"] == json!(page))
                            {
                                ended[index].get_or_insert(code);
                            }
                        }
                        if ended.iter().all(Option::is_some) {
                            return Err(Error::from(code));
                        }
                    }
                }
                cursor = read["nextCursor"].as_u64().expect("Event cursor");
                if read["hasMore"] != true {
                    return Ok(wait_progress(context, after, cursor));
                }
                read = self.read(context, Some(cursor), 100)?;
            }
        })();
        result.map_err(|error| wait_failure(context, after, cursor, error))
    }
    pub fn push(&mut self, context: Reference, page: Reference, event: Event, resource: Reference) {
        self.record(context, Some(page), event, resource);
    }
    /// A context-level event with no owning page; page filters never match it.
    pub fn push_context(&mut self, context: Reference, event: Event, resource: Reference) {
        self.record(context, None, event, resource);
    }
    fn record(
        &mut self,
        context: Reference,
        page: Option<Reference>,
        event: Event,
        resource: Reference,
    ) {
        if matches!(
            event,
            Event::PageCreated | Event::PageClosed | Event::PageCrashed
        ) && self
            .records
            .iter()
            .any(|record| record["page"] == json!(page) && record["event"] == json!(event))
        {
            return;
        }
        self.cursor += 1;
        if self.records.len() == CAPACITY {
            self.dropped_through = self.records.pop_front().expect("Full history")["sequence"]
                .as_u64()
                .expect("Event cursor");
        }
        self.records.push_back(json!({"sequence":self.cursor,"context":context,"page":page,"event":event,"resource":resource}));
    }
    pub fn knows_page(&self, context: Reference, page: Reference) -> bool {
        self.records
            .iter()
            .any(|record| record["context"] == json!(context) && record["page"] == json!(page))
    }
    pub fn read(
        &self,
        context: Reference,
        after: Option<u64>,
        limit: usize,
    ) -> Result<Value, &'static str> {
        if !(1..=100).contains(&limit) || after.is_some_and(|after| after > MAX_CURSOR) {
            return Err("EVENT_QUERY_BOUNDS");
        }
        let after = after.unwrap_or(self.dropped_through);
        if after < self.dropped_through {
            return Err("EVENT_HISTORY_EXPIRED");
        }
        if after > self.cursor {
            return Err("EVENT_CURSOR_AHEAD");
        }
        let mut events = self.records.iter().filter(|record| {
            record["sequence"]
                .as_u64()
                .is_some_and(|sequence| sequence > after)
                && record["context"] == json!(context)
        });
        let items = events.by_ref().take(limit).cloned().collect::<Vec<_>>();
        let has_more = events.next().is_some();
        let next = if has_more {
            items.last().expect("Nonempty page")["sequence"]
                .as_u64()
                .expect("Event cursor")
        } else {
            self.cursor
        };
        Ok(
            json!({"context":context,"events":items,"nextCursor":next,"latestCursor":self.cursor,"droppedThrough":self.dropped_through,"hasMore":has_more,"contentIncluded":false}),
        )
    }
}

fn terminal(record: &Value) -> Option<&'static str> {
    match record["event"].as_str() {
        Some("page_closed") => Some("PAGE_CLOSED"),
        Some("page_crashed") => Some("PAGE_CRASHED"),
        _ => None,
    }
}
pub(crate) fn wait_progress(context: Reference, after: u64, cursor: u64) -> Value {
    json!({"context":context,"matched":false,"waiting":true,"after":after,"nextCursor":cursor,"contentIncluded":false})
}
pub(crate) fn wait_failure(context: Reference, after: u64, cursor: u64, mut error: Error) -> Error {
    if error.partial.is_none() {
        error.partial = Some(
            json!({"context":context,"matched":false,"after":after,"nextCursor":cursor,"nextAction":"events_read"}),
        );
    }
    error
}

pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let object = |properties, required: &[&str]| json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    let cursor = json!({"type":"integer","minimum":0,"maximum":MAX_CURSOR});
    let condition = object(
        json!({"event":{"enum":EVENT_NAMES},"page":reference}),
        &["event"],
    );
    let mut schemas = vec![
        object(
            json!({"operation":{"const":"events_read"},"context":reference,"after":cursor,"limit":{"type":"integer","minimum":1,"maximum":100,"default":100}}),
            &["operation", "context"],
        ),
        object(
            json!({"operation":{"const":"events_wait"},"context":reference,"after":cursor,"conditions":{"type":"array","minItems":1,"maxItems":8,"items":condition}}),
            &["operation", "context", "after", "conditions"],
        ),
    ];
    let properties = schemas[0]["properties"]
        .as_object_mut()
        .expect("event properties");
    properties.remove("after");
    properties.extend(
        crate::pagination::properties(canonic_mcp_kit::page::Limits::STANDARD)
            .as_object()
            .expect("page properties")
            .clone(),
    );
    schemas
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;
    #[test]
    fn public_pages_bind_context_and_traverse_to_null() {
        let context = reference();
        let page = reference();
        let mut history = History::default();
        for _ in 0..3 {
            history.push(context, page, Event::Dialog, reference());
        }
        let first = history.page(context, None, 1).unwrap();
        assert_eq!(first["events"].as_array().unwrap().len(), 1);
        let cursor = first["nextCursor"].as_str().unwrap();
        assert_eq!(
            history.page(reference(), Some(cursor), 1),
            Err("invalid_cursor")
        );
        assert_eq!(history.page(context, Some("bad"), 1), Err("invalid_cursor"));
        assert!(history.page(context, None, 101).is_err());
        let last = history.page(context, Some(cursor), 100).unwrap();
        assert_eq!(last["events"].as_array().unwrap().len(), 2);
        assert!(last["nextCursor"].is_null());
    }
    fn reference() -> Reference {
        Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        }
    }
    #[test]
    fn cursors_keep_scope_and_report_expired_history_without_inventing_absence() {
        let context = reference();
        let foreign = reference();
        let page = reference();
        let mut history = History::default();
        for _ in 0..CAPACITY + 1 {
            history.push(context, page, Event::Dialog, reference());
        }
        assert_eq!(
            history.read(context, Some(0), 100),
            Err("EVENT_HISTORY_EXPIRED")
        );
        assert_eq!(
            history.read(context, Some(1000), 100),
            Err("EVENT_CURSOR_AHEAD")
        );
        let first = history.read(context, None, 100).unwrap();
        assert_eq!(first["events"].as_array().unwrap().len(), 100);
        assert_eq!(first["nextCursor"], 101);
        assert_eq!(first["hasMore"], true);
        let other = history.read(foreign, Some(1), 100).unwrap();
        assert_eq!(other["events"], json!([]));
        assert_eq!(other["nextCursor"], json!(CAPACITY + 1));
        let condition = Condition {
            event: Event::Dialog,
            page: Some(page),
        };
        assert!(condition.matches(&first["events"][0]));
        assert!(history.knows_page(context, page));
        assert!(!history.knows_page(foreign, page));
    }
    #[test]
    fn sdk_lifecycle_events_round_trip_and_context_events_match_only_unfiltered_waits() {
        for name in EVENT_NAMES {
            let event: Event = serde_json::from_value(json!(name)).unwrap();
            assert_eq!(json!(event), json!(name));
        }
        let context = reference();
        let page = reference();
        let worker = reference();
        let mut history = History::default();
        history.push(context, page, Event::DomContentLoaded, page);
        history.push_context(context, Event::ServiceWorkerCreated, worker);
        let read = history.read(context, None, 100).unwrap();
        assert_eq!(read["events"][1]["page"], Value::Null);
        assert_eq!(read["events"][1]["resource"], json!(worker));
        let filtered = Condition {
            event: Event::ServiceWorkerCreated,
            page: Some(page),
        };
        let unfiltered = Condition {
            event: Event::ServiceWorkerCreated,
            page: None,
        };
        assert!(!filtered.matches(&read["events"][1]));
        assert!(unfiltered.matches(&read["events"][1]));
        let waited = history
            .wait(context, 0, 0, &[filtered, unfiltered])
            .unwrap();
        assert_eq!(waited["conditionIndex"], 1);
        assert_eq!(waited["event"]["resource"], json!(worker));
    }
    #[test]
    fn event_wait_terminal_witnesses_preserve_order_and_other_page_alternatives() {
        let context = reference();
        let page = reference();
        let other = reference();
        let mut history = History::default();
        let dialog = Condition {
            event: Event::Dialog,
            page: Some(page),
        };
        let closed = Condition {
            event: Event::PageClosed,
            page: Some(page),
        };
        history.push(context, page, Event::Dialog, reference());
        history.push(context, page, Event::PageCrashed, page);
        history.push(context, page, Event::PageClosed, page);
        assert_eq!(
            history
                .wait(context, 0, 0, std::slice::from_ref(&dialog))
                .unwrap()["nextCursor"],
            1
        );
        let failure = history
            .wait(context, 1, 1, std::slice::from_ref(&closed))
            .unwrap_err();
        assert_eq!(failure.code, "PAGE_CRASHED");
        assert_eq!(failure.partial.unwrap()["nextCursor"], 2);
        assert_eq!(
            history
                .wait(context, 2, 2, std::slice::from_ref(&dialog))
                .unwrap_err()
                .code,
            "PAGE_CRASHED"
        );
        // A different page remains eligible, even when the first alternative's
        // requested close later occurs after its crash.
        history.push(context, other, Event::Dialog, reference());
        let result = history
            .wait(
                context,
                1,
                1,
                &[
                    closed,
                    Condition {
                        event: Event::Dialog,
                        page: Some(other),
                    },
                ],
            )
            .unwrap();
        assert_eq!(result["conditionIndex"], 1);
        assert_eq!(result["nextCursor"], 4);
        let global = Condition {
            event: Event::Download,
            page: None,
        };
        assert_eq!(
            history.wait(context, 4, 4, &[dialog, global]).unwrap()["waiting"],
            true
        );
    }
    #[test]
    fn event_wait_lifecycle_witnesses_deduplicate_allocation_and_acknowledged_close() {
        let context = reference();
        let page = reference();
        let mut history = History::default();
        for event in [Event::PageCreated, Event::PageCrashed, Event::PageClosed] {
            history.push(context, page, event, page);
            history.push(context, page, event, page);
        }
        assert_eq!(
            history.read(context, None, 100).unwrap()["events"]
                .as_array()
                .unwrap()
                .len(),
            3
        );
    }
    #[test]
    fn event_wait_close_matches_before_abort_and_recovery_keeps_consumed_cursor() {
        let context = reference();
        let page = reference();
        let mut history = History::default();
        let closed = Condition {
            event: Event::PageClosed,
            page: Some(page),
        };
        let dialog = Condition {
            event: Event::Dialog,
            page: Some(page),
        };
        for _ in 0..205 {
            history.push(context, page, Event::FrameNavigated, reference());
        }
        let progress = history
            .wait(context, 0, 0, std::slice::from_ref(&dialog))
            .unwrap();
        assert_eq!(progress["nextCursor"], 205);
        assert_eq!(progress["after"], 0);
        // Once consumed, the original cursor may leave retention without losing
        // this wait's progress. An unconsumed gap still refuses explicitly.
        for _ in 0..400 {
            history.push(context, page, Event::FrameNavigated, reference());
        }
        assert_eq!(
            history
                .wait(context, 0, 205, std::slice::from_ref(&dialog))
                .unwrap()["nextCursor"],
            605
        );
        let gap = history
            .wait(context, 0, 0, std::slice::from_ref(&dialog))
            .unwrap_err();
        assert_eq!(gap.code, "EVENT_HISTORY_EXPIRED");
        assert_eq!(gap.partial.unwrap()["nextCursor"], 0);
        history.push(context, page, Event::PageClosed, page);
        assert_eq!(
            history
                .wait(context, 605, 605, std::slice::from_ref(&closed))
                .unwrap()["event"]["event"],
            "page_closed"
        );
        let failure = history.wait(context, 605, 605, &[dialog]).unwrap_err();
        assert_eq!(failure.code, "PAGE_CLOSED");
        assert_eq!(failure.partial.unwrap()["nextCursor"], 606);
    }
}

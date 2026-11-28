//! Public diagnostic pages; internal waits retain typed sequence markers.
use super::{Diagnostics, Query, Reference};
use canonic_mcp_kit::page::{Builder, Limits, Position, Request};
use serde_json::{Value, json};

const PAGE_LIMITS: Limits = Limits {
    max_bytes: 7000,
    ..Limits::STANDARD
};

impl Diagnostics {
    pub(crate) fn page(&self, context: Reference, query: &Query) -> Result<Value, &'static str> {
        query.validate()?;
        let request = Request::from_cli(
            query.cursor.as_deref(),
            Some(query.limit as u64),
            PAGE_LIMITS,
        )
        .map_err(|error| error.code())?;
        let mut options = serde_json::to_value(query).expect("diagnostic query");
        let filters = crate::pagination::filters(&options);
        let scope = crate::pagination::scope("browser_send.diagnostics_read", &(context, filters))
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
            None => query
                .sequence
                .map(|sequence| sequence - 1)
                .or(query.after)
                .unwrap_or(self.evicted_through),
            _ => return Err("invalid_cursor"),
        };
        if after < self.evicted_through || after > self.sequence {
            return Err("invalid_cursor");
        }
        let through = query.sequence.or(query.through).unwrap_or(self.sequence);
        if through > self.sequence || through < self.evicted_through {
            return Err("invalid_cursor");
        }
        let latest = self.latest_matching(context, query, after, through);
        let mut rows = self.events.iter().filter(|event| {
            event["sequence"].as_u64().is_some_and(|sequence| {
                sequence > after
                    && sequence <= through
                    && (!query.latest || Some(sequence) == latest)
            }) && query.matches(context, event)
        });
        let mut builder = Builder::new(
            &scope,
            request.limit,
            PAGE_LIMITS,
            PAGE_LIMITS.max_bytes,
            "browser_send diagnostics_read query.sequence",
        )
        .map_err(|error| error.code())?;
        while !builder.is_full() {
            let Some(event) = rows.next() else {
                break;
            };
            let projected = self.project_event(context, event, query);
            let sequence = event["sequence"].as_u64().expect("sequence");
            if !builder
                .push(
                    projected,
                    &Position::Keyset(sequence.to_be_bytes().to_vec()),
                )
                .map_err(|error| error.code())?
            {
                break;
            }
        }
        let page =
            builder.finish(query.sequence.is_none() && !query.latest && rows.next().is_some());
        if query.sequence.is_some() && page.items.is_empty() {
            return Err("DIAGNOSTIC_EVENT_NOT_FOUND");
        }
        let next = page.next_cursor.as_ref().map(|cursor| {
            options
                .as_object_mut()
                .expect("query object")
                .retain(|_, value| !value.is_null());
            options["cursor"] = json!(cursor);
            json!({"operation":"diagnostics_read","context":context,"query":options})
        });
        Ok(
            json!({"context":context,"events":page.items,"nextCursor":page.next_cursor,"nextRequest":next,"oldestSequence":self.evicted_through,"latestSequence":self.sequence,"observedRange":{"afterSequence":after,"throughSequence":through},"latestOnly":query.latest,"active":self.active(context),"subscriptionPending":self.pending.contains(&context),"forgottenRequestIdentities":self.forgotten_requests,"forgottenWebSocketIdentities":self.forgotten_sockets,"correlation":if query.correlation.is_some(){"source-identity-or-unproven-candidates"}else{"exact-network-identity-only"},"textIncluded":query.include_text,"bodiesIncluded":false,"headersIncluded":false,"dropped":self.dropped,"contentIncluded":false}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::diagnostics::DiagnosticKind;
    use uuid::Uuid;

    #[test]
    fn browser_diagnostics_limits_events_and_bytes_and_ends_at_the_last_record() {
        let mut history = Diagnostics::default();
        let context = Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        for _ in 0..50 {
            history.push(
                context,
                None,
                DiagnosticKind::Console,
                json!({"text":"é".repeat(1000)}),
                0,
            );
        }
        let query = Query {
            include_text: true,
            ..Default::default()
        };
        let page = history.page(context, &query).unwrap();
        assert!(page["events"].as_array().unwrap().len() > 1);
        assert!(page["events"].to_string().len() <= PAGE_LIMITS.max_bytes);

        let mut query = Query { limit: 1, ..query };
        for sequence in 1..=50 {
            let page = history.page(context, &query).unwrap();
            assert_eq!(page["events"].as_array().unwrap().len(), 1);
            assert_eq!(page["events"][0]["sequence"], sequence);
            assert!(page.to_string().len() < 3500);
            query.cursor = page["nextCursor"].as_str().map(str::to_owned);
            assert_eq!(query.cursor.is_none(), sequence == 50);
        }
    }
}

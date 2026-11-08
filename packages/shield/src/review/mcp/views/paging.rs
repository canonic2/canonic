//! One live continuation per traversal and bounded, idempotent replay receipts.
use super::{Arc, Cursor, Error, Value, Views, json, limit, query, required};

const CURSOR_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone)]
pub(super) enum Content {
    Pending {
        data: Arc<Value>,
        field: String,
        offset: usize,
        bytes: usize,
    },
    Replay {
        page: Arc<Value>,
        bytes: usize,
    },
}
impl Content {
    fn bytes(&self) -> usize {
        match self {
            Self::Pending { bytes, .. } | Self::Replay { bytes, .. } => *bytes,
        }
    }
}

impl Views {
    pub(in crate::review::mcp) fn page(
        &mut self,
        name: &str,
        args: &Value,
        data: Value,
        field: &str,
    ) -> Result<Value, Error> {
        if args.get("cursor").is_some() {
            return self.resume(name, args);
        }
        if !data[field].is_array() {
            return Ok(data);
        }
        let (mut page, end, more) = slice(&data, field, 0, limit(args)?)?;
        if more {
            let query = query(name, args);
            let bytes = crate::json::encoded_len(&data)
                .map_err(|_| Error::Overloaded)?
                .saturating_add(query.len());
            // An admitted traversal must fit its full view and one maximum
            // page receipt together, even when every other cursor is reclaimed.
            if bytes
                .saturating_add(1024 * 1024)
                .saturating_add(query.len())
                > CURSOR_BYTES
            {
                return Err(Error::Overloaded);
            }
            let id = self.id("cursor");
            page["next_cursor"] = json!(id);
            page_limit(&page)?;
            self.cursor_room(None, bytes, 1)?;
            self.cursors.insert(
                id.clone(),
                Cursor {
                    query,
                    used: self.sequence,
                    content: Content::Pending {
                        data: Arc::new(data),
                        field: field.into(),
                        offset: end,
                        bytes,
                    },
                },
            );
            self.cursor_bytes += bytes;
        } else {
            page_limit(&page)?;
        }
        Ok(page)
    }

    pub(in crate::review::mcp) fn resume(
        &mut self,
        name: &str,
        args: &Value,
    ) -> Result<Value, Error> {
        let id = required(args, "cursor")?;
        let cursor = self
            .cursors
            .get(id)
            .cloned()
            .ok_or(Error::CursorUnavailable)?;
        if cursor.query != query(name, args) {
            return Err(Error::CursorUnavailable);
        }
        let limit = limit(args)?;
        if let Content::Replay { page, .. } = cursor.content {
            self.sequence += 1;
            self.cursors
                .get_mut(id)
                .ok_or(Error::CursorUnavailable)?
                .used = self.sequence;
            return Ok(page.as_ref().clone());
        }
        let Content::Pending {
            data,
            field,
            offset,
            bytes,
        } = cursor.content
        else {
            unreachable!()
        };
        let (mut page, end, more) = slice(&data, &field, offset, limit)?;
        let next = more.then(|| self.id("cursor"));
        page["next_cursor"] = json!(next);
        page_limit(&page)?;
        let replay_bytes = crate::json::encoded_len(&page)
            .map_err(|_| Error::Overloaded)?
            .saturating_add(cursor.query.len());
        self.cursor_room(
            Some(id),
            replay_bytes.saturating_add(if more { bytes } else { 0 }),
            if more { 2 } else { 1 },
        )?;
        self.cursor_bytes -= bytes;
        self.sequence += 1;
        self.cursors.insert(
            id.into(),
            Cursor {
                query: cursor.query.clone(),
                used: self.sequence,
                content: Content::Replay {
                    page: Arc::new(page.clone()),
                    bytes: replay_bytes,
                },
            },
        );
        self.cursor_bytes += replay_bytes;
        if let Some(next) = next {
            self.cursors.insert(
                next,
                Cursor {
                    query: cursor.query,
                    used: self.sequence,
                    content: Content::Pending {
                        data,
                        field,
                        offset: end,
                        bytes,
                    },
                },
            );
            self.cursor_bytes += bytes;
        }
        Ok(page)
    }

    fn cursor_room(
        &mut self,
        replace: Option<&str>,
        bytes: usize,
        slots: usize,
    ) -> Result<(), Error> {
        if bytes > CURSOR_BYTES {
            return Err(Error::Overloaded);
        }
        let released = replace
            .and_then(|id| self.cursors.get(id))
            .map_or(0, |c| c.content.bytes());
        let replacing = usize::from(replace.is_some());
        while self.cursors.len() - replacing + slots > 256
            || self
                .cursor_bytes
                .saturating_sub(released)
                .saturating_add(bytes)
                > CURSOR_BYTES
        {
            // Completed-page receipts go first; abandoned traversals are next.
            let victim = self
                .cursors
                .iter()
                .filter(|(id, _)| Some(id.as_str()) != replace)
                .min_by_key(|(_, c)| (matches!(c.content, Content::Pending { .. }), c.used))
                .map(|(id, _)| id.clone())
                .ok_or(Error::Overloaded)?;
            let cursor = self
                .cursors
                .remove(&victim)
                .ok_or(Error::CursorUnavailable)?;
            self.cursor_bytes -= cursor.content.bytes();
            crate::diagnostics::trace::count("cursor.reclaimed", 1);
        }
        Ok(())
    }
}

fn slice(
    data: &Value,
    field: &str,
    offset: usize,
    limit: usize,
) -> Result<(Value, usize, bool), Error> {
    let rows = data[field].as_array().ok_or(Error::CursorUnavailable)?;
    let end = offset.saturating_add(limit).min(rows.len());
    let mut page = Value::Object(
        data.as_object()
            .ok_or(Error::CursorUnavailable)?
            .iter()
            .filter(|(name, _)| name.as_str() != field)
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect(),
    );
    crate::diagnostics::trace::count("cursor.rows_copied", (end - offset) as u64);
    page[field] = json!(&rows[offset..end]);
    page["next_cursor"] = Value::Null;
    if page.get("truncated").is_none() {
        page["truncated"] = json!(false);
    }
    Ok((page, end, end < rows.len()))
}

fn page_limit(page: &Value) -> Result<(), Error> {
    if !crate::json::fits(page, 1024 * 1024) {
        return Err(crate::policy::Error::Limit.into());
    }
    Ok(())
}

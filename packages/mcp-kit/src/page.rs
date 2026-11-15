//! I/O-free application pagination: validated limits, scoped stateless cursors,
//! incremental JSON budgeting and shared schemas. Protocol catalogs are separate.

mod cursor;
#[cfg(test)]
mod tests;

pub use cursor::{Position, Scope, digest};
use serde_json::{Value, json};

/// Standard maximum encoded cursor length, in bytes.
pub const MAX_CURSOR_BYTES: usize = 256;
/// Largest raw keyset key that fits the versioned cursor envelope.
pub const MAX_KEY_BYTES: usize = 110;

/// Page admission or continuation failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// Bad limit, argument type or limits configuration.
    #[error("invalid_arguments: {0}")]
    InvalidArguments(String),
    /// Invalid, foreign or stale continuation.
    #[error("invalid_cursor: restart without a cursor")]
    InvalidCursor,
    /// A producer's position cannot fit the cursor format or scope.
    #[error("Invalid page position: {0}")]
    InvalidPosition(String),
    /// One item cannot fit the consumer's transport allowance.
    #[error("Item exceeds the transport limit; read it alone with {read_tool}")]
    ItemTooLarge {
        /// Tool that reads the item alone.
        read_tool: String,
    },
}

impl Error {
    /// Stable surface error category for product envelopes and invalid arguments.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidCursor => "invalid_cursor",
            Self::InvalidArguments(_) => "invalid_arguments",
            Self::InvalidPosition(_) => "invalid_position",
            Self::ItemTooLarge { .. } => "item_too_large",
        }
    }
}

/// Declared row and encoded JSON array budgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Default number of units.
    pub default: u64,
    /// Maximum number of units.
    pub max: u64,
    /// Maximum encoded JSON array bytes, allowing an oversized first item.
    pub max_bytes: usize,
}

impl Limits {
    /// Ordinary list limits: 50 default, 100 maximum, 256 KiB.
    pub const STANDARD: Self = Self {
        default: 50,
        max: 100,
        max_bytes: 256 * 1024,
    };

    /// Checks custom limits, which may only reduce ordinary list budgets.
    ///
    /// # Errors
    /// Returns invalid arguments for zero, inconsistent or larger limits.
    pub fn validate(self) -> Result<Self, Error> {
        self.validate_units()?;
        if self.default > Self::STANDARD.default || self.max > Self::STANDARD.max {
            return Err(invalid("custom list limits must not exceed 50/100"));
        }
        Ok(self)
    }

    /// Checks explicitly specified stream unit limits (lines, bytes or files).
    ///
    /// # Errors
    /// Returns invalid arguments for inconsistent or zero limits or byte budgets
    /// outside 2..=256 KiB. This does not raise ordinary list limits.
    pub fn validate_units(self) -> Result<Self, Error> {
        if self.default == 0
            || self.default > self.max
            || !(2..=Self::STANDARD.max_bytes).contains(&self.max_bytes)
        {
            return Err(invalid("invalid default, maximum or byte budget"));
        }
        Ok(self)
    }

    /// Validates an ordinary list count, applying the declared default.
    ///
    /// # Errors
    /// Returns invalid arguments rather than clamping out-of-range values.
    pub fn limit(self, value: Option<u64>) -> Result<Limit, Error> {
        self.validate()?;
        self.unit_limit(value)
    }

    /// Validates an explicitly declared stream unit count.
    ///
    /// # Errors
    /// Returns invalid arguments for invalid unit limits or out-of-range values.
    pub fn unit_limit(self, value: Option<u64>) -> Result<Limit, Error> {
        self.validate_units()?;
        let value = value.unwrap_or(self.default);
        if value == 0 || value > self.max {
            return Err(invalid("limit is outside the declared range"));
        }
        Ok(Limit(value))
    }

    /// JSON Schema properties for optional cursor and limit arguments.
    #[must_use]
    pub fn properties(self) -> Value {
        json!({"cursor":{"type":"string",
            "description":"Opaque URL-safe continuation of at most 256 bytes; reuse with the same filters. Invalid cursors refuse with invalid_cursor."},
            "limit":{"type":"integer","minimum":1,"maximum":self.max,"default":self.default}})
    }
}

/// A validated positive page size, passed to library stores instead of raw input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limit(u64);

impl Limit {
    /// Number of requested units.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Parsed wire inputs. Decode `cursor` with the query's scope before reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Opaque continuation, if supplied.
    pub cursor: Option<String>,
    /// Validated unit count.
    pub limit: Limit,
}

impl Request {
    /// Parses MCP properties without rejecting product-owned filter properties.
    ///
    /// # Errors
    /// Returns invalid arguments for a bad object or limit, invalid cursor for
    /// a bad cursor type or syntax.
    pub fn from_mcp(arguments: &Value, limits: Limits) -> Result<Self, Error> {
        limits.validate()?;
        Self::from_mcp_units(arguments, limits)
    }

    /// Parses MCP arguments for a stream with explicitly declared unit limits.
    ///
    /// # Errors
    /// Returns invalid arguments for bad objects/limits, invalid cursor for bad cursors.
    pub fn from_mcp_units(arguments: &Value, limits: Limits) -> Result<Self, Error> {
        let object = arguments
            .as_object()
            .ok_or_else(|| invalid("arguments must be an object"))?;
        let cursor = object
            .get("cursor")
            .map(|v| v.as_str().ok_or(Error::InvalidCursor))
            .transpose()?;
        let limit = object
            .get("limit")
            .map(|v| {
                v.as_u64()
                    .ok_or_else(|| invalid("limit must be a positive integer"))
            })
            .transpose()?;
        Self::from_cli_units(cursor, limit, limits)
    }

    /// Parses clap-independent CLI values; products declare their own flags.
    ///
    /// # Errors
    /// Returns invalid arguments for limits and invalid cursor for malformed cursors.
    pub fn from_cli(
        cursor: Option<&str>,
        limit: Option<u64>,
        limits: Limits,
    ) -> Result<Self, Error> {
        limits.validate()?;
        Self::from_cli_units(cursor, limit, limits)
    }

    /// Parses framework-free CLI values for a stream's declared unit limits.
    ///
    /// # Errors
    /// Returns invalid arguments for limits, invalid cursor for malformed cursors.
    pub fn from_cli_units(
        cursor: Option<&str>,
        limit: Option<u64>,
        limits: Limits,
    ) -> Result<Self, Error> {
        if let Some(cursor) = cursor {
            cursor::decode(cursor)?;
        }
        Ok(Self {
            cursor: cursor.map(str::to_owned),
            limit: limits.unit_limit(limit)?,
        })
    }
}

/// JSON field convention for a product's results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Casing {
    /// `next_cursor`.
    Snake,
    /// `nextCursor`.
    Camel,
}

impl Casing {
    /// Continuation field name.
    #[must_use]
    pub const fn next_cursor(self) -> &'static str {
        match self {
            Self::Snake => "next_cursor",
            Self::Camel => "nextCursor",
        }
    }
}

/// Output-schema properties fragment for the required nullable continuation.
#[must_use]
pub fn output_properties(casing: Casing) -> Value {
    json!({casing.next_cursor():{"type":["string","null"],"minLength":1,"maxLength":MAX_CURSOR_BYTES,"pattern":"^[A-Za-z0-9_-]+$"}})
}

/// One bounded application page. A total is a product-owned optional extension.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// Already serialized items in store order.
    pub items: Vec<Value>,
    /// Continuation or end of collection.
    pub next_cursor: Option<String>,
}

impl Page {
    /// Renders shared fields in the product's JSON convention.
    #[must_use]
    pub fn render(self, casing: Casing) -> Value {
        json!({"items":self.items, casing.next_cursor():self.next_cursor})
    }
}

/// Incremental page admission. Stop store iteration when `push` returns false
/// or `is_full` becomes true. Positions name the last accepted item (keyset)
/// or next unread unit (immutable offset). No collection is materialized here.
#[derive(Debug)]
pub struct Builder<'a> {
    scope: &'a Scope,
    limit: Limit,
    limits: Limits,
    transport_bytes: usize,
    read_tool: &'a str,
    items: Vec<Value>,
    bytes: usize,
    last: Option<String>,
    stopped: bool,
}

impl<'a> Builder<'a> {
    /// Creates a builder. `transport_bytes` is the maximum encoded items array
    /// after reserving envelope overhead (including any duplicated text content).
    ///
    /// # Errors
    /// Returns invalid arguments for inconsistent limits or transport allowance.
    pub fn new(
        scope: &'a Scope,
        limit: Limit,
        limits: Limits,
        transport_bytes: usize,
        read_tool: &'a str,
    ) -> Result<Self, Error> {
        limits.validate_units()?;
        limits.unit_limit(Some(limit.get()))?;
        if transport_bytes < limits.max_bytes {
            return Err(invalid("transport allowance is below the page budget"));
        }
        Ok(Self {
            scope,
            limit,
            limits,
            transport_bytes,
            read_tool,
            items: Vec::new(),
            bytes: 2,
            last: None,
            stopped: false,
        })
    }

    /// Tries one serialized item. False means it was NOT admitted: stop reading
    /// and resume after the previous accepted position. A first oversized item
    /// is admitted alone, subject to the transport allowance.
    ///
    /// # Errors
    /// Returns a position error or an oversized-item error naming `read_tool`.
    pub fn push(&mut self, item: Value, position: &Position) -> Result<bool, Error> {
        if self.is_full() {
            self.stopped = true;
            return Ok(false);
        }
        let bytes = item.to_string().len() + usize::from(!self.items.is_empty());
        if !self.items.is_empty() && self.bytes.saturating_add(bytes) > self.limits.max_bytes {
            self.stopped = true;
            return Ok(false);
        }
        if self.bytes.saturating_add(bytes) > self.transport_bytes {
            return Err(Error::ItemTooLarge {
                read_tool: self.read_tool.to_owned(),
            });
        }
        self.last = Some(self.scope.encode(position)?);
        self.bytes += bytes;
        self.items.push(item);
        Ok(true)
    }

    /// Whether reading should stop, without needing to fetch another row.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.stopped
            || self.items.len() as u64 >= self.limit.get()
            || (!self.items.is_empty() && self.bytes >= self.limits.max_bytes)
    }

    /// Finishes. `more` comes from the store, or is conservatively true if
    /// iteration stopped without checking exhaustion. Pass false on known end.
    #[must_use]
    pub fn finish(self, more: bool) -> Page {
        Page {
            items: self.items,
            next_cursor: if more || self.stopped {
                self.last
            } else {
                None
            },
        }
    }
}

fn invalid(message: &str) -> Error {
    Error::InvalidArguments(message.to_owned())
}

/// Builds MCP-shaped arguments from CLI values, omitting absent optional fields.
/// Admission remains the handler's responsibility through [`Request::from_mcp`].
#[must_use]
pub fn arguments(cursor: Option<&str>, limit: Option<u64>) -> Value {
    let mut value = json!({});
    if let Some(cursor) = cursor {
        value["cursor"] = json!(cursor);
    }
    if let Some(limit) = limit {
        value["limit"] = json!(limit);
    }
    value
}

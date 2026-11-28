//! Native Chrome Cache Storage of one origin in an owned context: caches,
//! entries, one bounded response, and deletion. No SDK API covers it.
use super::super::{contract::Reference, session::Error};
use serde_json::{Value, json};

/// Listings read entries whole and slice them; a larger cache refuses them.
pub(crate) const MAX_ENTRIES: u64 = 1000;
/// The decoded body size a response read returns, as other `maxBodyBytes`.
pub(crate) const MAX_BODY_BYTES: usize = 1024 * 1024;

/// The decoded size of Chrome's padded base64 body, without decoding it.
fn decoded_len(base64: &str) -> usize {
    let padding = base64
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'=')
        .count();
    (base64.len() / 4 * 3).saturating_sub(padding.min(2))
}

pub(crate) fn validate_origin(origin: &str) -> Result<(), Error> {
    super::super::settings::validate_origin(origin)?;
    Ok(())
}
pub(crate) fn validate_read(
    origin: &str,
    url: Option<&str>,
    cache: Option<&str>,
    cursor: Option<&str>,
    limit: Option<u64>,
) -> Result<(), Error> {
    validate_origin(origin)?;
    if url.is_some() && (cache.is_none() || cursor.is_some() || limit.is_some()) {
        return Err("INVALID_ARGUMENT".into());
    }
    Ok(())
}

/// The cache id for `name` among the origin's caches.
pub(crate) fn cache_id(caches: &Value, name: &str) -> Result<String, Error> {
    caches["caches"]
        .as_array()
        .ok_or("BACKEND_PROTOCOL")?
        .iter()
        .find(|cache| cache["cacheName"] == name)
        .and_then(|cache| cache["cacheId"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| Error::from("CACHE_NOT_FOUND"))
}

pub(crate) fn caches_page(
    context: Reference,
    origin: &str,
    caches: &Value,
    cursor: Option<String>,
    limit: Option<u64>,
) -> Result<Value, Error> {
    let rows = caches["caches"]
        .as_array()
        .ok_or("BACKEND_PROTOCOL")?
        .iter()
        .map(|cache| {
            let name = cache["cacheName"].as_str().ok_or("BACKEND_PROTOCOL")?;
            Ok((crate::pagination::identity_key(name), name.to_owned()))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let (items, cursor) = super::keyset(
        "browser_send.context_cache_storage_read.caches",
        &json!({"context":context,"origin":origin}),
        rows,
        |name| json!({"cacheName":name}),
        cursor.as_deref(),
        limit,
        "browser_send context_cache_storage_read with cacheName",
    )?;
    Ok(json!({"context":context,"origin":origin,"caches":items,"nextCursor":cursor}))
}

fn entry(entry: &Value) -> Value {
    json!({"url":entry["requestURL"],"method":entry["requestMethod"],"requestHeaders":entry["requestHeaders"],
        "status":entry["responseStatus"],"statusText":entry["responseStatusText"],"responseType":entry["responseType"],
        "responseTime":entry["responseTime"],"responseHeaders":entry["responseHeaders"]})
}
pub(crate) fn entries_page(
    context: Reference,
    origin: &str,
    cache: &str,
    entries: &Value,
    cursor: Option<String>,
    limit: Option<u64>,
) -> Result<Value, Error> {
    if entries["returnCount"]
        .as_u64()
        .is_some_and(|count| count > MAX_ENTRIES)
    {
        return Err("CACHE_STORAGE_ENTRY_BOUNDS".into());
    }
    let rows = entries["cacheDataEntries"]
        .as_array()
        .ok_or("BACKEND_PROTOCOL")?
        .iter()
        .map(|item| {
            let identity = json!([
                item["requestURL"],
                item["requestMethod"],
                item["requestHeaders"]
            ]);
            (crate::pagination::identity_key(&identity.to_string()), item)
        })
        .collect::<Vec<_>>();
    let (items, cursor) = super::keyset(
        "browser_send.context_cache_storage_read.entries",
        &json!({"context":context,"origin":origin,"cacheName":cache}),
        rows,
        entry,
        cursor.as_deref(),
        limit,
        "browser_send context_cache_storage_read with url",
    )?;
    Ok(
        json!({"context":context,"origin":origin,"cacheName":cache,"entries":items,"nextCursor":cursor,"secretBearing":true}),
    )
}
/// Entries requested per backend read: listings read one window, URL reads
/// scan window by window.
pub(crate) const WINDOW: u64 = MAX_ENTRIES + 1;

/// The outcome of searching one window of entries read from `skip`.
#[derive(Debug, PartialEq)]
pub(crate) enum Lookup {
    /// The first entry for the URL, which supplies the stored request headers.
    Found(Value),
    /// The window was full; read the next one from this skip count.
    Next(u64),
}

pub(crate) fn lookup(entries: &Value, url: &str, skip: u64) -> Result<Lookup, Error> {
    let window = entries["cacheDataEntries"]
        .as_array()
        .ok_or("BACKEND_PROTOCOL")?;
    if let Some(found) = window.iter().find(|entry| entry["requestURL"] == url) {
        return Ok(Lookup::Found(found.clone()));
    }
    let read = window.len() as u64;
    if read < WINDOW {
        return Err("CACHE_ENTRY_NOT_FOUND".into());
    }
    Ok(Lookup::Next(skip + read))
}
pub(crate) fn response(
    context: Reference,
    origin: &str,
    cache: &str,
    found: &Value,
    body: &Value,
) -> Result<Value, Error> {
    let body = body["response"]["body"]
        .as_str()
        .ok_or("BACKEND_PROTOCOL")?;
    if decoded_len(body) > MAX_BODY_BYTES {
        return Err("CACHE_STORAGE_READ_BOUNDS".into());
    }
    Ok(
        json!({"context":context,"origin":origin,"cacheName":cache,"entry":entry(found),"bodyBase64":body,"secretBearing":true}),
    )
}
pub(crate) fn deleted(context: Reference, origin: &str, cache: &str, url: Option<&str>) -> Value {
    json!({"context":context,"origin":origin,"cacheName":cache,"url":url,"deleted":true})
}

pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let text = json!({"type":"string","minLength":1,"maxLength":4096});
    let mut read = json!({"operation":{"const":"context_cache_storage_read"},"context":reference,"origin":text,"cacheName":text,"url":text});
    read.as_object_mut().expect("read properties").extend(
        crate::pagination::properties(canonic_mcp_kit::page::Limits::STANDARD)
            .as_object()
            .expect("page properties")
            .clone(),
    );
    vec![
        json!({"type":"object","properties":read,"required":["operation","context","origin"],"additionalProperties":false}),
        json!({"type":"object","properties":{"operation":{"const":"context_cache_storage_delete"},"context":reference,"origin":text,"cacheName":text,"url":text},"required":["operation","context","origin","cacheName"],"additionalProperties":false}),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn context() -> Reference {
        Reference {
            connection_id: Uuid::nil(),
            id: Uuid::nil(),
            generation: Uuid::nil(),
        }
    }

    #[test]
    fn cache_storage_pages_name_order_and_refuse_oversized_caches_and_bodies() {
        let caches =
            json!({"caches":[{"cacheId":"2","cacheName":"b"},{"cacheId":"1","cacheName":"a"}]});
        assert_eq!(cache_id(&caches, "b").unwrap(), "2");
        assert_eq!(cache_id(&caches, "z").unwrap_err().code, "CACHE_NOT_FOUND");
        let page = caches_page(context(), "https://example.com", &caches, None, None).unwrap();
        assert_eq!(page["caches"], json!([{"cacheName":"a"},{"cacheName":"b"}]));
        let entries = json!({"returnCount":2,"cacheDataEntries":[
            {"requestURL":"https://example.com/b","requestMethod":"GET","requestHeaders":[],"responseStatus":200},
            {"requestURL":"https://example.com/a","requestMethod":"GET","requestHeaders":[],"responseStatus":404}]});
        let first = entries_page(
            context(),
            "https://example.com",
            "a",
            &entries,
            None,
            Some(1),
        )
        .unwrap();
        assert_eq!(first["entries"][0]["url"], "https://example.com/a");
        assert_eq!(first["entries"][0]["status"], 404);
        assert!(first["nextCursor"].is_string());
        let oversized = json!({"returnCount":MAX_ENTRIES + 1,"cacheDataEntries":[]});
        assert_eq!(
            entries_page(
                context(),
                "https://example.com",
                "a",
                &oversized,
                None,
                None
            )
            .unwrap_err()
            .code,
            "CACHE_STORAGE_ENTRY_BOUNDS"
        );
        let Lookup::Found(found) = lookup(&entries, "https://example.com/b", 0).unwrap() else {
            panic!("entry in the first window");
        };
        let body = json!({"response":{"body":"QWNtZQ=="}});
        assert_eq!(
            response(context(), "https://example.com", "a", &found, &body).unwrap()["bodyBase64"],
            "QWNtZQ=="
        );
        assert_eq!(
            validate_read(
                "https://example.com",
                Some("https://example.com/a"),
                None,
                None,
                None
            )
            .unwrap_err()
            .code,
            "INVALID_ARGUMENT"
        );
        assert!(validate_read("not an origin", None, None, None, None).is_err());
    }

    #[test]
    fn cache_body_bound_counts_decoded_bytes() {
        assert_eq!(decoded_len("QWNtZQ=="), 4);
        assert_eq!(decoded_len("QWNt"), 3);
        let found = json!({"requestURL":"https://example.com/a"});
        let read = |bytes: usize| {
            // Exactly `bytes` decoded bytes: whole groups of three, no padding.
            let body = json!({"response":{"body":"QUFB".repeat(bytes / 3)}});
            response(context(), "https://example.com", "a", &found, &body)
        };
        // A 1 MiB body encodes to about 1.33 MiB of base64 and is still read.
        assert!(read(MAX_BODY_BYTES / 3 * 3).is_ok());
        assert_eq!(
            read(MAX_BODY_BYTES + 3).unwrap_err().code,
            "CACHE_STORAGE_READ_BOUNDS"
        );
    }

    #[test]
    fn cache_url_reads_reach_entries_past_the_listing_bound() {
        let entries = |range: std::ops::Range<u64>| json!({"cacheDataEntries":range.map(|index| json!({"requestURL":format!("https://example.com/{index}")})).collect::<Vec<_>>()});
        let target = "https://example.com/1500";
        assert_eq!(
            lookup(&entries(0..WINDOW), target, 0).unwrap(),
            Lookup::Next(WINDOW)
        );
        let Lookup::Found(found) = lookup(&entries(WINDOW..1600), target, WINDOW).unwrap() else {
            panic!("entry in the second window");
        };
        assert_eq!(found["requestURL"], target);
        assert_eq!(
            lookup(&entries(WINDOW..1200), "https://example.com/z", WINDOW)
                .unwrap_err()
                .code,
            "CACHE_ENTRY_NOT_FOUND"
        );
    }
}

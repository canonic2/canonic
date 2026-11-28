//! Bounded ECMAScript predicates over cookie attributes, without host bindings.
use super::*;
use std::sync::{OnceLock, atomic::AtomicBool};
use tokio::sync::Semaphore;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CookieFilter {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub name: Option<CookieMatch>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub domain: Option<CookieMatch>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub path: Option<CookieMatch>,
}
fn present<'de, D: serde::Deserializer<'de>>(input: D) -> Result<Option<CookieMatch>, D::Error> {
    CookieMatch::deserialize(input).map(Some)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CookieMatch {
    Exact(String),
    Regex(CookieRegex),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CookieRegex {
    pub regex: String,
    #[serde(default)]
    pub flags: String,
}
impl CookieFilter {
    pub(crate) fn is_empty(&self) -> bool {
        self.name.is_none() && self.domain.is_none() && self.path.is_none()
    }
    pub(crate) fn validate(&self) -> Result<(), Error> {
        for value in [&self.name, &self.domain, &self.path].into_iter().flatten() {
            match value {
                CookieMatch::Exact(value) if value.len() > 65536 => {
                    return Err("HTTP_COOKIE_FILTER_BOUNDS".into());
                }
                CookieMatch::Regex(value) if value.regex.len() > 2048 || value.flags.len() > 8 => {
                    return Err("HTTP_COOKIE_FILTER_BOUNDS".into());
                }
                _ => {}
            }
        }
        Ok(())
    }
    pub(crate) fn schema() -> Value {
        let criterion = json!({"anyOf":[{"type":"string","maxLength":65536},
            {"type":"object","properties":{"regex":{"type":"string","maxLength":2048},"flags":{"type":"string","maxLength":8,"default":""}},"required":["regex"],"additionalProperties":false}]});
        json!({"type":"object","properties":{"name":criterion,"domain":criterion,"path":criterion},"additionalProperties":false})
    }
}
mod native;
struct Stop(Arc<AtomicBool>);
pub(crate) struct FilterLease(Lease);
impl Contexts {
    pub(crate) fn cookie_filter_lease(&self, reference: Reference) -> Result<FilterLease, Error> {
        let client = self.get(reference)?;
        Ok(FilterLease(client.filter_lease()?))
    }
}
pub(crate) fn selected(values: Vec<Value>, indices: Vec<usize>) -> Result<Vec<Value>, Error> {
    if indices.last().is_some_and(|index| *index >= values.len())
        || indices.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err("HTTP_COOKIE_FILTER_FAILED".into());
    }
    let mut indices = indices.into_iter().peekable();
    Ok(values
        .into_iter()
        .enumerate()
        .filter_map(|(index, value)| {
            if indices.peek() == Some(&index) {
                indices.next();
                Some(value)
            } else {
                None
            }
        })
        .collect())
}
impl Drop for Stop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
/// Browser contexts own no HTTP-context lease; dropping this future still
/// stops its worker through the `Stop` guard.
pub(crate) async fn select(
    values: &[Value],
    filter: CookieFilter,
    deadline: Instant,
) -> Result<Vec<usize>, Error> {
    select_work(values, filter, deadline, None).await
}
pub(crate) async fn select_owned(
    values: &[Value],
    filter: CookieFilter,
    deadline: Instant,
    lease: FilterLease,
) -> Result<Vec<usize>, Error> {
    select_work(values, filter, deadline, Some(lease)).await
}
async fn select_work(
    values: &[Value],
    filter: CookieFilter,
    deadline: Instant,
    lease: Option<FilterLease>,
) -> Result<Vec<usize>, Error> {
    filter.validate()?;
    if Instant::now() >= deadline {
        return Err("HTTP_DEADLINE".into());
    }
    if values.len() > 4096 {
        return Err("HTTP_COOKIE_FILTER_BOUNDS".into());
    }
    let mut attributes = Vec::with_capacity(values.len());
    let mut bytes = 0usize;
    for value in values {
        let name = value["name"]
            .as_str()
            .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?;
        let domain = value["domain"]
            .as_str()
            .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?;
        let path = value["path"]
            .as_str()
            .ok_or("HTTP_BROWSER_COOKIE_PROTOCOL")?;
        bytes = bytes
            .checked_add(name.len() + domain.len() + path.len())
            .filter(|size| *size <= 1048576)
            .ok_or("HTTP_COOKIE_FILTER_BOUNDS")?;
        attributes.push([name.to_owned(), domain.to_owned(), path.to_owned()]);
    }
    {
        let envelope = json!({"cookies":attributes,"filter":filter});
        super::super::schema::encoded_len(&envelope, 1048576)
            .map_err(|_| Error::from("HTTP_COOKIE_FILTER_BOUNDS"))?;
    }
    static WORKERS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    let permit = tokio::time::timeout_at(
        deadline,
        WORKERS
            .get_or_init(|| Arc::new(Semaphore::new(4)))
            .clone()
            .acquire_owned(),
    )
    .await
    .map_err(|_| Error::from("HTTP_DEADLINE"))?
    .map_err(|_| Error::from("HTTP_COOKIE_FILTER_FAILED"))?;
    let stop = Stop(lease.as_ref().map_or_else(
        || Arc::new(AtomicBool::new(false)),
        |lease| lease.0.cancelled.clone(),
    ));
    let cancelled = stop.0.clone();
    let work = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let _lease = lease;
        native::evaluate(attributes, filter, deadline, cancelled)
    });
    tokio::time::timeout_at(deadline, work)
        .await
        .map_err(|_| Error::from("HTTP_DEADLINE"))?
        .map_err(|_| Error::from("HTTP_COOKIE_FILTER_FAILED"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cookies(names: &[&str]) -> Vec<Value> {
        names
            .iter()
            .map(|name| json!({"name":name,"domain":"example.com","path":"/acme"}))
            .collect()
    }
    #[tokio::test]
    async fn cookie_filter_cancelled_worker_keeps_context_owned_until_its_resources_stop() {
        let mut contexts = Contexts::default();
        let created = contexts.create(Uuid::new_v4(), Options::default()).unwrap();
        let reference = serde_json::from_value(created["httpContext"].clone()).unwrap();
        let lease = contexts.cookie_filter_lease(reference).unwrap();
        assert_eq!(
            contexts.remove(reference).unwrap_err().code,
            "HTTP_CONTEXT_BUSY"
        );
        assert_eq!(contexts.close().unwrap_err().code, "CLEANUP_PENDING");
        let name = format!("{}!", "a".repeat(4000));
        let filter = serde_json::from_value(json!({"name":{"regex":"^(a+)+$"}})).unwrap();
        let values = cookies(&[&name]);
        let stopped = tokio::time::timeout(
            Duration::from_millis(30),
            select_owned(
                &values,
                filter,
                Instant::now() + Duration::from_secs(5),
                lease,
            ),
        )
        .await;
        assert!(
            stopped.is_err(),
            "The caller stops before the original filter deadline"
        );
        let until = Instant::now() + Duration::from_secs(2);
        while contexts.require_idle(reference).is_err() {
            assert!(
                Instant::now() < until,
                "Cancelled evaluator must actually release its context lease"
            );
            tokio::task::yield_now().await;
        }
        contexts.remove(reference).unwrap();
        contexts.close().unwrap();
    }
    #[cfg(feature = "acceptance-browser-public")]
    #[test]
    fn pinned_cookie_filter_unicode_reference() {
        use std::process::Command;
        let cache = PathBuf::from(
            std::env::var_os("TESTRUN_BROWSER_ROOT")
                .expect("Select the pinned private browser cache"),
        );
        let output = Command::new(cache.join("node-24.12.0/node-v24.12.0-darwin-x64/bin/node"))
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/browser-cookie-regexp-unicode-baseline.cjs"
            ))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let reference: Value = serde_json::from_slice(&output.stdout).unwrap();
        // This reference workload evaluates over eight million strings, far
        // beyond a production call's 4096-cookie bound. Its harness budget is
        // separate from production operation deadlines and cancellation tests.
        let deadline = Instant::now() + Duration::from_secs(180);
        for property in reference["properties"].as_array().unwrap() {
            let pattern = property["pattern"].as_str().unwrap();
            eprintln!("Comparing Unicode property {pattern}");
            let actual =
                native::classify_codepoints(pattern, property["flags"].as_str().unwrap(), deadline)
                    .unwrap();
            let bitmap = |ranges: &Value| {
                let mut bitmap = vec![false; 0x110000];
                for range in ranges.as_array().unwrap() {
                    bitmap
                        [range[0].as_u64().unwrap() as usize..=range[1].as_u64().unwrap() as usize]
                        .fill(true);
                }
                bitmap
            };
            let expected = bitmap(&property["ranges"]);
            let first = actual
                .iter()
                .zip(expected)
                .position(|(actual, expected)| *actual != expected);
            assert!(
                first.is_none(),
                "{} differs first at {first:?}; pinned Node Unicode {}",
                property["pattern"],
                reference["unicode"]
            );
        }
    }
    #[tokio::test]
    async fn cookie_filter_ecmascript_preserves_flags_lookaround_backreferences_and_unicode_sets() {
        for (source, flags, names, expected) in [
            ("(?<=Acme)42$", "", vec!["Acme42", "Acme43"], vec![0]),
            ("^(a+)b\\1$", "", vec!["aabaa", "aaba"], vec![0]),
            (
                "^acme$",
                "gi",
                vec!["ACME", "acme", "Acme-extra"],
                vec![0, 1],
            ),
            ("acme", "y", vec!["acme-extra", "x-acme"], vec![0]),
            (
                r"^[\p{ASCII}&&\p{Letter}]+$",
                "v",
                vec!["Acme", "42", "é"],
                vec![0],
            ),
        ] {
            let filter =
                serde_json::from_value(json!({"name":{"regex":source,"flags":flags}})).unwrap();
            assert_eq!(
                select(
                    &cookies(&names),
                    filter,
                    Instant::now() + Duration::from_secs(2)
                )
                .await
                .unwrap(),
                expected
            );
        }
    }
    #[tokio::test]
    async fn cookie_filter_exact_criteria_combine_with_and_and_null_cannot_clear_everything() {
        let filter = serde_json::from_value(
            json!({"name":"Acme","domain":"example.com","path":{"regex":"^/acme$"}}),
        )
        .unwrap();
        assert_eq!(
            select(
                &cookies(&["Acme", "Other"]),
                filter,
                Instant::now() + Duration::from_secs(2)
            )
            .await
            .unwrap(),
            [0]
        );
        for value in [
            json!({"name":null}),
            json!({"domain":null}),
            json!({"path":null}),
            json!({"name":{"regex":"x","unknown":true}}),
        ] {
            assert!(serde_json::from_value::<CookieFilter>(value).is_err());
        }
        let invalid = serde_json::from_value(json!({"name":{"regex":"["}})).unwrap();
        assert_eq!(
            select(
                &cookies(&["Acme"]),
                invalid,
                Instant::now() + Duration::from_secs(2)
            )
            .await
            .unwrap_err()
            .code,
            "HTTP_COOKIE_REGEX_INVALID"
        );
    }
    #[tokio::test]
    async fn cookie_filter_catastrophic_backtracking_obeys_the_original_deadline_and_releases_capacity()
     {
        let name = format!("{}!", "a".repeat(4000));
        let filter = serde_json::from_value(json!({"name":{"regex":"^(a+)+$"}})).unwrap();
        assert_eq!(
            select(
                &cookies(&[&name]),
                filter,
                Instant::now() + Duration::from_millis(30)
            )
            .await
            .unwrap_err()
            .code,
            "HTTP_DEADLINE"
        );
        assert_eq!(
            select(
                &cookies(&["Acme"]),
                CookieFilter::default(),
                Instant::now() + Duration::from_secs(2)
            )
            .await
            .unwrap(),
            [0]
        );
    }
}

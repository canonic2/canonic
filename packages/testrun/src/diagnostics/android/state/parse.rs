//! Explicit text projections. Unknown fields and units stay unknown; no absence/causal claims.
use serde_json::{Value, json};
pub fn metric(name: &str, value: f64, unit: &str, definition: &str) -> Value {
    json!({"name":name,"value":value,"unit":unit,"definition":definition})
}
fn key<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.lines().find_map(|l| {
        l.trim()
            .strip_prefix(name)
            .and_then(|s| s.strip_prefix(':').or_else(|| s.strip_prefix('=')))
            .map(str::trim)
    })
}
fn integer(text: &str) -> Option<u64> {
    text.trim().parse().ok()
}
pub fn status_metrics(text: &str) -> Vec<Value> {
    let mut metrics = Vec::new();
    for (field, name, definition) in [
        (
            "VmRSS",
            "rss",
            "proc status resident set estimate; shared pages included; not PSS or live heap",
        ),
        (
            "RssAnon",
            "rss_anon",
            "proc status anonymous resident pages; not a live-object measurement",
        ),
        (
            "RssFile",
            "rss_file",
            "proc status file-backed resident pages",
        ),
        (
            "RssShmem",
            "rss_shared_memory",
            "proc status shared-memory resident pages",
        ),
        (
            "VmSize",
            "virtual_size",
            "proc status virtual address space; not resident memory",
        ),
        (
            "VmSwap",
            "swap",
            "proc status swapped private anonymous pages",
        ),
    ] {
        if let Some(value) = key(text, field) {
            let words: Vec<_> = value.split_whitespace().collect();
            if words.len() == 2
                && words[1] == "kB"
                && let Some(bytes) = integer(words[0]).and_then(|n| n.checked_mul(1024))
            {
                metrics.push(metric(name, bytes as f64, "bytes", definition));
            }
        }
    }
    if let Some(n) = key(text, "Threads").and_then(integer) {
        metrics.push(metric(
            "threads",
            n as f64,
            "count",
            "proc status thread count at the observed read",
        ));
    }
    metrics
}
pub fn process_fields(text: &str) -> Value {
    json!({"state":key(text,"State"),"threads":key(text,"Threads").and_then(integer),"threadStates":"separate bounded ps snapshot","heapLiveBytes":null,"allocatorRetainedBytes":null,"gcCount":null,"gcPauseMs":null,"retentionMeaning":"RSS/PSS/heap categories do not establish live ownership or leaks"})
}
pub(super) struct Counters {
    pub uptime: f64,
    user: u64,
    system: u64,
    hz: Option<u64>,
}
impl Counters {
    pub fn read(text: &str, uptime: f64, hz: Option<u64>) -> Option<Self> {
        let stat = text.lines().next()?;
        let fields: Vec<_> = stat.rsplit_once(')')?.1.split_whitespace().collect();
        Some(Self {
            uptime,
            user: fields.get(11)?.parse().ok()?,
            system: fields.get(12)?.parse().ok()?,
            hz,
        })
    }
    pub fn metrics(&self, previous: Option<&Self>) -> Vec<Value> {
        let mut out = vec![
            metric(
                "cpu_user_ticks",
                self.user as f64,
                "count",
                "proc stat utime, cumulative clock ticks for this process generation",
            ),
            metric(
                "cpu_system_ticks",
                self.system as f64,
                "count",
                "proc stat stime, cumulative clock ticks for this process generation",
            ),
        ];
        if let Some(hz) = self.hz {
            out.push(metric(
                "cpu_time",
                (self.user as f64 + self.system as f64) / hz as f64,
                "seconds",
                "cumulative process user plus kernel CPU time; excludes child processes",
            ));
            if let Some(old) = previous.filter(|old| {
                old.hz == self.hz
                    && self.uptime > old.uptime
                    && self.user >= old.user
                    && self.system >= old.system
            }) {
                let elapsed = self.uptime - old.uptime;
                let cpu =
                    ((self.user - old.user) as f64 + (self.system - old.system) as f64) / hz as f64;
                let mut delta = metric(
                    "cpu_time_delta",
                    cpu,
                    "seconds",
                    "change in cumulative process CPU time between same-generation sequential proc reads",
                );
                delta["intervalSeconds"] = json!(elapsed);
                out.push(delta);
                let mut utilization = metric(
                    "cpu_utilization",
                    100.0 * cpu / elapsed,
                    "percent",
                    "CPU seconds / boot-uptime observation interval * 100; 100% is one core, may exceed 100%; sequential-read estimate",
                );
                utilization["intervalSeconds"] = json!(elapsed);
                out.push(utilization);
            }
        }
        out
    }
}
pub fn io(text: &str) -> Value {
    let mut out = Vec::new();
    for (field, unit, meaning) in [
        (
            "rchar",
            "bytes",
            "bytes returned by read-like syscalls, including cache; cumulative",
        ),
        (
            "wchar",
            "bytes",
            "bytes supplied to write-like syscalls, including cache; cumulative",
        ),
        (
            "read_bytes",
            "bytes",
            "storage-layer bytes charged to the process; cumulative",
        ),
        (
            "write_bytes",
            "bytes",
            "storage-layer bytes charged for writing; cumulative",
        ),
        ("syscr", "count", "read-like syscall count; cumulative"),
        ("syscw", "count", "write-like syscall count; cumulative"),
    ] {
        if let Some(n) = key(text, field).and_then(integer) {
            let mut m = metric(&format!("io_{field}"), n as f64, unit, meaning);
            m["exactCounter"] = json!(n.to_string());
            out.push(m);
        }
    }
    json!({"status":if out.is_empty(){"unrecognized_format"}else{"sampled"},"metrics":out,"raw":text,"complete":false})
}
pub fn fds(text: &str) -> Value {
    let lines: Vec<_> = text.lines().filter(|s| !s.trim().is_empty()).collect();
    if lines.iter().any(|s| integer(s).is_none()) {
        return json!({"status":"unrecognized_format","metrics":[]});
    }
    json!({"status":"sampled","metrics":[metric("file_descriptors",lines.len()as f64,"count","numeric entries observed in proc fd directory; concurrent open/close can change the listing")],"complete":false})
}
pub fn threads(text: &str, pid: u32, uid: u32) -> Value {
    if !text.lines().next().is_some_and(|l| {
        l.split_whitespace()
            .eq(["PID", "TID", "UID", "STAT", "NAME"])
    }) {
        return json!({"status":"unrecognized_format","threads":[]});
    }
    let threads: Vec<_> = text
        .lines()
        .skip(1)
        .filter_map(|l| {
            let w: Vec<_> = l.split_whitespace().collect();
            if w.len() < 5
                || w[0].parse::<u32>().ok() != Some(pid)
                || w[2].parse::<u32>().ok() != Some(uid)
            {
                return None;
            }
            Some(json!({"tid":w[1].parse::<u32>().ok()?,"state":w[3],"name":w[4]}))
        })
        .take(128)
        .collect();
    json!({"status":"sampled","threads":threads,"limit":128,"complete":false})
}
pub fn memory(text: &str, pid: u32) -> Value {
    let header = format!("** MEMINFO in pid {pid} ");
    if !text.contains(&header) {
        return json!({"status":"unrecognized_format","metrics":[],"raw":text});
    }
    let mut out = Vec::new();
    // App Summary has explicit PSS labels. Do not guess column positions in OEM heap tables.
    if let Some((_, summary)) = text.split_once("App Summary") {
        for (label, name) in [
            ("Java Heap", "pss_java_heap"),
            ("Native Heap", "pss_native_heap"),
            ("Code", "pss_code"),
            ("Stack", "pss_stack"),
            ("Graphics", "pss_graphics"),
            ("Private Other", "pss_private_other"),
            ("System", "pss_system"),
        ] {
            if let Some(bytes) = key(summary, label)
                .and_then(|s| {
                    if summary.contains("Pss(KB)") {
                        s.split_whitespace().next()
                    } else {
                        Some(s)
                    }
                })
                .and_then(integer)
                .and_then(|n| n.checked_mul(1024))
            {
                out.push(metric(name,bytes as f64,"bytes","dumpsys meminfo App Summary category PSS in KiB converted to bytes; not live allocations or retained ownership"));
            }
        }
        for (label, name, meaning) in [
            (
                "TOTAL PSS:",
                "pss",
                "proportional set size from dumpsys meminfo, KiB converted to bytes",
            ),
            (
                "TOTAL RSS:",
                "meminfo_rss",
                "RSS from dumpsys meminfo, KiB converted to bytes",
            ),
            (
                "TOTAL SWAP PSS:",
                "swap_pss",
                "proportional swapped memory from dumpsys meminfo, KiB converted to bytes",
            ),
        ] {
            if let Some(n) = summary
                .split_once(label)
                .and_then(|(_, s)| s.split_whitespace().next())
                .and_then(integer)
                .and_then(|n| n.checked_mul(1024))
            {
                out.push(metric(name, n as f64, "bytes", meaning));
            }
        }
    }
    json!({"status":if out.is_empty(){"unrecognized_format"}else{"sampled"},"metrics":out,"raw":text,"heapAllocation":"unknown; no heap dump or forced GC","complete":false})
}
pub fn frames(text: &str, pid: u32) -> Value {
    if !text.contains(&format!("** Graphics info for pid {pid} ")) {
        return json!({"status":"unrecognized_format","frames":[]});
    }
    let mut header = None;
    let mut out = Vec::new();
    let mut excluded = 0usize;
    for line in text.lines() {
        if line == "---PROFILEDATA---" {
            header = None;
            continue;
        }
        let columns: Vec<_> = line.trim().trim_end_matches(',').split(',').collect();
        if columns.contains(&"IntendedVsync") && columns.contains(&"FrameCompleted") {
            header = Some(
                columns
                    .iter()
                    .position(|s| *s == "Flags")
                    .zip(columns.iter().position(|s| *s == "IntendedVsync"))
                    .zip(columns.iter().position(|s| *s == "FrameCompleted")),
            );
            continue;
        }
        if let Some(Some(((flag, start), end))) = header {
            let values = columns
                .get(flag)
                .and_then(|s| integer(s))
                .zip(columns.get(start).and_then(|s| integer(s)))
                .zip(columns.get(end).and_then(|s| integer(s)));
            if let Some(((0, start), end)) = values
                .filter(|((_, start), end)| *start > 0 && *end >= *start && *end < i64::MAX as u64)
            {
                if out.len() < 256 {
                    out.push(json!({"intendedVsyncNs":start.to_string(),"frameCompletedNs":end.to_string(),"durationMs":(end-start)as f64/1e6,"clock":"android_monotonic_ns","source":"gfxinfo PROFILEDATA"}));
                } else {
                    excluded += 1;
                }
            } else {
                excluded += 1;
            }
        }
    }
    json!({"status":if header.is_none() && out.is_empty(){"unrecognized_format"}else{"sampled"},"frames":out,"excluded":excluded,"limit":256,"complete":false,"meaning":"recent HWUI frame history; not reset, not every rendered frame or frame deadline/jank classification"})
}
fn bool_value(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}
pub fn context(group: &str, text: &str, app: &str, user: u32, uid: u32) -> Value {
    let mut fields = json!({});
    let mut metrics = Vec::new();
    let mut retained = Vec::new();
    let mut scope = "device";
    match group {
        "standby" => {
            fields["bucketCode"] = json!(integer(text));
            scope = "app_user";
        }
        "manufacturer" => {
            let value = text.trim();
            if !value.is_empty() && value.len() <= 128 {
                fields["manufacturer"] = json!(value);
            }
            fields["oemRestrictionPolicy"] = Value::Null;
        }
        "background" => {
            scope = "app_user";
            fields["operation"] = json!("RUN_ANY_IN_BACKGROUND");
            fields["mode"] = text
                .split("RUN_ANY_IN_BACKGROUND:")
                .nth(1)
                .and_then(|s| s.split([';', ',', '\n']).next())
                .map(str::trim)
                .filter(|s| matches!(*s, "allow" | "ignore" | "deny" | "default" | "foreground"))
                .map_or(Value::Null, |s| json!(s));
        }
        "battery" => {
            battery_fields(text, &mut fields, &mut metrics);
        }
        "idle" | "power" => {
            power_fields(group, text, &mut fields);
            if group == "power" {
                retained = app_lines(text, app, user, uid);
                fields["wakeLocks"] = json!(
                    retained
                        .iter()
                        .filter(|s| s.contains("WAKE_LOCK") || s.contains("PARTIAL_WAKE_LOCK"))
                        .collect::<Vec<_>>()
                );
            }
        }
        "thermal" => {
            fields["statusCode"] = json!(
                key(text, "Thermal Status")
                    .or_else(|| key(text, "mStatus"))
                    .and_then(integer)
            );
            fields["causeEstablished"] = json!(false);
        }
        "audio" | "media_sessions" => {
            // Only lines with exact app AND selected user/UID witnesses survive.
            // Do not associate following unscoped lines with a preceding session.
            scope = "app_user";
            retained = app_lines(text, app, user, uid);
            fields["records"] = json!(retained);
            fields["absenceProven"] = json!(false);
        }
        "locale" | "timezone" => {
            let value = text.trim();
            if !value.is_empty()
                && value != "null"
                && value.len() <= 256
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/_-+.@=".contains(&b))
            {
                fields[group] = json!(value);
            }
            fields["meaning"] =
                json!("device property readback; per-app locale/timezone behavior may differ");
        }
        "font_scale" => {
            fields["fontScale"] = json!(
                text.trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite() && *v > 0.0 && *v <= 10.0)
            );
        }
        "user_rotation" | "accelerometer_rotation" => {
            let value =
                integer(text.trim()).filter(|v| *v <= if group == "user_rotation" { 3 } else { 1 });
            fields[group] = json!(value);
        }
        "jobs" | "alarms" | "services" => {
            scope = "app_user";
            retained = app_lines(text, app, user, uid);
            fields["records"] = if group == "services" {
                json!(service_records(text, app, user, uid))
            } else {
                json!(retained)
            };
            fields["absenceProven"] = json!(false);
        }
        _ => {}
    }
    let recognized = context_recognized(&fields, &metrics);
    json!({"group":group,"status":if recognized{"sampled"}else{"unrecognized_format"},"scope":scope,"fields":fields,"metrics":metrics,"retainedLines":retained,"complete":false,"causeEstablished":false})
}
fn app_lines(text: &str, app: &str, user: u32, uid: u32) -> Vec<String> {
    let user_token = format!("u{user}");
    let uid_token = if uid % 100000 >= 10000 {
        format!("u{user}a{}", uid % 100000 - 10000)
    } else {
        format!("u{user}s{}", uid % 100000)
    };
    text.lines()
        .filter(|l| super::super::parse::package_token(l, app))
        .filter(|l| {
            l.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .any(|w| w == user_token || w == uid_token)
                || l.split_whitespace().any(|w| {
                    let w = w.trim_matches(['(', ')', ',']);
                    w.strip_prefix("uid=")
                        .or_else(|| w.strip_prefix("uid:"))
                        .and_then(integer)
                        == Some(uid as u64)
                })
        })
        .take(64)
        .map(|l| l.chars().take(512).collect())
        .collect()
}

fn service_records(text: &str, app: &str, user: u32, uid: u32) -> Vec<Value> {
    let mut records = Vec::new();
    let mut current = None;
    let mut lines = 0;
    for line in text.lines() {
        if line.contains("ServiceRecord{") {
            current = None;
            lines = 0;
            if records.len() < 64 && !app_lines(line, app, user, uid).is_empty() {
                records.push(json!({"header":line.chars().take(512).collect::<String>(),"isForeground":null,"startRequested":null,"foregroundId":null}));
                current = Some(records.len() - 1);
            }
        }
        if let Some(index) = current {
            lines += 1;
            if lines > 128 {
                current = None;
                continue;
            }
            for word in line.split_whitespace() {
                for name in ["isForeground", "startRequested"] {
                    if let Some(value) = word.strip_prefix(&format!("{name}=")) {
                        records[index][name] = json!(bool_value(value.trim_end_matches(',')));
                    }
                }
                if let Some(value) = word.strip_prefix("foregroundId=") {
                    records[index]["foregroundId"] = json!(integer(value));
                }
            }
        }
    }
    records
}

fn battery_fields(text: &str, fields: &mut Value, metrics: &mut Vec<Value>) {
    for label in ["AC powered", "USB powered", "Wireless powered", "present"] {
        fields[label] = json!(key(text, label).and_then(bool_value));
    }
    for label in ["status", "health", "level", "scale"] {
        fields[label] = json!(key(text, label).and_then(integer));
    }
    if let Some(t) = key(text, "temperature")
        .and_then(|s| s.parse::<i32>().ok())
        .filter(|n| (-1000..=2000).contains(n))
    {
        metrics.push(metric(
            "battery_temperature",
            t as f64 / 10.0,
            "celsius",
            "battery service temperature in tenths Celsius; not CPU temperature",
        ));
    }
}

fn power_fields(group: &str, text: &str, fields: &mut Value) {
    for name in if group == "idle" {
        vec![
            "mState",
            "mLightState",
            "mForceIdle",
            "mScreenOn",
            "mCharging",
        ]
    } else {
        vec![
            "mWakefulness",
            "mIsPowered",
            "mWakeLockSummary",
            "mDeviceIdleMode",
            "mLightDeviceIdleMode",
        ]
    } {
        // dumps contain several fields per line; retain only the selected field token.
        fields[name] = text
            .split_whitespace()
            .find_map(|w| w.strip_prefix(&format!("{name}=")))
            .map(|v| v.trim_end_matches(','))
            .map_or(Value::Null, |s| {
                if matches!(
                    name,
                    "mState" | "mLightState" | "mWakefulness" | "mWakeLockSummary"
                ) {
                    json!(s)
                } else {
                    json!(bool_value(s))
                }
            });
    }
}

fn context_recognized(fields: &Value, metrics: &[Value]) -> bool {
    fields.as_object().unwrap().iter().any(|(key, v)| {
        !v.is_null()
            && !matches!(
                key.as_str(),
                "causeEstablished"
                    | "absenceProven"
                    | "oemRestrictionPolicy"
                    | "operation"
                    | "meaning"
            )
            && (!v.is_array() || !v.as_array().unwrap().is_empty())
    }) || !metrics.is_empty()
}

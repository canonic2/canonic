//! Pure Android text readers. Unknown formats/identities never become empty success.
use crate::error::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Process {
    pub pid: u32,
    pub uid: u32,
    pub name: String,
    pub start_ticks: String,
    pub generation: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Log {
    pub time: String,
    pub uid: Option<u32>,
    pub pid: u32,
    pub tid: u32,
    pub priority: String,
    pub tag: String,
    pub message: String,
}
pub(crate) fn package(value: &str) -> bool {
    value.len() <= 255
        && value.contains('.')
        && value.split('.').all(|part| {
            !part.is_empty()
                && part.bytes().enumerate().all(|(i, b)| {
                    b.is_ascii_alphabetic() || b == b'_' || (i > 0 && b.is_ascii_digit())
                })
        })
}
pub(crate) fn package_token(text: &str, app: &str) -> bool {
    text.match_indices(app).any(|(i, _)| {
        let identifier = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'.';
        (i == 0 || !identifier(text.as_bytes()[i - 1]))
            && (i + app.len() == text.len() || !identifier(text.as_bytes()[i + app.len()]))
    })
}
pub(crate) fn uid_inventory(text: &str, app: &str) -> Result<(u32, usize)> {
    let entries: Vec<_> = text
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let name = words.next()?.strip_prefix("package:")?;
            let uid = words.find_map(|word| word.strip_prefix("uid:")?.parse::<u32>().ok())?;
            Some((name, uid))
        })
        .collect();
    let uid = entries
        .iter()
        .find(|(name, _)| *name == app)
        .map(|(_, uid)| *uid)
        .ok_or_else(|| {
            Error::new(
                "ANDROID_APP_UNAVAILABLE",
                "Selected package/UID was not observed for this Android user",
            )
        })?;
    Ok((uid, entries.iter().filter(|(_, n)| *n == uid).count()))
}
pub(crate) fn installed(text: &str, paths: &str) -> Value {
    let field = |key: &str| {
        text.lines().find_map(|line| {
            line.trim().strip_prefix(key).map(|value| {
                value
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_string()
            })
        })
    };
    let flag = |group: &str, value: &str| {
        text.lines().find_map(|line| {
            line.trim()
                .strip_prefix(group)
                .filter(|rest| rest.contains('[') && rest.contains(']'))
                .map(|rest| {
                    rest.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                        .any(|word| word == value)
                })
        })
    };
    let splits: Vec<_> = paths
        .lines()
        .filter_map(|line| line.trim().strip_prefix("package:"))
        .take(64)
        .map(str::to_string)
        .collect();
    let profileable = if text.contains("isProfileableByShell=true") {
        Some(true)
    } else if text.contains("isProfileableByShell=false") {
        Some(false)
    } else {
        flag("privateFlags=", "PROFILEABLE_BY_SHELL")
            .or_else(|| flag("privatePkgFlags=", "PROFILEABLE_BY_SHELL"))
    };
    json!({"versionCode":field("versionCode="),"versionName":text.lines().find_map(|l|l.trim().strip_prefix("versionName=")).map(str::to_string),"lastUpdateTime":text.lines().find_map(|l|l.trim().strip_prefix("lastUpdateTime=")).map(str::to_string),"debuggable":flag("flags=","DEBUGGABLE").or_else(||flag("pkgFlags=","DEBUGGABLE")),"profileableByShell":profileable,"apkPaths":splits,"nativeLibraryDir":field("nativeLibraryDir="),"symbols":"unknown_until_library_inspection"})
}

/// Retain a bounded privileged DEBUG block only with both an exact package and UID header.
/// Split/past headers are deliberately not inferred across logcat snapshots.
pub(crate) fn tombstone_context(
    logs: &[Log],
    app: &str,
    uid: u32,
) -> std::collections::HashSet<usize> {
    let mut retained = std::collections::HashSet::new();
    let mut start = 0;
    while start < logs.len() {
        let first = &logs[start];
        if first.tag != "DEBUG"
            || first.uid.is_none_or(|v| v >= 10000)
            || !(first.message.contains("*** *** ***") || first.message.starts_with("pid:"))
        {
            start += 1;
            continue;
        }
        let mut end = start + 1;
        while end < logs.len() && end - start < 256 {
            let next = &logs[end];
            if (next.uid, next.pid, next.tid, next.tag.as_str())
                != (first.uid, first.pid, first.tid, first.tag.as_str())
                || next.message.contains("*** *** ***")
            {
                break;
            }
            end += 1;
        }
        let headers: Vec<_> = logs[start..end]
            .iter()
            .filter_map(|l| {
                l.message
                    .split_once(">>> ")
                    .and_then(|(_, s)| s.split_once(" <<<"))
                    .map(|(s, _)| s)
                    .or_else(|| l.message.strip_prefix("Cmdline:").map(str::trim))
            })
            .collect();
        let package = !headers.is_empty()
            && headers
                .iter()
                .all(|s| *s == app || s.strip_prefix(app).is_some_and(|s| s.starts_with(':')));
        let identities: Vec<_> = logs[start..end]
            .iter()
            .filter_map(|l| l.message.strip_prefix("uid:"))
            .collect();
        let identity = !identities.is_empty()
            && identities
                .iter()
                .all(|s| s.trim().parse::<u32>().ok() == Some(uid));
        if package && identity {
            retained.extend(start..end);
        }
        start = end;
    }
    retained
}
pub(crate) fn processes(
    text: &str,
    uid: u32,
    app: &str,
    shared: bool,
) -> Result<Vec<(u32, String)>> {
    if !text
        .lines()
        .next()
        .is_some_and(|line| line.contains("PID") && line.contains("UID"))
    {
        return Err(Error::new(
            "ANDROID_PROCESS_FORMAT",
            "Android ps did not return numeric PID/UID/name columns",
        ));
    }
    let mut output = Vec::new();
    for line in text.lines().skip(1) {
        let words: Vec<_> = line.split_whitespace().collect();
        if words.len() < 3 {
            continue;
        }
        let (Ok(pid), Ok(real_uid)) = (words[0].parse::<u32>(), words[1].parse::<u32>()) else {
            continue;
        };
        let name = words[2];
        if real_uid == uid && (!shared || name == app || name.starts_with(&format!("{app}:"))) {
            output.push((pid, name.into()));
        }
    }
    if output.len() > 32 {
        return Err(Error::new(
            "ANDROID_PROCESS_LIMIT",
            "Selected app exceeds 32 inspectable processes",
        ));
    }
    Ok(output)
}
pub(crate) fn process(bytes: &[u8], expected_pid: u32, uid: u32, boot: &str) -> Option<Process> {
    let text = std::str::from_utf8(bytes).ok()?;
    let first = text.lines().next()?;
    let pid = first.split_whitespace().next()?.parse().ok()?;
    if pid != expected_pid {
        return None;
    }
    let stat = first
        .get(first.rfind(')')? + 1..)?
        .split_whitespace()
        .collect::<Vec<_>>();
    let ticks = *stat.get(19)?;
    ticks.parse::<u64>().ok()?;
    let observed = text.lines().find_map(|line| {
        line.strip_prefix("Uid:")?
            .split_whitespace()
            .next()?
            .parse::<u32>()
            .ok()
    })?;
    if observed != uid {
        return None;
    }
    let prefix = text.split('\0').next()?;
    let name = prefix.rsplit('\n').next()?.trim();
    if name.is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
        return None;
    }
    Some(Process {
        pid,
        uid,
        name: name.into(),
        start_ticks: ticks.into(),
        generation: format!("{boot}:{pid}:{ticks}"),
    })
}
pub(crate) fn log(line: &str) -> Option<Log> {
    let (prefix, message) = line.split_once(": ")?;
    let words: Vec<_> = prefix.split_whitespace().collect();
    if words.len() < 5 {
        return None;
    }
    let time = words[0];
    let (seconds, fraction) = time.split_once('.')?;
    if seconds.parse::<u64>().is_err()
        || fraction.is_empty()
        || fraction.len() > 9
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let level = words
        .iter()
        .enumerate()
        .skip(3)
        .find(|(_, w)| w.len() == 1 && "VDIWEFS".contains(**w))?
        .0;
    let (uid, pid, tid) = match level {
        3 => (None, words[1].parse().ok()?, words[2].parse().ok()?),
        4 => (
            Some(words[1].parse().ok()?),
            words[2].parse().ok()?,
            words[3].parse().ok()?,
        ),
        _ => return None,
    };
    let tag = words.get(level + 1..)?.join(" ");
    if tag.len() > 128 || message.len() > 16384 {
        return None;
    }
    Some(Log {
        time: time.into(),
        uid,
        pid,
        tid,
        priority: words[level].into(),
        tag,
        message: message.into(),
    })
}
pub(crate) fn time_key(time: &str) -> Option<(u64, u32)> {
    let (a, b) = time.split_once('.')?;
    if b.is_empty() || b.len() > 9 || !b.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut fraction = b.to_string();
    while fraction.len() < 9 {
        fraction.push('0');
    }
    Some((a.parse().ok()?, fraction.parse().ok()?))
}
#[derive(Default)]
pub(crate) struct Cursor {
    pub time: Option<String>,
    boundary: HashMap<String, usize>,
}
impl Cursor {
    pub fn take(&mut self, logs: Vec<Log>) -> (Vec<Log>, bool) {
        let old = self.time.as_deref().and_then(time_key);
        let mut seen = HashMap::new();
        let mut output = Vec::new();
        let mut maximum = old;
        let mut boundary = HashMap::new();
        let mut regressed = false;
        for entry in logs {
            let Some(key) = time_key(&entry.time) else {
                continue;
            };
            let identity = serde_json::to_string(&entry).unwrap();
            let n = seen.entry(identity.clone()).or_insert(0usize);
            *n += 1;
            if Some(key) < old {
                regressed = true;
                continue;
            }
            if Some(key) == old && *n <= self.boundary.get(&identity).copied().unwrap_or(0) {
                continue;
            }
            if maximum.is_none_or(|max| key > max) {
                maximum = Some(key);
                boundary.clear();
            }
            if Some(key) == maximum {
                *boundary.entry(identity).or_insert(0) += 1;
            }
            output.push(entry);
        }
        if let Some(max) = maximum {
            if Some(max) == old {
                for (key, count) in &self.boundary {
                    *boundary.entry(key.clone()).or_insert(0) += *count;
                }
            }
            self.time = Some(format!("{}.{:09}", max.0, max.1));
            self.boundary = boundary;
        }
        (output, regressed)
    }
}
pub(crate) fn exit_entries(text: &str, uid: u32) -> Vec<Value> {
    let field = |block: &str, key: &str| {
        block
            .split_whitespace()
            .find_map(|word| word.strip_prefix(key).map(str::to_string))
    };
    text.split("ApplicationExitInfo ").skip(1).filter_map(|block|{
        let package_uid=field(block,"packageUid=").or_else(||field(block,"realUid="))?.parse::<u32>().ok()?;
        if package_uid!=uid{return None;}
        let pid=field(block,"pid=")?.parse::<u32>().ok()?;let reason=field(block,"reason=")?.parse::<u32>().ok()?;
        let class=match reason{2=>"signal_exit",3=>"low_memory_kill",4=>"java_crash",5=>"native_crash",6=>"anr",9=>"permission_change_exit",10=>"user_requested_exit",11=>"user_stopped_exit",12=>"dependency_exit",13=>"other_exit",14=>"freezer_exit",15=>"package_state_change",16=>"package_updated",_=>"unknown_exit"};
        Some(json!({"pid":pid,"uid":package_uid,"reasonCode":reason,"classification":class,"process":field(block,"process="),"timestampText":block.lines().find_map(|line|line.split_once("timestamp=").map(|(_,v)|v.split(" pid=").next().unwrap_or(v).trim())),"status":field(block,"status="),"tracePath":field(block,"trace=").filter(|p|p!="null"),"raw":block.chars().take(16384).collect::<String>(),"processGeneration":null,"causeEstablished":false}))
    }).take(128).collect()
}
pub(crate) fn safe_report(path: &str) -> bool {
    path.len() < 512
        && !path.contains("..")
        && (path.starts_with("/data/anr/anr_") || path.starts_with("/data/tombstones/tombstone_"))
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_-.".contains(&b))
}

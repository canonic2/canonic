//! Versioned native Chrome snapshots (`testrun-chrome-storage-v1`, and `-v2`
//! when they carry virtual credentials), their validated conversion from
//! pinned Playwright storage state, and the page scripts that replace or
//! merge one origin's storage.
use super::Options;
use serde_json::{Map, Value, json};

pub(crate) const FORMAT: &str = "testrun-chrome-storage-v1";
/// `FORMAT` plus `included.credentials` and the pinned `VirtualCredential`
/// array. Snapshots without credentials keep writing `FORMAT`.
pub(crate) const CREDENTIALS_FORMAT: &str = "testrun-chrome-storage-v2";
const MAX_BUILD_BYTES: usize = 128;

/// The format a valid native snapshot declares.
pub(crate) fn format(state: &Value) -> &'static str {
    if state["format"] == CREDENTIALS_FORMAT {
        CREDENTIALS_FORMAT
    } else {
        FORMAT
    }
}

fn object(value: &Value) -> Result<&Map<String, Value>, &'static str> {
    value.as_object().ok_or("INVALID_STORAGE_STATE")
}
fn exact_keys(value: &Map<String, Value>, required: &[&str], optional: &[&str]) -> bool {
    required.iter().all(|key| value.contains_key(*key))
        && value
            .keys()
            .all(|key| required.contains(&key.as_str()) || optional.contains(&key.as_str()))
}

/// The snapshot of collected pinned-shape state (`partitionKey` site plus
/// `_crHasCrossSiteAncestor`), with explicit native partition keys.
pub(crate) fn snapshot(
    state: &Value,
    options: &Options,
    build: &str,
) -> Result<Value, &'static str> {
    let cookies = state["cookies"]
        .as_array()
        .ok_or("INVALID_STORAGE_STATE")?
        .iter()
        .map(|cookie| {
            let mut cookie = object(cookie)?.clone();
            let ancestor = cookie.remove("_crHasCrossSiteAncestor");
            if let Some(site) = cookie.remove("partitionKey") {
                let mut key = json!({"topLevelSite":site});
                if let Some(ancestor) = ancestor {
                    key["hasCrossSiteAncestor"] = ancestor;
                }
                cookie.insert("partitionKey".into(), key);
            }
            Ok(Value::Object(cookie))
        })
        .collect::<Result<Vec<_>, &'static str>>()?;
    let mut snapshot = json!({"format":FORMAT,"backend":{"engine":"chrome","build":build},
        "included":{"cookies":true,"localStorage":true,"indexedDB":options.indexed_db,"opfs":options.opfs},
        "cookies":cookies,"origins":state["origins"].clone()});
    if options.credentials {
        snapshot["format"] = json!(CREDENTIALS_FORMAT);
        snapshot["included"]["credentials"] = json!(true);
        snapshot["credentials"] = state
            .get("credentials")
            .cloned()
            .unwrap_or_else(|| json!([]));
    }
    Ok(snapshot)
}

/// The pinned storage-state projection the restore script and cookie writer use.
pub(crate) fn playwright(state: &Value) -> Result<Value, &'static str> {
    let cookies = state["cookies"]
        .as_array()
        .ok_or("INVALID_STORAGE_STATE")?
        .iter()
        .map(|cookie| {
            let mut cookie = object(cookie)?.clone();
            if let Some(key) = cookie.remove("partitionKey") {
                let key = object(&key)?;
                if !exact_keys(key, &["topLevelSite"], &["hasCrossSiteAncestor"]) {
                    return Err("INVALID_COOKIE_SCOPE");
                }
                cookie.insert("partitionKey".into(), key["topLevelSite"].clone());
                if let Some(ancestor) = key.get("hasCrossSiteAncestor") {
                    cookie.insert("_crHasCrossSiteAncestor".into(), ancestor.clone());
                }
            }
            Ok(Value::Object(cookie))
        })
        .collect::<Result<Vec<_>, &'static str>>()?;
    let mut projection = json!({"cookies":cookies,"origins":state["origins"].clone()});
    if let Some(credentials) = state.get("credentials") {
        projection["credentials"] = credentials.clone();
    }
    Ok(projection)
}

/// Complete validation of a native snapshot: envelope, declared types against
/// each origin's content, then the pinned schema on its projection.
pub(crate) fn validate(state: &Value) -> Result<(), &'static str> {
    let envelope = object(state)?;
    let credentials = match state["format"].as_str() {
        Some(FORMAT) => false,
        Some(CREDENTIALS_FORMAT) => true,
        _ => return Err("STORAGE_FORMAT_UNSUPPORTED"),
    };
    let mut keys = vec!["format", "backend", "included", "cookies", "origins"];
    let mut types = vec!["cookies", "localStorage", "indexedDB", "opfs"];
    if credentials {
        keys.push("credentials");
        types.push("credentials");
    }
    if !exact_keys(envelope, &keys, &[]) {
        return Err("INVALID_STORAGE_STATE");
    }
    let backend = object(&state["backend"])?;
    if !exact_keys(backend, &["engine", "build"], &[])
        || state["backend"]["engine"] != "chrome"
        || !state["backend"]["build"]
            .as_str()
            .is_some_and(|build| !build.is_empty() && build.len() <= MAX_BUILD_BYTES)
    {
        return Err("STORAGE_BACKEND_MISMATCH");
    }
    let included = object(&state["included"])?;
    if !exact_keys(included, &types, &[])
        || state["included"]["cookies"] != true
        || state["included"]["localStorage"] != true
        || credentials && state["included"]["credentials"] != true
        || !included.values().all(Value::is_boolean)
    {
        return Err("INVALID_STORAGE_STATE");
    }
    for origin in state["origins"].as_array().ok_or("INVALID_STORAGE_STATE")? {
        for (key, flag) in [("indexedDB", "indexedDB"), ("opfs", "opfs")] {
            if origin.get(key).is_some() && state["included"][flag] != true {
                return Err("STORAGE_TYPE_NOT_INCLUDED");
            }
        }
    }
    super::validate_with(&playwright(state)?, true)
}

/// The validated one-to-one conversion of a pinned Playwright state file:
/// cookies, local storage and any IndexedDB/OPFS its origins carry.
pub(crate) fn convert(state: &Value, build: &str) -> Result<Value, &'static str> {
    super::validate_with(state, true)?;
    let present = |key: &str| {
        state["origins"]
            .as_array()
            .is_some_and(|origins| origins.iter().any(|origin| origin.get(key).is_some()))
    };
    let options = Options {
        indexed_db: present("indexedDB"),
        opfs: present("opfs"),
        credentials: state.get("credentials").is_some(),
        path: None,
    };
    let converted = snapshot(state, &options, build)?;
    validate(&converted)?;
    Ok(converted)
}

/// A registered file is native when it declares this format; anything else
/// must be pinned Playwright state and is converted.
pub(crate) fn register(state: Value, build: &str) -> Result<(Value, bool), &'static str> {
    if state.get("format").is_some() {
        validate(&state)?;
        Ok((state, false))
    } else {
        Ok((convert(&state, build)?, true))
    }
}

fn origin_state(state: &Value, origin: &str) -> Value {
    state["origins"]
        .as_array()
        .and_then(|origins| origins.iter().find(|item| item["origin"] == origin))
        .cloned()
        .unwrap_or(Value::Null)
}
fn wrap(origin: &str, body: &str) -> String {
    format!(
        "(async()=>{{if(location.origin!=={origin})throw Error('Storage origin changed');const module={{exports:{{}}}};\n{source}\nconst script=new (module.exports.StorageScript())('chromium');{body}if(location.origin!=={origin})throw Error('Storage origin changed');return true;}})()",
        origin = json!(origin),
        source = include_str!("../reading/storage.js"),
    )
}
/// The pinned `restore` for one origin: its storage is cleared, then the
/// snapshot's state for that origin (if any) is written.
pub(crate) fn restore_expression(state: &Value, origin: &str) -> String {
    let origin_state = origin_state(state, origin);
    let argument = if origin_state.is_null() {
        "undefined".to_owned()
    } else {
        origin_state.to_string()
    };
    wrap(origin, &format!("await script.restore({argument});"))
}
/// Merge one origin: local-storage keys upsert, each snapshot database is
/// replaced by name, and OPFS paths are overwritten; nothing else is cleared.
///
/// A blocked `deleteDatabase` stays queued and deletes the database once the
/// other page closes it, so each database is first claimed by a version
/// upgrade. A blocked upgrade aborts when it eventually runs, leaving the
/// database unchanged; only a claimed database is deleted and restored.
pub(crate) fn merge_expression(state: &Value, origin: &str) -> String {
    let body = format!(
        "const state={};for(const {{name,value}} of state.localStorage||[])localStorage.setItem(name,value);\
const opened=(request)=>new Promise((resolve,reject)=>{{request.onsuccess=()=>resolve(request.result);request.onerror=()=>reject(request.error);}});\
const claim=async(name)=>{{const current=await opened(indexedDB.open(name));const version=current.version;current.close();\
return new Promise((resolve,reject)=>{{const request=indexedDB.open(name,version+1);let blocked=false;\
request.onblocked=()=>{{blocked=true;reject(new Error('IndexedDB database '+name+' is open in another page'));}};\
request.onupgradeneeded=()=>{{if(blocked)request.transaction.abort();}};\
request.onerror=()=>{{if(!blocked)reject(request.error);}};request.onsuccess=()=>resolve(request.result);}});}};\
for(const db of state.indexedDB||[]){{const claimed=await claim(db.name);await new Promise((resolve,reject)=>{{claimed.close();const request=indexedDB.deleteDatabase(db.name);request.onsuccess=resolve;request.onerror=()=>reject(request.error);request.onblocked=()=>reject(new Error('IndexedDB database '+db.name+' was opened in another page during the merge; its deletion may still complete'));}});await script._restoreDB(db);}}\
if(state.opfs&&state.opfs.length){{const root=await navigator.storage.getDirectory();for(const entry of state.opfs){{const parts=entry.path.split('/');let directory=root;for(const part of parts.slice(0,-1))directory=await directory.getDirectoryHandle(part,{{create:true}});const name=parts[parts.length-1];if(entry.type==='directory'){{await directory.getDirectoryHandle(name,{{create:true}});continue;}}const blob=await script._blobFromBase64(entry.base64);const handle=await directory.getFileHandle(name,{{create:true}});const writable=await handle.createWritable();await writable.write(blob);await writable.close();}}}}",
        origin_state(state, origin)
    );
    wrap(origin, &body)
}

/// Snapshot origins in their recorded order.
pub(crate) fn origins(state: &Value) -> Vec<String> {
    state["origins"]
        .as_array()
        .map(|origins| {
            origins
                .iter()
                .filter_map(|origin| origin["origin"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collected() -> Value {
        json!({"cookies":[
            {"name":"acme","value":"private","domain":"example.com","path":"/","expires":-1,"httpOnly":true,"secure":true,"sameSite":"Lax"},
            {"name":"part","value":"private","domain":"example.com","path":"/","expires":-1,"httpOnly":false,"secure":true,"sameSite":"None","partitionKey":"https://example.org","_crHasCrossSiteAncestor":false}],
            "origins":[{"origin":"https://example.com","localStorage":[{"name":"k","value":"v"}]}]})
    }

    #[test]
    fn native_snapshots_carry_explicit_partitions_and_round_trip_to_the_pinned_shape() {
        let native = snapshot(&collected(), &Options::default(), "156.0.8078.47").unwrap();
        assert_eq!(native["format"], FORMAT);
        assert_eq!(
            native["backend"],
            json!({"engine":"chrome","build":"156.0.8078.47"})
        );
        assert_eq!(
            native["cookies"][1]["partitionKey"],
            json!({"topLevelSite":"https://example.org","hasCrossSiteAncestor":false})
        );
        assert!(
            native["cookies"][1]
                .get("_crHasCrossSiteAncestor")
                .is_none()
        );
        validate(&native).unwrap();
        assert_eq!(playwright(&native).unwrap(), collected());
    }

    #[test]
    fn native_snapshot_validation_refuses_foreign_formats_backends_and_undeclared_types() {
        let native = snapshot(&collected(), &Options::default(), "156").unwrap();
        let mut foreign = native.clone();
        foreign["format"] = json!("testrun-chrome-storage-v3");
        assert_eq!(validate(&foreign), Err("STORAGE_FORMAT_UNSUPPORTED"));
        let mut relabeled = native.clone();
        relabeled["format"] = json!(CREDENTIALS_FORMAT);
        assert_eq!(validate(&relabeled), Err("INVALID_STORAGE_STATE"));
        let mut firefox = native.clone();
        firefox["backend"]["engine"] = json!("firefox");
        assert_eq!(validate(&firefox), Err("STORAGE_BACKEND_MISMATCH"));
        let mut undeclared = native.clone();
        undeclared["origins"][0]["indexedDB"] = json!([]);
        assert_eq!(validate(&undeclared), Err("STORAGE_TYPE_NOT_INCLUDED"));
        let mut extra = native;
        extra["credentials"] = json!([]);
        assert_eq!(validate(&extra), Err("INVALID_STORAGE_STATE"));
    }

    #[test]
    fn playwright_files_convert_with_their_present_types_and_native_files_register_unchanged() {
        let mut state = collected();
        state["origins"][0]["opfs"] = json!([{"type":"directory","path":"acme"}]);
        let (converted, was_converted) = register(state, "156").unwrap();
        assert!(was_converted);
        assert_eq!(converted["included"]["opfs"], true);
        assert_eq!(converted["included"]["indexedDB"], false);
        let (same, was_converted) = register(converted.clone(), "156").unwrap();
        assert!(!was_converted);
        assert_eq!(same, converted);
        let mut credentials = collected();
        credentials["credentials"] = json!([{"id":"YQ","rpId":"example.com"}]);
        assert_eq!(register(credentials, "156"), Err("INVALID_STORAGE_STATE"));
    }

    #[test]
    fn credential_snapshots_use_v2_while_others_keep_v1_and_round_trip_pinned_credentials() {
        let credential =
            crate::browser::credentials::Credential::generate("example.com", None, None, 3)
                .unwrap();
        let mut state = collected();
        state["credentials"] = json!([credential]);
        let options = Options {
            credentials: true,
            ..Default::default()
        };
        let native = snapshot(&state, &options, "156").unwrap();
        assert_eq!(native["format"], CREDENTIALS_FORMAT);
        assert_eq!(native["included"]["credentials"], true);
        validate(&native).unwrap();
        assert_eq!(format(&native), CREDENTIALS_FORMAT);
        assert_eq!(
            playwright(&native).unwrap()["credentials"],
            json!([credential])
        );
        let mut undeclared = native.clone();
        undeclared["included"]["credentials"] = json!(false);
        assert_eq!(validate(&undeclared), Err("INVALID_STORAGE_STATE"));
        let mut duplicate = native.clone();
        duplicate["credentials"] = json!([credential, credential]);
        assert_eq!(validate(&duplicate), Err("DUPLICATE_CREDENTIAL_ID"));
        let (converted, was_converted) = register(state, "156").unwrap();
        assert!(was_converted);
        assert_eq!(converted, native);
        let without = snapshot(&collected(), &Options::default(), "156").unwrap();
        assert_eq!(format(&without), FORMAT);
        assert!(without.get("credentials").is_none());
    }

    #[test]
    fn origin_scripts_restore_or_merge_only_their_origin_state() {
        let state = collected();
        let restore = restore_expression(&state, "https://example.com");
        assert!(restore.contains(r#"await script.restore({"localStorage""#));
        let cleared = restore_expression(&state, "https://other.example.com");
        assert!(cleared.contains("await script.restore(undefined);"));
        let merge = merge_expression(&state, "https://example.com");
        assert!(merge.contains("localStorage.setItem(name,value)"));
        // The embedded library defines restore; a merge never calls it.
        assert!(!merge.contains("await script.restore("));
        // A database is claimed by an abortable upgrade before it is deleted.
        let claim = merge.find("const claimed=await claim(db.name)").unwrap();
        assert!(claim < merge.rfind("indexedDB.deleteDatabase(db.name)").unwrap());
        assert!(merge.contains("if(blocked)request.transaction.abort()"));
        assert_eq!(origins(&state), ["https://example.com"]);
    }
}

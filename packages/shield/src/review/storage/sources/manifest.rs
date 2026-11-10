//! Artifact sharing preserves the original compact Bundle and policy digests.
use super::{BTreeSet, Error, Namespace, Value, fs, json, private, retention, sync};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{Read, Write};

pub(super) const KIND: &str = "retained-source-v2";
pub(super) const JSON_KIND: &str = "retained-json-v2";
pub(super) const MAX_MANIFEST: usize = 16 * 1024;
pub(super) const RESERVE_ENTRIES: usize = 70;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Blob {
    digest: String,
    length: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "encoding", content = "value", rename_all = "snake_case")]
enum Part {
    Text(Blob),
    Json(Blob),
    Texts(Vec<Blob>),
    Literal(Value),
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u8,
    digest: String,
    candidate_digest: String,
    length: u64,
    fields: BTreeMap<String, Part>,
}

fn hash_ok(value: &Value) -> bool {
    value.as_str().is_some_and(|v| {
        v.len() == 64
            && v.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
pub(super) fn check(reference: &Value) -> Result<(), Error> {
    if reference.as_object().is_none_or(|m| m.len() != 6)
        || reference["kind"] != KIND
        || !["digest", "candidate_digest", "manifest_digest"]
            .iter()
            .all(|f| hash_ok(&reference[*f]))
        || reference["length"]
            .as_u64()
            .is_none_or(|n| n > crate::configuration::MAX_BUNDLE_BYTES as u64)
        || reference["manifest_length"]
            .as_u64()
            .is_none_or(|n| n > MAX_MANIFEST as u64)
    {
        return Err(Error::Invalid("invalid source manifest reference"));
    }
    Ok(())
}
pub(super) fn name(reference: &Value) -> Result<String, Error> {
    check(reference)?;
    Ok(format!(
        "{}.manifest.json",
        reference["manifest_digest"]
            .as_str()
            .ok_or(Error::Invalid("source manifest digest missing"))?
    ))
}
pub(super) fn json_name(reference: &Value) -> Result<String, Error> {
    if reference["kind"] != JSON_KIND
        || reference.as_object().is_none_or(|m| m.len() != 3)
        || !hash_ok(&reference["digest"])
        || reference["length"]
            .as_u64()
            .is_none_or(|n| n > super::super::MAX_STATE as u64)
    {
        return Err(Error::Invalid("invalid retained metadata reference"));
    }
    Ok(format!(
        "{}.blob",
        reference["digest"]
            .as_str()
            .ok_or(Error::Invalid("metadata digest missing"))?
    ))
}
pub(super) fn retain_json(
    store: &Namespace,
    value: &Value,
) -> Result<(Value, super::super::objects::Pins), Error> {
    if !crate::json::fits(value, super::super::MAX_STATE) {
        return Err(crate::policy::Error::Limit.into());
    }
    super::super::private_directory(&store.directory.join("sources"))?;
    let mut batch = retention::Batch::new(&Value::Null, true)?;
    let blob = put(store, &mut batch, Input::Json(value), "blob", true)?;
    let reference = json!({"kind":JSON_KIND,"digest":blob.digest,"length":blob.length});
    Ok((reference, batch.finish(store)?))
}
pub(super) fn read_json(store: &Namespace, reference: &Value) -> Result<Value, Error> {
    let bytes = object_bytes(
        store,
        &json_name(reference)?,
        reference["length"]
            .as_u64()
            .ok_or(Error::Invalid("metadata length missing"))?,
        reference["digest"]
            .as_str()
            .ok_or(Error::Invalid("metadata digest missing"))?,
        super::super::MAX_STATE,
    )?;
    crate::json::parse(&bytes).map_err(|_| Error::Invalid("retained metadata JSON invalid"))
}

fn copy_object(
    target: &Namespace,
    source: &Namespace,
    batch: &mut retention::Batch,
    blob: &Blob,
    suffix: &str,
    pin: bool,
) -> Result<(), Error> {
    let name = format!("{}.{suffix}", blob.digest);
    if suffix != "manifest.json" {
        batch.protect_name(&name, pin);
    }
    batch.lock(target)?;
    let directory = target.directory.join("sources");
    let path = directory.join(&name);
    if path.try_exists()? {
        let mut file = super::super::objects::read_file(&path)?;
        if file.metadata()?.len() != blob.length {
            return Err(Error::Invalid("retained source was replaced"));
        }
        verify_existing(&mut file, blob.length, &blob.digest)?;
    } else {
        batch.admit(target, &name, blob.length)?;
        let mut original =
            super::super::objects::read_file(&source.directory.join("sources").join(&name))?;
        if original.metadata()?.len() != blob.length {
            return Err(Error::Invalid("imported source length differs"));
        }
        let temporary = directory.join(format!(
            "{}.{}-{}.tmp",
            blob.digest,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let result = (|| {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            private(&mut options);
            let mut file = options.open(&temporary)?;
            let mut input = (&mut original).take(blob.length.saturating_add(1));
            let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
            let mut bytes = vec![0u8; 64 * 1024];
            let mut observed = 0u64;
            loop {
                let n = input.read(&mut bytes)?;
                if n == 0 {
                    break;
                }
                observed += n as u64;
                if observed > blob.length {
                    return Err(Error::Invalid("imported source grew"));
                }
                hash.update(&bytes[..n]);
                file.write_all(&bytes[..n])?;
            }
            if observed != blob.length
                || canonic_registry_kit::digest::encode(hash.finish().as_ref()) != blob.digest
            {
                return Err(Error::Invalid("imported source digest differs"));
            }
            sync(&file)?;
            drop(file);
            super::super::replace(&temporary, &path)?;
            #[cfg(unix)]
            sync(&fs::File::open(&directory)?)?;
            Ok::<_, Error>(())
        })();
        let _ = fs::remove_file(&temporary);
        result?;
        batch.added(blob.length);
    }
    if suffix == "manifest.json" {
        batch.protect_name(&name, pin);
    }
    Ok(())
}
pub(super) fn import(
    target: &Namespace,
    source: &Namespace,
    reference: &Value,
) -> Result<(Value, super::super::objects::Pins), Error> {
    let _source_pin = source.pin(reference)?;
    if reference["kind"] != KIND {
        return target.retain_owned(source.captured(reference)?);
    }
    let manifest = read_manifest(source, reference)?;
    super::super::private_directory(&target.directory.join("sources"))?;
    let mut batch = retention::Batch::new(&Value::Null, true)?;
    let mut blobs = BTreeMap::<String, Blob>::new();
    for part in manifest.fields.values() {
        match part {
            Part::Text(blob) | Part::Json(blob) => {
                blobs.insert(blob.digest.clone(), blob.clone());
            }
            Part::Texts(rows) => {
                for blob in rows {
                    blobs.insert(blob.digest.clone(), blob.clone());
                }
            }
            Part::Literal(_) => {}
        }
    }
    for blob in blobs.values() {
        copy_object(target, source, &mut batch, blob, "blob", false)?;
    }
    copy_object(
        target,
        source,
        &mut batch,
        &Blob {
            digest: reference["manifest_digest"]
                .as_str()
                .ok_or(Error::Invalid("source manifest digest missing"))?
                .into(),
            length: reference["manifest_length"]
                .as_u64()
                .ok_or(Error::Invalid("source manifest length missing"))?,
        },
        "manifest.json",
        true,
    )?;
    Ok((reference.clone(), batch.finish(target)?))
}
fn blob_name(blob: &Blob) -> Result<String, Error> {
    if !hash_ok(&json!(blob.digest)) || blob.length > crate::configuration::MAX_BUNDLE_BYTES as u64
    {
        return Err(Error::Invalid("invalid source blob reference"));
    }
    Ok(format!("{}.blob", blob.digest))
}
fn object_bytes(
    store: &Namespace,
    name: &str,
    length: u64,
    digest: &str,
    limit: usize,
) -> Result<Vec<u8>, Error> {
    let file = super::super::objects::read_file(&store.directory.join("sources").join(name))?;
    if file.metadata()?.len() != length || length > limit as u64 {
        return Err(Error::Invalid("source object length differs"));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != length || policy_hash(&bytes) != digest {
        return Err(Error::Invalid("source object digest differs"));
    }
    crate::diagnostics::trace::count("source.read_bytes", bytes.len() as u64);
    Ok(bytes)
}
fn policy_hash(bytes: &[u8]) -> String {
    crate::policy::fingerprint(bytes)
}
fn read_manifest(store: &Namespace, reference: &Value) -> Result<Manifest, Error> {
    let bytes = object_bytes(
        store,
        &name(reference)?,
        reference["manifest_length"]
            .as_u64()
            .ok_or(Error::Invalid("source manifest length missing"))?,
        reference["manifest_digest"]
            .as_str()
            .ok_or(Error::Invalid("source manifest digest missing"))?,
        MAX_MANIFEST,
    )?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|_| Error::Invalid("invalid source manifest"))?;
    if manifest.schema_version != 2
        || Some(manifest.digest.as_str()) != reference["digest"].as_str()
        || Some(manifest.candidate_digest.as_str()) != reference["candidate_digest"].as_str()
        || Some(manifest.length) != reference["length"].as_u64()
    {
        return Err(Error::Invalid("source manifest binding differs"));
    }
    validate_manifest(&manifest)?;
    Ok(manifest)
}
fn validate_manifest(manifest: &Manifest) -> Result<(), Error> {
    let mut artifacts = 0;
    let mut charge = 0u64;
    let mut account = |blob: &Blob| -> Result<(), Error> {
        blob_name(blob)?;
        charge = charge
            .checked_add(blob.length)
            .filter(|total| *total <= manifest.length)
            .ok_or(Error::Invalid("source manifest aggregate exceeds bundle"))?;
        Ok(())
    };
    for (field, part) in &manifest.fields {
        match (field.as_str(), part) {
            ("config" | "local_config" | "rules" | "local_rules", Part::Text(blob))
            | ("lock", Part::Json(blob)) => {
                account(blob)?;
            }
            ("packs" | "disabled", Part::Texts(blobs)) => {
                artifacts += blobs.len();
                for blob in blobs {
                    account(blob)?;
                }
            }
            ("schema_version" | "kind" | "trust", Part::Literal(value))
                if value.is_number() || value.is_string() => {}
            ("local_config" | "rules" | "local_rules", Part::Literal(Value::Null)) => {}
            _ => return Err(Error::Invalid("invalid source manifest field")),
        }
    }
    if artifacts > 64
        || !manifest.fields.contains_key("config")
        || !manifest.fields.contains_key("lock")
    {
        return Err(Error::Invalid("invalid source manifest shape"));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Input<'a> {
    Text(&'a str),
    Json(&'a Value),
}
pub(super) fn verify_existing(reader: impl Read, length: u64, digest: &str) -> Result<(), Error> {
    let mut reader = reader.take(length.saturating_add(1));
    let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
    let mut bytes = vec![0u8; 64 * 1024];
    let mut observed = 0u64;
    loop {
        let n = reader.read(&mut bytes)?;
        if n == 0 {
            break;
        }
        observed += n as u64;
        hash.update(&bytes[..n]);
    }
    if observed != length || canonic_registry_kit::digest::encode(hash.finish().as_ref()) != digest
    {
        return Err(Error::Invalid("retained source was replaced"));
    }
    Ok(())
}
impl Input<'_> {
    fn digest_length(&self) -> Result<(String, u64), Error> {
        match self {
            Self::Text(text) => Ok((policy_hash(text.as_bytes()), text.len() as u64)),
            Self::Json(value) => Ok((
                crate::json::fingerprint(value)
                    .map_err(|_| Error::Invalid("source blob encoding"))?,
                crate::json::encoded_len(value)
                    .map_err(|_| Error::Invalid("source blob encoding"))? as u64,
            )),
        }
    }
    fn write(&self, file: &mut fs::File) -> Result<(), Error> {
        match self {
            Self::Text(text) => file.write_all(text.as_bytes())?,
            Self::Json(value) => serde_json::to_writer(file, value)
                .map_err(|_| Error::Invalid("source blob encoding"))?,
        }
        Ok(())
    }
}
fn put(
    store: &Namespace,
    batch: &mut retention::Batch,
    input: Input<'_>,
    suffix: &str,
    pin: bool,
) -> Result<Blob, Error> {
    let (digest, length) = input.digest_length()?;
    if length > crate::configuration::MAX_BUNDLE_BYTES as u64 {
        return Err(crate::policy::Error::Limit.into());
    }
    let name = format!("{digest}.{suffix}");
    if suffix != "manifest.json" {
        batch.protect_name(&name, pin);
    }
    batch.lock(store)?;
    let directory = store.directory.join("sources");
    super::super::private_directory(&directory)?;
    let path = directory.join(&name);
    if path.try_exists()? {
        let mut file = super::super::objects::read_file(&path)?;
        if file.metadata()?.len() != length {
            return Err(Error::Invalid("retained source was replaced"));
        }
        verify_existing(&mut file, length, &digest)?;
        if file.metadata()?.len() != length {
            return Err(Error::Invalid("retained source was replaced"));
        }
    } else {
        batch.admit(store, &name, length)?;
        let temporary = directory.join(format!(
            "{digest}.{}-{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        let result = (|| {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            private(&mut options);
            let mut file = options.open(&temporary)?;
            input.write(&mut file)?;
            sync(&file)?;
            drop(file);
            super::super::replace(&temporary, &path)?;
            #[cfg(unix)]
            sync(&fs::File::open(&directory)?)?;
            Ok::<_, Error>(())
        })();
        let _ = fs::remove_file(&temporary);
        result?;
        batch.added(length);
    }
    if suffix == "manifest.json" {
        batch.protect_name(&name, pin);
    }
    Ok(Blob { digest, length })
}
pub(super) fn retain(
    store: &Namespace,
    value: &Value,
    batch: &mut retention::Batch,
) -> Result<Value, Error> {
    super::super::private_directory(&store.directory.join("sources"))?;
    let digest = crate::json::fingerprint(value).map_err(|_| Error::Invalid("source encoding"))?;
    let length = crate::json::encoded_len(value).map_err(|_| Error::Invalid("source encoding"))?;
    if length > crate::configuration::MAX_BUNDLE_BYTES {
        return Err(crate::policy::Error::Limit.into());
    }
    let candidate_digest = crate::json::fingerprint(&BTreeMap::from([("project", value)]))
        .map_err(|_| Error::Invalid("source encoding"))?;
    let mut fields = BTreeMap::new();
    for (field, item) in value
        .as_object()
        .ok_or(Error::Invalid("source bundle object"))?
    {
        let part = match field.as_str() {
            "schema_version" | "kind" | "trust" => Part::Literal(item.clone()),
            "config" | "local_config" | "rules" | "local_rules" if item.is_null() => {
                Part::Literal(Value::Null)
            }
            "config" | "local_config" | "rules" | "local_rules" => Part::Text(put(
                store,
                batch,
                Input::Text(item.as_str().ok_or(Error::Invalid("source text"))?),
                "blob",
                false,
            )?),
            "packs" | "disabled" => Part::Texts(
                item.as_array()
                    .ok_or(Error::Invalid("source artifacts"))?
                    .iter()
                    .map(|item| {
                        put(
                            store,
                            batch,
                            Input::Text(
                                item.as_str()
                                    .ok_or(Error::Invalid("source artifact text"))?,
                            ),
                            "blob",
                            false,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            "lock" => Part::Json(put(store, batch, Input::Json(item), "blob", false)?),
            _ => return Err(Error::Invalid("unknown source bundle field")),
        };
        fields.insert(field.clone(), part);
    }
    let manifest = Manifest {
        schema_version: 2,
        digest: digest.clone(),
        candidate_digest: candidate_digest.clone(),
        length: length as u64,
        fields,
    };
    validate_manifest(&manifest)?;
    let wire =
        serde_json::to_value(&manifest).map_err(|_| Error::Invalid("source manifest encoding"))?;
    if !crate::json::fits(&wire, MAX_MANIFEST) {
        return Err(crate::policy::Error::Limit.into());
    }
    let stored = put(store, batch, Input::Json(&wire), "manifest.json", true)?;
    Ok(
        json!({"kind":KIND,"digest":digest,"candidate_digest":candidate_digest,"length":length,"manifest_digest":stored.digest,"manifest_length":stored.length}),
    )
}
pub(super) fn hydrate(store: &Namespace, reference: &Value) -> Result<Value, Error> {
    let manifest = read_manifest(store, reference)?;
    let mut value = serde_json::Map::new();
    for (field, part) in manifest.fields {
        value.insert(field, hydrate_part(store, part)?);
    }
    let value = Value::Object(value);
    if crate::json::encoded_len(&value).ok()
        != reference["length"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
        || crate::json::fingerprint(&value).ok().as_ref()
            != reference["digest"].as_str().map(str::to_owned).as_ref()
        || crate::json::fingerprint(&BTreeMap::from([("project", &value)]))
            .ok()
            .as_ref()
            != reference["candidate_digest"]
                .as_str()
                .map(str::to_owned)
                .as_ref()
    {
        return Err(Error::Invalid("hydrated source identity differs"));
    }
    crate::diagnostics::trace::count("source.hydrated", 1);
    Ok(value)
}
fn hydrate_part(store: &Namespace, part: Part) -> Result<Value, Error> {
    let read = |blob: &Blob| {
        object_bytes(
            store,
            &blob_name(blob)?,
            blob.length,
            &blob.digest,
            crate::configuration::MAX_BUNDLE_BYTES,
        )
    };
    Ok(match part {
        Part::Literal(value) => value,
        Part::Text(blob) => Value::String(
            String::from_utf8(read(&blob)?).map_err(|_| Error::Invalid("source blob UTF8"))?,
        ),
        Part::Json(blob) => {
            crate::json::parse(&read(&blob)?).map_err(|_| Error::Invalid("source JSON blob"))?
        }
        Part::Texts(blobs) => Value::Array(
            blobs
                .iter()
                .map(|blob| {
                    Ok(Value::String(
                        String::from_utf8(read(blob)?)
                            .map_err(|_| Error::Invalid("source blob UTF8"))?,
                    ))
                })
                .collect::<Result<_, Error>>()?,
        ),
    })
}
/// Non-authorizing selected source inspection; full admission verifies old digests.
pub(in crate::review) fn field(
    store: &Namespace,
    reference: &Value,
    field: &str,
    index: Option<usize>,
) -> Result<Value, Error> {
    let mut manifest = read_manifest(store, reference)?;
    let Some(part) = manifest.fields.remove(field) else {
        return Ok(Value::Null);
    };
    let part = match (part, index) {
        (Part::Texts(mut rows), Some(index)) => {
            if index >= rows.len() {
                return Err(Error::Missing);
            }
            Part::Text(rows.remove(index))
        }
        (part, None) => part,
        _ => return Err(Error::Invalid("source field selector")),
    };
    hydrate_part(store, part)
}
pub(super) fn descendants(
    store: &Namespace,
    reference: &Value,
    keep: &mut BTreeSet<String>,
) -> Result<(), Error> {
    let manifest = read_manifest(store, reference)?;
    keep.insert(name(reference)?);
    for part in manifest.fields.values() {
        match part {
            Part::Text(blob) | Part::Json(blob) => {
                keep.insert(blob_name(blob)?);
            }
            Part::Texts(blobs) => {
                for blob in blobs {
                    keep.insert(blob_name(blob)?);
                }
            }
            Part::Literal(_) => {}
        }
    }
    Ok(())
}
pub(super) fn pinned_descendants(
    store: &Namespace,
    keep: &mut BTreeSet<String>,
) -> Result<(), Error> {
    let directory = store.directory.join("sources");
    let entries = fs::read_dir(&directory)?
        .take(16_385)
        .collect::<Result<Vec<_>, _>>()?;
    if entries.len() > 16_384 {
        return Err(Error::CleanupPending);
    }
    for entry in entries {
        let name = entry.file_name();
        let Some(name) = name.to_str().filter(|n| n.ends_with(".manifest.json")) else {
            continue;
        };
        let file = super::super::objects::read_file(&entry.path())?;
        match super::super::objects::reader_pinned(&entry.path()) {
            Ok(true) => {
                let length = file.metadata()?.len();
                let digest = name
                    .strip_suffix(".manifest.json")
                    .ok_or(Error::Invalid("source manifest filename invalid"))?;
                let bytes = object_bytes(store, name, length, digest, MAX_MANIFEST)?;
                let value: Manifest = serde_json::from_slice(&bytes)
                    .map_err(|_| Error::Invalid("pinned source manifest unsupported"))?;
                let reference = json!({"kind":KIND,"digest":value.digest,"candidate_digest":value.candidate_digest,"length":value.length,"manifest_digest":digest,"manifest_length":length});
                descendants(store, &reference, keep)?;
            }
            Ok(false) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
pub(super) fn expand_names(store: &Namespace, keep: &mut BTreeSet<String>) -> Result<(), Error> {
    let names: Vec<_> = keep
        .iter()
        .filter(|name| name.ends_with(".manifest.json"))
        .cloned()
        .collect();
    for name in names {
        let directory = store.directory.join("sources");
        let file = match super::super::objects::read_file(&directory.join(&name)) {
            Ok(file) => file,
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                // An unrelated unavailable manifest cannot block a new review.
                // Its descendants are unknown, so conservatively preserve the
                // bounded namespace; admission still enforces bytes and entries.
                for (index, entry) in fs::read_dir(&directory)?
                    .take(super::super::objects::FILE_LIMIT + 1)
                    .enumerate()
                {
                    if index == super::super::objects::FILE_LIMIT {
                        return Err(Error::CleanupPending);
                    }
                    if let Some(name) = entry?.file_name().to_str() {
                        keep.insert(name.to_owned());
                    }
                }
                continue;
            }
            Err(error) => return Err(error),
        };
        let length = file.metadata()?.len();
        let digest = name
            .strip_suffix(".manifest.json")
            .ok_or(Error::Invalid("source manifest filename invalid"))?;
        let bytes = object_bytes(store, &name, length, digest, MAX_MANIFEST)?;
        let value: Manifest = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Invalid("source manifest unsupported"))?;
        let reference = json!({"kind":KIND,"digest":value.digest,"candidate_digest":value.candidate_digest,"length":value.length,"manifest_digest":digest,"manifest_length":length});
        descendants(store, &reference, keep)?;
    }
    Ok(())
}

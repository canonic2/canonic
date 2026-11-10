//! Bounded overflow roots avoid a shared lifetime cap across unrelated policies.
use super::{
    BTreeMap, Error, INDEX_LIMIT, Index, Namespace, RECORD_LIMIT, Record, json, namespace, objects,
    private_files, read_index, sources,
};

fn valid(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn full(index: &Index) -> bool {
    index.candidates.values().map(BTreeMap::len).sum::<usize>() >= RECORD_LIMIT
        || !crate::json::fits(&json!(index), INDEX_LIMIT / 2)
}

pub(super) fn select(key: &str, create: bool) -> Result<Namespace, Error> {
    if !valid(key) {
        return Err(Error::Invalid("private consent partition key invalid"));
    }
    let shared = namespace(create)?;
    let path = shared.directory.join("policies").join(key);
    if path.join("state.json").try_exists()? {
        private_files::directory(&shared.directory.join("policies"), false)?;
        return Namespace::private(&path);
    }
    if create {
        let index = read_index(&shared)?;
        if full(&index) {
            return partition(&shared, key);
        }
    }
    Ok(shared)
}

pub(super) fn create(key: &str) -> Result<Namespace, Error> {
    if !valid(key) {
        return Err(Error::Invalid("private consent partition key invalid"));
    }
    partition(&namespace(true)?, key)
}

fn partition(shared: &Namespace, key: &str) -> Result<Namespace, Error> {
    // The original namespace lock serializes first publication with historical
    // writers. New readers select a partition only after its root is durable.
    let _guard = objects::Directory::enter(&shared.directory)?;
    let base = shared.directory.join("policies");
    private_files::directory(&base, true)?;
    let path = base.join(key);
    private_files::directory(&path, true)?;
    let target = Namespace::private(&path)?;
    if path.join("state.json").try_exists()? {
        return Ok(target);
    }
    let mut index = Index {
        schema_version: 3,
        ..Index::default()
    };
    let mut pins = Vec::new();
    let old = read_index(shared)?;
    if let Some(variants) = old.candidates.get(key) {
        let mut imported = BTreeMap::new();
        for record in variants.values() {
            let (source, source_pin) = target.import(shared, &record.source)?;
            let (context, context_pin) = target.retain_json(&shared.read_json(&record.context)?)?;
            pins.extend([source_pin, context_pin]);
            imported.insert(
                sources::candidate_identity(&source),
                Record { source, context },
            );
        }
        index.candidates.insert(key.into(), imported);
    }
    private_files::write(&path.join("state.json"), &json!(index), INDEX_LIMIT)?;
    Ok(target)
}

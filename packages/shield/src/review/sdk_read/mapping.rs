//! The reviewed adapter's complete owned JavaScript mapping closure.
use ring::digest::{Context as DigestContext, SHA256};

pub(super) const ID: &str = "claude-read-only";
const FILES: &[(&str, &[u8])] = &[
    (
        "claude-runtime-read-only/index.mjs",
        include_bytes!("../../../adapters/src/modules/claude-runtime-read-only/index.mjs"),
    ),
    (
        "claude-runtime-read-only/binary.mjs",
        include_bytes!("../../../adapters/src/modules/claude-runtime-read-only/binary.mjs"),
    ),
    (
        "claude-runtime-read-only/owner.mjs",
        include_bytes!("../../../adapters/src/modules/claude-runtime-read-only/owner.mjs"),
    ),
    (
        "runtime-response/index.mjs",
        include_bytes!("../../../adapters/src/modules/runtime-response/index.mjs"),
    ),
    (
        "enforcement-response/json.mjs",
        include_bytes!("../../../adapters/src/modules/enforcement-response/json.mjs"),
    ),
    (
        "process-transport/index.mjs",
        include_bytes!("../../../adapters/src/modules/process-transport/index.mjs"),
    ),
];

pub(super) fn digest() -> &'static str {
    static DIGEST: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    DIGEST.get_or_init(|| {
        let mut hash = DigestContext::new(&SHA256);
        hash.update(b"shield-sdk-read-mapping-v1\0");
        for (path, content) in FILES {
            hash.update(
                &u64::try_from(path.len())
                    .expect("bounded mapping path")
                    .to_be_bytes(),
            );
            hash.update(path.as_bytes());
            hash.update(
                &u64::try_from(content.len())
                    .expect("bounded embedded mapping")
                    .to_be_bytes(),
            );
            hash.update(content);
        }
        canonic_registry_kit::digest::encode(hash.finish().as_ref())
    })
}

pub(super) fn paths() -> Vec<&'static str> {
    FILES.iter().map(|(path, _)| *path).collect()
}

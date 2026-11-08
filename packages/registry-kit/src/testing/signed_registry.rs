//! A signed registry, with releases and catalogs, served by an owned [`Server`].

use super::{Response, Server, SigningKey};
use crate::{catalog::Entry, digest, names::PackId};
use semver::Version;
use serde_json::json;

/// A signed registry served by an owned [`Server`].
pub struct SignedRegistry {
    server: Server,
    key: SigningKey,
    registry_id: String,
    entries: Vec<Entry>,
}

impl SignedRegistry {
    /// Starts a server publishing `catalog-key.pub` for a new key.
    #[must_use]
    pub fn new(registry_id: &str) -> Self {
        let server = Server::new();
        let key = SigningKey::generate();
        server.set("catalog-key.pub", Response::ok(key.public_hex()));
        Self {
            server,
            key,
            registry_id: registry_id.to_owned(),
            entries: Vec::new(),
        }
    }

    /// Serves `bytes` as the artifact of `id@version` and lists it in the next catalog.
    ///
    /// # Panics
    /// Panics for an invalid pack ID or version.
    pub fn release(&mut self, id: &str, version: &str, bytes: &[u8]) -> Entry {
        let id: PackId = id.parse().expect("fixture pack ID");
        let version: Version = version.parse().expect("fixture version");
        let entry = Entry {
            path: format!("packs/{}-{}-{version}", id.publisher(), id.name()),
            sha256: digest::sha256_hex(bytes),
            id,
            version,
        };
        self.server.set(&entry.path, Response::ok(bytes.to_vec()));
        self.entries.push(entry.clone());
        entry
    }

    /// Publishes a signed catalog of every release with this sequence and expiry.
    pub fn publish(&self, sequence: u64, expires_at: &str) {
        let bytes = json!({
            "schema_version": 1,
            "registry_id": self.registry_id,
            "sequence": sequence,
            "expires_at": expires_at,
            "packs": self.entries,
        })
        .to_string();
        self.publish_bytes(bytes.as_bytes());
    }

    /// Publishes a version-3 discovery catalog and content-addressed history trees.
    ///
    /// # Panics
    /// Panics if an internal fixture page is empty.
    pub fn publish_indexed(&self, sequence: u64, expires_at: &str) {
        let mut groups = std::collections::BTreeMap::<_, Vec<_>>::new();
        for entry in &self.entries {
            groups.entry(entry.id.clone()).or_default().push(entry);
        }
        let mut heads = Vec::new();
        let mut histories = std::collections::BTreeMap::new();
        for (id, mut entries) in groups {
            entries.sort_by(|a, b| a.version.cmp(&b.version));
            heads.push(
                *entries
                    .iter()
                    .rev()
                    .find(|entry| entry.version.pre.is_empty())
                    .or_else(|| entries.last())
                    .expect("fixture releases"),
            );
            let mut level: Vec<_> = entries
                .chunks(crate::catalog::history::PAGE_SIZE)
                .map(|entries| {
                    self.page(
                        &id,
                        json!({"entries":entries}),
                        &entries[0].version,
                        &entries.last().expect("chunk").version,
                    )
                })
                .collect();
            while level.len() > 1 {
                level = level
                    .chunks(crate::catalog::history::PAGE_SIZE)
                    .map(|children| {
                        self.page(
                            &id,
                            json!({"children":children}),
                            &children[0].first,
                            &children.last().expect("chunk").last,
                        )
                    })
                    .collect();
            }
            histories.insert(id, level.pop().expect("fixture history"));
        }
        self.publish_bytes(
            json!({"schema_version":3,"registry_id":self.registry_id,
            "sequence":sequence,"expires_at":expires_at,"packs":heads,"histories":histories})
            .to_string()
            .as_bytes(),
        );
    }

    fn page(
        &self,
        id: &PackId,
        mut body: serde_json::Value,
        first: &Version,
        last: &Version,
    ) -> crate::catalog::history::Reference {
        body["schema_version"] = json!(1);
        body["id"] = json!(id);
        let bytes = body.to_string().into_bytes();
        let sha256 = digest::sha256_hex(&bytes);
        let path = format!("metadata/{sha256}.json");
        self.server.set(&path, Response::ok(bytes));
        crate::catalog::history::Reference {
            path,
            sha256,
            first: first.clone(),
            last: last.clone(),
        }
    }

    /// Publishes exact catalog bytes with a valid signature.
    pub fn publish_bytes(&self, bytes: &[u8]) {
        self.server
            .set("catalog.json", Response::ok(bytes.to_vec()));
        self.server
            .set("catalog.sig", Response::ok(self.key.sign_hex(bytes)));
    }

    /// The underlying server, for raw responses and request inspection.
    #[must_use]
    pub fn server(&self) -> &Server {
        &self.server
    }

    /// The signing key.
    #[must_use]
    pub fn key(&self) -> &SigningKey {
        &self.key
    }
}

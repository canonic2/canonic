//! Generated Ed25519 keys that sign catalogs the way a registry publisher does.

use crate::digest;
use ring::signature::{Ed25519KeyPair, KeyPair};

/// A generated Ed25519 catalog signing key.
pub struct SigningKey(Ed25519KeyPair);

impl SigningKey {
    /// Generates a new key.
    ///
    /// # Panics
    /// Panics if the system random source fails.
    #[must_use]
    pub fn generate() -> Self {
        let document = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
            .expect("key generation");
        Self(Ed25519KeyPair::from_pkcs8(document.as_ref()).expect("generated key"))
    }

    /// The public key as `catalog-key.pub` content.
    #[must_use]
    pub fn public_hex(&self) -> String {
        digest::encode(self.0.public_key().as_ref())
    }

    /// A detached signature over `bytes` as `catalog.sig` content.
    #[must_use]
    pub fn sign_hex(&self, bytes: &[u8]) -> String {
        digest::encode(self.0.sign(bytes).as_ref())
    }
}

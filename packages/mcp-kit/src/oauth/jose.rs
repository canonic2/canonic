//! Compact JSON Web Signatures and JSON Web Key Sets for identity assertions:
//! bounded decoding, key set preparation and RS256/ES256 verification. Nothing
//! here fetches keys; [`super::IdentityAssertions`] supplies key set bytes.

use super::{
    assertion::{MAX_KEY_SET_BYTES, MAX_KEYS, RSA_MODULUS_BITS, SigningAlgorithm},
    wire,
};
// RS256 hashes with the SHA-256 that `rsa` is built on.
use rsa::{BoxedUint, pkcs1v15, sha2::Sha256, signature::Verifier as _};
use serde_json::{Map, Value};

/// The largest RSA public exponent, in bytes.
const MAX_EXPONENT_BYTES: usize = 8;
/// A P-256 coordinate, in bytes.
const COORDINATE_BYTES: usize = 32;

/// A compact JWS, split and decoded but not yet verified.
pub(super) struct Compact<'a> {
    pub(super) header: Map<String, Value>,
    pub(super) claims: Map<String, Value>,
    /// `header.payload`, the bytes the signature covers.
    pub(super) signing_input: &'a [u8],
    pub(super) signature: Vec<u8>,
}

/// Splits a compact JWS of at most `limit` bytes into its JSON object header,
/// JSON object claims and signature. Encrypted (five-part) tokens are refused.
pub(super) fn parse(token: &str, limit: usize) -> Option<Compact<'_>> {
    if token.len() > limit {
        return None;
    }
    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let signed = header.len() + 1 + payload.len();
    Some(Compact {
        header: object(&wire::decode_base64url(header, limit)?)?,
        claims: object(&wire::decode_base64url(payload, limit)?)?,
        signing_input: &token.as_bytes()[..signed],
        signature: wire::decode_base64url(signature, limit)?,
    })
}

fn object(bytes: &[u8]) -> Option<Map<String, Value>> {
    match serde_json::from_slice(bytes).ok()? {
        Value::Object(object) => Some(object),
        _ => None,
    }
}

/// A prepared verification key from an issuer's key set.
pub(super) enum Key {
    Rs256 {
        id: Option<String>,
        key: pkcs1v15::VerifyingKey<Sha256>,
    },
    Es256 {
        id: Option<String>,
        key: p256::ecdsa::VerifyingKey,
    },
}

impl Key {
    pub(super) fn algorithm(&self) -> SigningAlgorithm {
        match self {
            Self::Rs256 { .. } => SigningAlgorithm::Rs256,
            Self::Es256 { .. } => SigningAlgorithm::Es256,
        }
    }

    pub(super) fn id(&self) -> Option<&str> {
        match self {
            Self::Rs256 { id, .. } | Self::Es256 { id, .. } => id.as_deref(),
        }
    }

    /// Whether `signature` is this key's signature over `input`.
    pub(super) fn verify(&self, input: &[u8], signature: &[u8]) -> bool {
        match self {
            Self::Rs256 { key, .. } => pkcs1v15::Signature::try_from(signature)
                .is_ok_and(|signature| key.verify(input, &signature).is_ok()),
            Self::Es256 { key, .. } => {
                use p256::ecdsa::signature::Verifier as _;
                p256::ecdsa::Signature::from_slice(signature)
                    .is_ok_and(|signature| key.verify(input, &signature).is_ok())
            }
        }
    }
}

/// Why a key set cannot be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum KeySetError {
    TooLarge,
    Malformed,
}

/// Prepares the signing keys of a JSON Web Key Set. Keys of other types or
/// curves, encryption keys and keys too weak or too large are skipped; a set
/// that is not a JSON object with a `keys` array, or that holds more than
/// [`MAX_KEYS`] entries, is refused.
pub(super) fn key_set(bytes: &[u8]) -> Result<Vec<Key>, KeySetError> {
    if bytes.len() > MAX_KEY_SET_BYTES {
        return Err(KeySetError::TooLarge);
    }
    let document = object(bytes).ok_or(KeySetError::Malformed)?;
    let keys = document
        .get("keys")
        .and_then(Value::as_array)
        .ok_or(KeySetError::Malformed)?;
    if keys.len() > MAX_KEYS {
        return Err(KeySetError::TooLarge);
    }
    Ok(keys.iter().filter_map(prepare).collect())
}

fn prepare(jwk: &Value) -> Option<Key> {
    let jwk = jwk.as_object()?;
    let text = |name: &str| jwk.get(name).and_then(Value::as_str);
    if jwk.get("use").is_some_and(|usage| usage != "sig") {
        return None;
    }
    if let Some(operations) = jwk.get("key_ops")
        && !operations
            .as_array()?
            .iter()
            .any(|operation| operation == "verify")
    {
        return None;
    }
    let id = match jwk.get("kid") {
        None => None,
        Some(Value::String(id)) => Some(id.clone()),
        Some(_) => return None,
    };
    let declared = match jwk.get("alg") {
        None => None,
        Some(Value::String(name)) => Some(SigningAlgorithm::parse(name)?),
        Some(_) => return None,
    };
    let key = match text("kty")? {
        "RSA" => {
            let modulus = wire::decode_base64url(text("n")?, RSA_MODULUS_BITS.end() / 8)?;
            let exponent = wire::decode_base64url(text("e")?, MAX_EXPONENT_BYTES)?;
            let public = rsa::PublicKey::new_with_size_limits(
                BoxedUint::from_be_slice_vartime(&modulus),
                BoxedUint::from_be_slice_vartime(&exponent),
                RSA_MODULUS_BITS,
            )
            .ok()?;
            Key::Rs256 {
                id,
                key: pkcs1v15::VerifyingKey::new(public),
            }
        }
        "EC" if text("crv") == Some("P-256") => {
            let x = wire::decode_base64url(text("x")?, COORDINATE_BYTES)?;
            let y = wire::decode_base64url(text("y")?, COORDINATE_BYTES)?;
            if x.len() != COORDINATE_BYTES || y.len() != COORDINATE_BYTES {
                return None;
            }
            let point = [&[4][..], &x, &y].concat();
            Key::Es256 {
                id,
                key: p256::ecdsa::VerifyingKey::from_sec1_bytes(&point).ok()?,
            }
        }
        _ => return None,
    };
    declared
        .is_none_or(|algorithm| algorithm == key.algorithm())
        .then_some(key)
}

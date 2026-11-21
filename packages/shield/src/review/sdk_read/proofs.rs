//! Launch bearer secrets never implement serialization or diagnostic formatting.
use crate::review::Error;
use ring::rand::{SecureRandom as _, SystemRandom};

pub(super) struct Secret(String);

fn hex(bytes: &[u8]) -> String {
    let alphabet = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|byte| {
            [
                char::from(alphabet[usize::from(byte >> 4)]),
                char::from(alphabet[usize::from(byte & 15)]),
            ]
        })
        .collect()
}

fn decode(value: &str) -> Result<[u8; 32], Error> {
    if value.len() != 64 {
        return Err(Error::Invalid("SDK read proof unavailable"));
    }
    let digit = |byte| match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(Error::Invalid("SDK read proof unavailable")),
    };
    let mut bytes = [0_u8; 32];
    for (target, pair) in bytes.iter_mut().zip(value.as_bytes().as_chunks::<2>().0) {
        *target = digit(pair[0])? * 16 + digit(pair[1])?;
    }
    Ok(bytes)
}

fn message(request: &str, binding: &str) -> Result<Vec<u8>, Error> {
    if request.is_empty()
        || request.len() > 128
        || request.contains('\0')
        || !super::model::digest(binding)
    {
        return Err(Error::Invalid("SDK read retry binding is invalid"));
    }
    serde_json::to_vec(&("sdk-read-retry-v1", request, binding))
        .map_err(|_| Error::Invalid("SDK read retry encoding"))
}

impl Secret {
    pub(super) fn generate() -> Result<Self, Error> {
        let mut bytes = [0_u8; 32];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| Error::Invalid("SDK read context entropy unavailable"))?;
        Ok(Self(hex(&bytes)))
    }

    pub(super) fn parse(value: String) -> Result<Self, Error> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(Error::Invalid("SDK read context proof unavailable"));
        }
        Ok(Self(value))
    }

    /// Only the dedicated owned launch-pipe writer may expose these bytes.
    pub(super) fn launch_value(&self) -> &str {
        &self.0
    }

    pub(super) fn hash(&self) -> String {
        crate::policy::fingerprint(self.0.as_bytes())
    }

    pub(super) fn retry_reference(&self, request: &str, binding: &str) -> Result<String, Error> {
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &decode(&self.0)?);
        Ok(hex(
            ring::hmac::sign(&key, &message(request, binding)?).as_ref()
        ))
    }

    pub(super) fn verify_retry_reference(
        &self,
        request: &str,
        binding: &str,
        reference: &str,
    ) -> Result<(), Error> {
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &decode(&self.0)?);
        ring::hmac::verify(&key, &message(request, binding)?, &decode(reference)?)
            .map_err(|_| Error::Invalid("SDK read retry proof differs"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn random_launch_proofs_and_retry_bindings_are_independent_of_public_ids() {
        let first = Secret::generate().unwrap();
        let second = Secret::generate().unwrap();
        assert_ne!(first.hash(), second.hash());
        assert_ne!(first.hash(), first.launch_value());
        let binding = "a".repeat(64);
        let reference = first.retry_reference("acme-read", &binding).unwrap();
        first
            .verify_retry_reference("acme-read", &binding, &reference)
            .unwrap();
        assert!(
            second
                .verify_retry_reference("acme-read", &binding, &reference)
                .is_err()
        );
        assert!(
            first
                .verify_retry_reference("other-read", &binding, &reference)
                .is_err()
        );
        assert_eq!(
            reference,
            first.retry_reference("acme-read", &binding).unwrap()
        );
        assert_ne!(
            reference,
            second.retry_reference("acme-read", &binding).unwrap()
        );
        assert_ne!(
            reference,
            first.retry_reference("other-read", &binding).unwrap()
        );
        assert_ne!(
            reference,
            first.retry_reference("acme-read", &"b".repeat(64)).unwrap()
        );
        assert!(Secret::parse("not-a-secret".into()).is_err());
        assert!(first.retry_reference("acme-read", "not-a-binding").is_err());
    }
}

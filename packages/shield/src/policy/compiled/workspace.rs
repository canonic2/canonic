//! Bound compiler-owned wire buffers before allocating or retaining them.
use super::{ENTRY, Error, HEADER, MAX_BYTES, MAX_ROWS, PATTERN_OWNER, patterns, query};
use serde::Serialize;
use std::io::Write;
use std::{collections::BTreeMap, sync::Arc};

pub(super) struct Encoding {
    pub(super) rows: Vec<(Vec<u8>, Vec<u8>, usize)>,
    images: BTreeMap<String, Vec<u8>>,
    image_bytes: usize,
    charged: usize,
    limit: usize,
    row_limit: usize,
}
impl Encoding {
    pub(super) fn new() -> Self {
        Self::bounded(MAX_BYTES, MAX_ROWS)
    }
    fn bounded(limit: usize, row_limit: usize) -> Self {
        Self {
            rows: Vec::new(),
            images: BTreeMap::new(),
            image_bytes: 0,
            charged: HEADER,
            limit,
            row_limit,
        }
    }
    fn remaining(&self) -> usize {
        self.limit.saturating_sub(self.charged)
    }
    fn charge(&mut self, bytes: usize) -> Result<(), Error> {
        self.charged = self
            .charged
            .checked_add(bytes)
            .filter(|n| *n <= self.limit)
            .ok_or(Error::Limit)?;
        Ok(())
    }
    fn image(&mut self, key: String, bytes: Vec<u8>) -> Result<(), Error> {
        if let Some(previous) = self.images.get(&key) {
            return if *previous == bytes {
                Ok(())
            } else {
                Err(Error::Contract("conflicting compiled program images"))
            };
        }
        self.reserve_image(&key, bytes.len())?;
        self.images.insert(key, bytes);
        Ok(())
    }
    fn reserve_image(&mut self, key: &str, length: usize) -> Result<(), Error> {
        if length > patterns::MAX_PATTERN_BYTES
            || self
                .rows
                .len()
                .checked_add(self.images.len())
                .is_none_or(|count| count >= self.row_limit)
            || key
                .len()
                .checked_add(2)
                .is_none_or(|length| length > 65_536)
        {
            return Err(Error::Limit);
        }
        let image_bytes = self
            .image_bytes
            .checked_add(length)
            .filter(|n| *n <= patterns::MAX_PREPARED_BYTES)
            .ok_or(Error::Limit)?;
        self.charge(
            length
                .checked_add(key.len())
                .and_then(|length| length.checked_add(ENTRY + 2))
                .ok_or(Error::Limit)?,
        )?;
        self.image_bytes = image_bytes;
        Ok(())
    }
    pub(super) fn patterns(&mut self, patterns: Arc<patterns::Store>) -> Result<(), Error> {
        match Arc::try_unwrap(patterns) {
            Ok(owned) => self.images(owned.images),
            Err(shared) => {
                for (key, bytes) in &shared.images {
                    if let Some(previous) = self.images.get(key) {
                        if previous != bytes {
                            return Err(Error::Contract("conflicting compiled program images"));
                        }
                        continue;
                    }
                    // Retained comparison inputs can share this Store. Admit
                    // its copy before allocating rather than mutating the Arc.
                    self.reserve_image(key, bytes.len())?;
                    let mut copy = Vec::new();
                    copy.try_reserve_exact(bytes.len())
                        .map_err(|_| Error::Limit)?;
                    copy.extend_from_slice(bytes);
                    self.images.insert(key.clone(), copy);
                }
                Ok(())
            }
        }
    }
    pub(super) fn images(&mut self, images: BTreeMap<String, Vec<u8>>) -> Result<(), Error> {
        for (key, bytes) in images {
            self.image(key, bytes)?;
        }
        Ok(())
    }
    pub(super) fn detach(&mut self, value: &mut serde_json::Value) -> Result<(), Error> {
        crate::graphql::detach(value, &mut |model| {
            let key = format!(
                "graphql:{}",
                crate::json::fingerprint(model).map_err(|_| Error::Syntax)?
            );
            if !self.images.contains_key(&key) {
                let limit = self
                    .remaining()
                    .saturating_sub(ENTRY + key.len() + 2)
                    .min(patterns::MAX_PREPARED_BYTES.saturating_sub(self.image_bytes))
                    .min(patterns::MAX_PATTERN_BYTES);
                let bytes = encode_bounded(model, limit)?;
                self.image(key.clone(), bytes)?;
            }
            Ok(key)
        })
    }
    fn row_header(&mut self, key: &[u8]) -> Result<(), Error> {
        if self
            .rows
            .len()
            .checked_add(self.images.len())
            .is_none_or(|count| count >= self.row_limit)
            || key.len() > 65_536
        {
            return Err(Error::Limit);
        }
        self.charge(ENTRY.checked_add(key.len()).ok_or(Error::Limit)?)?;
        Ok(())
    }
    pub(super) fn row(
        &mut self,
        key: Vec<u8>,
        value: &impl Serialize,
        owner: usize,
    ) -> Result<(), Error> {
        self.row_header(&key)?;
        let bytes = encode_bounded(
            value,
            self.remaining()
                .min((2 * 1024 * 1024usize).saturating_sub(key.len())),
        )?;
        self.charge(bytes.len())?;
        self.rows.try_reserve(1).map_err(|_| Error::Limit)?;
        self.rows.push((key, bytes, owner));
        Ok(())
    }
    pub(super) fn finish_images(&mut self) -> Result<(), Error> {
        for (name, image) in std::mem::take(&mut self.images) {
            let key = query(9, &name);
            // Payload, posting key and index entry were all admitted at image
            // insertion. Moving them into row order does not charge twice.
            self.rows.try_reserve(1).map_err(|_| Error::Limit)?;
            self.rows.push((key, image, PATTERN_OWNER));
        }
        Ok(())
    }
    pub(super) fn metadata(&mut self, metadata: &impl Serialize) -> Result<Vec<u8>, Error> {
        let bytes = encode_bounded(metadata, self.remaining().min(4 * 1024 * 1024))?;
        self.charge(bytes.len())?;
        Ok(bytes)
    }
    pub(super) fn wire_length(&self) -> usize {
        self.charged
    }
}

struct Bounded {
    bytes: Option<Vec<u8>>,
    length: usize,
    limit: usize,
    exceeded: bool,
}
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let Some(length) = self
            .length
            .checked_add(bytes.len())
            .filter(|n| *n <= self.limit)
        else {
            self.exceeded = true;
            return Err(std::io::Error::other("compiled encoding limit"));
        };
        if let Some(target) = &mut self.bytes {
            target.extend_from_slice(bytes);
        }
        self.length = length;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
/// Count under the same bound before allocating. The second pass cannot grow
/// beyond that allocation, including for a stateful/malformed Serialize impl.
fn encode_bounded(value: &impl Serialize, limit: usize) -> Result<Vec<u8>, Error> {
    let mut count = Bounded {
        bytes: None,
        length: 0,
        limit,
        exceeded: false,
    };
    serde_json::to_writer(&mut count, value).map_err(|_| {
        if count.exceeded {
            Error::Limit
        } else {
            Error::Syntax
        }
    })?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(count.length)
        .map_err(|_| Error::Limit)?;
    let mut writer = Bounded {
        bytes: Some(bytes),
        length: 0,
        limit: count.length,
        exceeded: false,
    };
    serde_json::to_writer(&mut writer, value).map_err(|_| {
        if writer.exceeded {
            Error::Limit
        } else {
            Error::Syntax
        }
    })?;
    if writer.length != count.length {
        return Err(Error::Syntax);
    }
    writer.bytes.ok_or(Error::Syntax)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encoding_bounds_rows_metadata_and_cumulative_payload_before_retention() {
        assert_eq!(
            encode_bounded(&serde_json::json!({"acme":"é"}), 64).unwrap(),
            serde_json::to_vec(&serde_json::json!({"acme":"é"})).unwrap()
        );
        assert!(matches!(encode_bounded(&"Acme", 5), Err(Error::Limit)));
        let mut encoding = Encoding::bounded(64, 2);
        encoding.row(vec![0], &"Acme", 0).unwrap();
        assert!(matches!(
            encoding.row(vec![0], &"Acme larger payload", 0),
            Err(Error::Limit)
        ));
        assert_eq!(encoding.rows.len(), 1);
        let mut encoding = Encoding::bounded(128, 1);
        encoding.row(vec![0], &0, 0).unwrap();
        assert!(matches!(encoding.row(vec![0], &0, 0), Err(Error::Limit)));
        assert!(matches!(
            encoding.metadata(&"x".repeat(128)),
            Err(Error::Limit)
        ));
    }
    #[test]
    fn moved_images_deduplicate_without_charging_or_copying_the_same_program_twice() {
        let image = vec![1; 16];
        let pointer = image.as_ptr();
        let mut encoding = Encoding::bounded(128, 4);
        encoding
            .images(BTreeMap::from([("regex:acme".into(), image)]))
            .unwrap();
        encoding
            .images(BTreeMap::from([("regex:acme".into(), vec![1; 16])]))
            .unwrap();
        assert_eq!(encoding.image_bytes, 16);
        encoding.finish_images().unwrap();
        assert_eq!(encoding.rows.len(), 1);
        assert_eq!(encoding.rows[0].1.as_ptr(), pointer);
    }
    #[test]
    fn shared_store_copy_and_image_key_bookkeeping_are_admitted_before_allocation() {
        let mut source = patterns::Store::default();
        source.images.insert("regex:acme".into(), vec![1; 16]);
        let source = Arc::new(source);
        let mut encoding = Encoding::bounded(128, 4);
        encoding.patterns(source.clone()).unwrap();
        assert_eq!(Arc::strong_count(&source), 1);
        assert_ne!(
            encoding.images["regex:acme"].as_ptr(),
            source.images["regex:acme"].as_ptr()
        );
        let charged = encoding.wire_length();
        encoding.patterns(source).unwrap();
        assert_eq!(encoding.wire_length(), charged);
        encoding.finish_images().unwrap();
        assert_eq!(
            encoding.wire_length(),
            charged,
            "moving image postings cannot charge twice"
        );

        let mut encoding = Encoding::bounded(128, 4);
        encoding.image("a".repeat(80), Vec::new()).unwrap();
        assert!(matches!(
            encoding.image("b".repeat(80), Vec::new()),
            Err(Error::Limit)
        ));
        assert_eq!(
            encoding.images.len(),
            1,
            "empty payloads still charge keys and index entries before insertion"
        );
    }
}

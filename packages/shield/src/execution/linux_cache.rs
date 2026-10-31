//! Bounded glibc 1.1 cache entries, retaining every native variant of a library name.
use super::Error;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

pub(super) type Libraries = BTreeMap<String, BTreeSet<PathBuf>>;

fn word(bytes: &[u8], offset: usize) -> Result<u32, Error> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or(Error::Unsupported)?
            .try_into()
            .map_err(|_| Error::Unsupported)?,
    ))
}

fn string(bytes: &[u8], offset: u32, start: usize, end: usize) -> Result<&str, Error> {
    let offset = usize::try_from(offset).map_err(|_| Error::Unsupported)?;
    if !(start..end).contains(&offset) {
        return Err(Error::Unsupported);
    }
    let tail = bytes.get(offset..end).ok_or(Error::Unsupported)?;
    let length = tail
        .iter()
        .position(|byte| *byte == 0)
        .ok_or(Error::Unsupported)?;
    if length == 0 || length > 4096 {
        return Err(Error::Unsupported);
    }
    std::str::from_utf8(&tail[..length]).map_err(|_| Error::Unsupported)
}

pub(super) fn libraries(bytes: &[u8], native_flags: u32) -> Result<Libraries, Error> {
    if bytes.len() < 48
        || bytes.len() > 16_777_216
        || !bytes.starts_with(b"glibc-ld.so.cache1.1")
        || !matches!(bytes[28] & 3, 0 | 2)
    {
        return Err(Error::Unsupported);
    }
    let count = usize::try_from(word(bytes, 20)?).map_err(|_| Error::Unsupported)?;
    if count > 32_768 {
        return Err(Error::Unsupported);
    }
    let start = 48 + count * 24;
    let end = start
        .checked_add(usize::try_from(word(bytes, 24)?).map_err(|_| Error::Unsupported)?)
        .ok_or(Error::Unsupported)?;
    if end > bytes.len() {
        return Err(Error::Unsupported);
    }
    extension(bytes, word(bytes, 32)?)?;
    let mut result = BTreeMap::<String, BTreeSet<PathBuf>>::new();
    let mut baseline = BTreeSet::new();
    for index in 0..count {
        let entry = 48 + index * 24;
        let flags = word(bytes, entry)?;
        if flags != native_flags && flags != 1 {
            continue;
        }
        let name = string(bytes, word(bytes, entry + 4)?, start, end)?;
        if name.len() > 128
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._+-".contains(&byte))
        {
            return Err(Error::Unsupported);
        }
        let path = PathBuf::from(string(bytes, word(bytes, entry + 8)?, start, end)?);
        if !path.is_absolute() {
            return Err(Error::Unsupported);
        }
        if bytes.get(entry + 16..entry + 24) == Some(&[0; 8]) {
            baseline.insert(name.to_owned());
        }
        result.entry(name.into()).or_default().insert(path);
    }
    // A baseline entry avoids relying on CPU-specific availability or emulating
    // glibc's priority selection. Every retained variant is bound by the caller.
    result.retain(|name, _| baseline.contains(name));
    Ok(result)
}

fn extension(bytes: &[u8], offset: u32) -> Result<(), Error> {
    if offset == 0 {
        return Ok(());
    }
    let offset = usize::try_from(offset).map_err(|_| Error::Unsupported)?;
    if offset % 4 != 0 || word(bytes, offset)? != 0_u32.wrapping_sub(358_342_284) {
        return Err(Error::Unsupported);
    }
    let count = usize::try_from(word(bytes, offset + 4)?).map_err(|_| Error::Unsupported)?;
    if count > 64 {
        return Err(Error::Unsupported);
    }
    for index in 0..count {
        let entry = offset
            .checked_add(8 + index * 16)
            .ok_or(Error::Unsupported)?;
        let tag = word(bytes, entry)?;
        let start = usize::try_from(word(bytes, entry + 8)?).map_err(|_| Error::Unsupported)?;
        let size = usize::try_from(word(bytes, entry + 12)?).map_err(|_| Error::Unsupported)?;
        if tag > 1
            || start.checked_add(size).is_none_or(|end| end > bytes.len())
            || (tag == 1 && (start % 4 != 0 || size == 0 || size % 4 != 0))
        {
            return Err(Error::Unsupported);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let strings = b"libc.so.6\0/usr/lib/libc.so.6\0/usr/lib/glibc-hwcaps/v3/libc.so.6\0";
        let mut bytes = vec![0; 96];
        bytes[..20].copy_from_slice(b"glibc-ld.so.cache1.1");
        bytes[20..24].copy_from_slice(&2_u32.to_le_bytes());
        bytes[24..28].copy_from_slice(&u32::try_from(strings.len()).unwrap().to_le_bytes());
        bytes[28] = 2;
        for (entry, path) in [(48, 106_u32), (72, 125)] {
            bytes[entry..entry + 4].copy_from_slice(&0x303_u32.to_le_bytes());
            bytes[entry + 4..entry + 8].copy_from_slice(&96_u32.to_le_bytes());
            bytes[entry + 8..entry + 12].copy_from_slice(&path.to_le_bytes());
        }
        bytes[88] = 1;
        bytes.extend_from_slice(strings);
        bytes
    }
    #[test]
    fn linux_cache_retains_variants_and_requires_native_baseline() {
        let bytes = fixture();
        assert_eq!(libraries(&bytes, 0x303).unwrap()["libc.so.6"].len(), 2);
        assert!(libraries(&bytes, 0xa03).unwrap().is_empty());
        let mut only_variant = bytes.clone();
        only_variant[64] = 1;
        assert!(libraries(&only_variant, 0x303).unwrap().is_empty());
    }
    #[test]
    fn linux_cache_refuses_truncation_relative_paths_and_wrong_endianness() {
        let bytes = fixture();
        for length in [0, 20, 48, 96, bytes.len() - 1] {
            assert!(libraries(&bytes[..length], 0x303).is_err());
        }
        let mut altered = bytes.clone();
        altered[28] = 3;
        assert!(libraries(&altered, 0x303).is_err());
        let mut altered = bytes;
        altered[106] = b'.';
        assert!(libraries(&altered, 0x303).is_err());
    }

    fn extended_fixture() -> (Vec<u8>, usize) {
        let mut bytes = fixture();
        let offset = bytes.len().next_multiple_of(4);
        bytes.resize(offset, 0);
        bytes[32..36].copy_from_slice(&u32::try_from(offset).unwrap().to_le_bytes());
        // One aligned glibc-hwcaps section with one string-table reference.
        for word in [
            0_u32.wrapping_sub(358_342_284),
            1,
            1,
            0,
            u32::try_from(offset + 24).unwrap(),
            4,
            96,
        ] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        (bytes, offset)
    }

    #[test]
    fn linux_cache_accepts_a_bounded_hwcaps_extension() {
        let (bytes, _) = extended_fixture();
        assert_eq!(
            libraries(&bytes, 0x303).unwrap(),
            libraries(&fixture(), 0x303).unwrap()
        );
    }

    #[test]
    fn linux_cache_refuses_invalid_extension_magic_tags_alignment_and_bounds() {
        let (bytes, offset) = extended_fixture();
        for (name, field, value) in [
            ("magic", offset, 0),
            ("unknown tag", offset + 8, 2),
            (
                "extension alignment",
                32,
                u32::try_from(offset + 1).unwrap(),
            ),
            (
                "section alignment",
                offset + 16,
                u32::try_from(offset + 25).unwrap(),
            ),
            ("empty hwcaps", offset + 20, 0),
            ("hwcaps size alignment", offset + 20, 3),
            ("section offset bounds", offset + 16, u32::MAX),
            ("section size bounds", offset + 20, u32::MAX - 3),
            ("section count bound", offset + 4, 65),
            ("extension offset bounds", 32, u32::MAX - 3),
        ] {
            let mut altered = bytes.clone();
            altered[field..field + 4].copy_from_slice(&value.to_le_bytes());
            assert!(libraries(&altered, 0x303).is_err(), "{name}");
        }
        for length in [offset + 4, offset + 8, offset + 20, bytes.len() - 1] {
            assert!(libraries(&bytes[..length], 0x303).is_err());
        }
    }
}

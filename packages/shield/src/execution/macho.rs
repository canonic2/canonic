//! Pure bounded native Mach-O leaf loader contract; scripts/rpaths/custom libraries block.
use super::Error;

fn word(bytes: &[u8], at: usize, big: bool) -> Result<u32, Error> {
    let data: [u8; 4] = bytes
        .get(at..at + 4)
        .ok_or(Error::Unsupported)?
        .try_into()
        .map_err(|_| Error::Unsupported)?;
    Ok(if big {
        u32::from_be_bytes(data)
    } else {
        u32::from_le_bytes(data)
    })
}

fn slice(bytes: &[u8], cpu: u32) -> Result<&[u8], Error> {
    if word(bytes, 0, true)? != 0xcafe_babe {
        return Ok(bytes);
    }
    let count = usize::try_from(word(bytes, 4, true)?).map_err(|_| Error::Unsupported)?;
    if count > 16 {
        return Err(Error::Unsupported);
    }
    let mut found = None;
    for index in 0..count {
        let at = 8 + 20 * index;
        if word(bytes, at, true)? == cpu {
            let offset =
                usize::try_from(word(bytes, at + 8, true)?).map_err(|_| Error::Unsupported)?;
            let size =
                usize::try_from(word(bytes, at + 12, true)?).map_err(|_| Error::Unsupported)?;
            if found.is_some() {
                return Err(Error::Unsupported);
            }
            found = bytes.get(offset..offset.checked_add(size).ok_or(Error::Unsupported)?);
        }
    }
    found.ok_or(Error::Unsupported)
}

fn name(command: &[u8], minimum: usize) -> Result<&str, Error> {
    let offset = usize::try_from(word(command, 8, false)?).map_err(|_| Error::Unsupported)?;
    if offset < minimum {
        return Err(Error::Unsupported);
    }
    let tail = command.get(offset..).ok_or(Error::Unsupported)?;
    let end = tail
        .iter()
        .position(|b| *b == 0)
        .ok_or(Error::Unsupported)?;
    std::str::from_utf8(&tail[..end]).map_err(|_| Error::Unsupported)
}

#[cfg(test)]
pub(super) fn verify(bytes: &[u8], cpu: u32) -> Result<(), Error> {
    verify_family(bytes, cpu, false)
}

pub(super) fn verify_family(bytes: &[u8], cpu: u32, git: bool) -> Result<(), Error> {
    let bytes = slice(bytes, cpu)?;
    if git
        && !bytes
            .windows(b"2.50.1 (Apple Git-155)".len())
            .any(|part| part == b"2.50.1 (Apple Git-155)")
    {
        return Err(Error::Unsupported);
    }
    if word(bytes, 0, false)? != 0xfeed_facf
        || word(bytes, 4, false)? != cpu
        || word(bytes, 12, false)? != 2
    {
        return Err(Error::Unsupported);
    }
    let count = usize::try_from(word(bytes, 16, false)?).map_err(|_| Error::Unsupported)?;
    let size = usize::try_from(word(bytes, 20, false)?).map_err(|_| Error::Unsupported)?;
    if count > 128 || size > 65_536 {
        return Err(Error::Unsupported);
    }
    let commands = bytes.get(32..32 + size).ok_or(Error::Unsupported)?;
    let (mut at, mut dyld, mut library, mut main) = (0usize, 0, 0, 0);
    for _ in 0..count {
        let kind = word(commands, at, false)?;
        let length =
            usize::try_from(word(commands, at + 4, false)?).map_err(|_| Error::Unsupported)?;
        if length < 8 || length % 8 != 0 {
            return Err(Error::Unsupported);
        }
        let command = commands
            .get(at..at.checked_add(length).ok_or(Error::Unsupported)?)
            .ok_or(Error::Unsupported)?;
        match kind {
            0xe if name(command, 12)? == "/usr/lib/dyld" => dyld += 1,
            0xc if name(command, 24)? == "/usr/lib/libSystem.B.dylib" => library += 1,
            0xc if git
                && matches!(
                    name(command, 24)?,
                    "/usr/lib/libz.1.dylib"
                        | "/usr/lib/libiconv.2.dylib"
                        | "/System/Library/Frameworks/CoreServices.framework/Versions/A/CoreServices"
                        | "/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation"
                ) => {}
            0x8000_0028 => main += 1,
            // Explicit data-only commands admitted for the pinned system leaf family.
            0x19 | 0x2 | 0xb | 0x1b | 0x32 | 0x24 | 0x2a | 0x26 | 0x29 | 0x1d | 0x8000_0034
            | 0x8000_0033 => {}
            _ => return Err(Error::Unsupported),
        }
        at += length;
    }
    if at != size || dyld != 1 || library != 1 || main != 1 {
        return Err(Error::Unsupported);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arbitrary_truncated_scripts_and_unknown_loaders_never_validate() {
        for bytes in [
            b"#!/bin/sh\n".as_slice(),
            &[0; 64],
            &[0xca, 0xfe, 0xba, 0xbe, 0, 0, 0, 17],
        ] {
            assert!(verify(bytes, 0x0100_000c).is_err());
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn actual_native_leaf_rejects_changed_loader_and_library() {
        let original = std::fs::read("/bin/echo").unwrap();
        let cpu = if cfg!(target_arch = "aarch64") {
            0x0100_000c
        } else {
            0x0100_0007
        };
        verify(&original, cpu).unwrap();
        let native = slice(&original, cpu).unwrap().to_vec();
        for name in [b"/usr/lib/dyld".as_slice(), b"/usr/lib/libSystem.B.dylib"] {
            let mut changed = native.clone();
            let at = changed
                .windows(name.len())
                .position(|part| part == name)
                .unwrap();
            changed[at + 1] = b'x';
            assert!(verify(&changed, cpu).is_err());
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn actual_git_requires_the_observed_version_and_library_contract() {
        let original = std::fs::read(super::super::git::EXECUTABLE).unwrap();
        let cpu = if cfg!(target_arch = "aarch64") {
            0x0100_000c
        } else {
            0x0100_0007
        };
        verify_family(&original, cpu, true).unwrap();
        assert!(verify(&original, cpu).is_err());
        let native = slice(&original, cpu).unwrap().to_vec();
        for name in [
            b"2.50.1 (Apple Git-155)".as_slice(),
            b"/usr/lib/libz.1.dylib",
        ] {
            let mut changed = native.clone();
            let at = changed
                .windows(name.len())
                .position(|part| part == name)
                .unwrap();
            changed[at] = b'x';
            assert!(verify_family(&changed, cpu, true).is_err());
        }
    }
}

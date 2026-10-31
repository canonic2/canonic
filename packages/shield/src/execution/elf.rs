//! Native ELF64 interpreter and declared-library contract, without executing image bytes.
use super::Error;
use object::{
    elf,
    endian::LittleEndian as LE,
    read::elf::{Dyn, ElfFile64, ProgramHeader},
};
use std::collections::BTreeSet;

#[derive(Clone, Copy)]
pub(super) enum Machine {
    X86_64,
    Aarch64,
}
impl Machine {
    const fn header(self) -> u16 {
        match self {
            Self::X86_64 => elf::EM_X86_64,
            Self::Aarch64 => elf::EM_AARCH64,
        }
    }
}

pub(super) struct Image {
    pub(super) interpreter: Option<String>,
    pub(super) needed: BTreeSet<String>,
}

fn library(bytes: &[u8]) -> Result<String, Error> {
    if bytes.is_empty()
        || bytes.len() > 128
        || !bytes[0].is_ascii_alphanumeric()
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._+-".contains(byte))
    {
        return Err(Error::Unsupported);
    }
    let name = std::str::from_utf8(bytes).map_err(|_| Error::Unsupported)?;
    if !name.contains(".so") {
        return Err(Error::Unsupported);
    }
    Ok(name.into())
}

pub(super) fn inspect(bytes: &[u8], machine: Machine) -> Result<Image, Error> {
    let image = ElfFile64::<LE>::parse(bytes).map_err(|_| Error::Unsupported)?;
    let header = image.elf_header();
    let segments = image.elf_program_headers();
    if header.e_machine.get(LE) != machine.header()
        || ![elf::ET_EXEC, elf::ET_DYN].contains(&header.e_type.get(LE))
        || segments.len() > 128
    {
        return Err(Error::Unsupported);
    }
    let mut interpreter = None;
    let mut dynamics = None;
    for segment in segments {
        if let Some(path) = segment
            .interpreter(LE, bytes)
            .map_err(|_| Error::Unsupported)?
        {
            if interpreter.is_some() || path.len() > 256 || !path.starts_with(b"/") {
                return Err(Error::Unsupported);
            }
            interpreter = Some(
                std::str::from_utf8(path)
                    .map_err(|_| Error::Unsupported)?
                    .into(),
            );
        }
        if let Some(entries) = segment.dynamic(LE, bytes).map_err(|_| Error::Unsupported)? {
            if dynamics.is_some() || entries.len() > 4096 {
                return Err(Error::Unsupported);
            }
            dynamics = Some(entries);
        }
    }
    let (address, size, offsets) = declarations(dynamics.ok_or(Error::Unsupported)?)?;
    let mut strings = None;
    for segment in segments
        .iter()
        .filter(|segment| segment.p_type(LE) == elf::PT_LOAD)
    {
        if let Some(data) = segment
            .data_range(LE, bytes, address, size)
            .map_err(|()| Error::Unsupported)?
            && strings.replace(data).is_some()
        {
            return Err(Error::Unsupported);
        }
    }
    let strings = strings.ok_or(Error::Unsupported)?;
    let mut needed = BTreeSet::new();
    for offset in offsets {
        let tail = strings
            .get(usize::try_from(offset).map_err(|_| Error::Unsupported)?..)
            .ok_or(Error::Unsupported)?;
        let end = tail
            .iter()
            .position(|byte| *byte == 0)
            .ok_or(Error::Unsupported)?;
        needed.insert(library(&tail[..end])?);
    }
    Ok(Image {
        interpreter,
        needed,
    })
}

fn declarations(dynamics: &[elf::Dyn64<LE>]) -> Result<(u64, u64, Vec<u64>), Error> {
    let mut address = None;
    let mut size = None;
    let mut offsets = Vec::new();
    let mut terminated = false;
    for entry in dynamics {
        let tag = entry.d_tag(LE);
        match tag {
            elf::DT_NULL => {
                terminated = true;
                break;
            }
            elf::DT_NEEDED => offsets.push(entry.d_val(LE)),
            elf::DT_STRTAB if address.is_none() => address = Some(entry.d_val(LE)),
            elf::DT_STRSZ if size.is_none() => size = Some(entry.d_val(LE)),
            elf::DT_FLAGS_1 if entry.d_val(LE) & u64::from(elf::DF_1_NODEFLIB) != 0 => {
                return Err(Error::Unsupported);
            }
            elf::DT_STRTAB
            | elf::DT_STRSZ
            | elf::DT_RPATH
            | elf::DT_RUNPATH
            | elf::DT_FILTER
            | elf::DT_AUXILIARY
            | elf::DT_AUDIT
            | elf::DT_DEPAUDIT => {
                return Err(Error::Unsupported);
            }
            _ => {}
        }
    }
    if !terminated || offsets.len() > 128 {
        return Err(Error::Unsupported);
    }
    let address = address.ok_or(Error::Unsupported)?;
    let size = size
        .filter(|size| (1..=4_194_304).contains(size))
        .ok_or(Error::Unsupported)?;
    Ok((address, size, offsets))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(machine: Machine, name: &str) -> Vec<u8> {
        let mut bytes = vec![0; 1024];
        bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        for (offset, value) in [
            (16, elf::ET_DYN),
            (18, machine.header()),
            (52, 64),
            (54, 56),
            (56, 3),
        ] {
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
        bytes[32..40].copy_from_slice(&64_u64.to_le_bytes());
        let interpreter = b"/lib64/ld-linux-x86-64.so.2\0";
        bytes[256..256 + interpreter.len()].copy_from_slice(interpreter);
        for (index, kind, offset, address, size) in [
            (0, elf::PT_INTERP, 256, 0, interpreter.len() as u64),
            (1, elf::PT_DYNAMIC, 320, 0, 64),
            (2, elf::PT_LOAD, 512, 0x1200, 256),
        ] {
            let start = 64 + index * 56;
            bytes[start..start + 4].copy_from_slice(&kind.to_le_bytes());
            for (field, value) in [(8, offset), (16, address), (32, size), (40, size)] {
                bytes[start + field..start + field + 8].copy_from_slice(&value.to_le_bytes());
            }
        }
        for (index, tag, value) in [
            (0, elf::DT_NEEDED, 1_u64),
            (1, elf::DT_STRTAB, 0x1200),
            (2, elf::DT_STRSZ, 256),
        ] {
            let start = 320 + index * 16;
            bytes[start..start + 8].copy_from_slice(&tag.to_le_bytes());
            bytes[start + 8..start + 16].copy_from_slice(&value.to_le_bytes());
        }
        bytes[513..513 + name.len()].copy_from_slice(name.as_bytes());
        bytes
    }

    #[test]
    fn linux_elf_native_machines_bind_interpreter_and_libraries() {
        for machine in [Machine::X86_64, Machine::Aarch64] {
            let bytes = fixture(machine, "libc.so.6");
            let image = inspect(&bytes, machine).unwrap();
            assert_eq!(
                image.interpreter.as_deref(),
                Some("/lib64/ld-linux-x86-64.so.2")
            );
            assert_eq!(image.needed, BTreeSet::from(["libc.so.6".into()]));
            let foreign = match machine {
                Machine::X86_64 => Machine::Aarch64,
                Machine::Aarch64 => Machine::X86_64,
            };
            assert!(inspect(&bytes, foreign).is_err());
        }
    }

    #[test]
    fn linux_elf_refuses_custom_search_audits_escaping_names_and_truncation() {
        let machine = Machine::X86_64;
        for name in [
            "../libc.so.6",
            "/tmp/libc.so.6",
            "$ORIGIN/libc.so.6",
            "libc",
            "",
        ] {
            assert!(inspect(&fixture(machine, name), machine).is_err(), "{name}");
        }
        let bytes = fixture(machine, "libc.so.6");
        for length in [0, 64, 128, 256, 512, 700] {
            assert!(inspect(&bytes[..length], machine).is_err());
        }
        for tag in [
            elf::DT_RPATH,
            elf::DT_RUNPATH,
            elf::DT_AUDIT,
            elf::DT_DEPAUDIT,
            elf::DT_FILTER,
            elf::DT_AUXILIARY,
        ] {
            let mut altered = bytes.clone();
            altered[320..328].copy_from_slice(&tag.to_le_bytes());
            assert!(inspect(&altered, machine).is_err());
        }
        let mut alternate_search = bytes;
        alternate_search[320..328].copy_from_slice(&elf::DT_FLAGS_1.to_le_bytes());
        alternate_search[328..336].copy_from_slice(&u64::from(elf::DF_1_NODEFLIB).to_le_bytes());
        assert!(inspect(&alternate_search, machine).is_err());
    }
}

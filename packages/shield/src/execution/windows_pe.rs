//! Bounded native AMD64/ARM64 PE imports; foreign architectures never inherit a load contract.
use super::Error;
use object::{
    endian::LittleEndian as LE,
    pe,
    read::pe::{ImageNtHeaders, ImageOptionalHeader, PeFile64},
};
use std::collections::BTreeSet;

#[derive(Clone, Copy)]
pub(super) enum Machine {
    Amd64,
    Arm64,
}

#[cfg(windows)]
pub(super) const fn native_machine() -> Option<Machine> {
    if cfg!(target_arch = "x86_64") {
        Some(Machine::Amd64)
    } else if cfg!(target_arch = "aarch64") {
        Some(Machine::Arm64)
    } else {
        None
    }
}

impl Machine {
    const fn header(self) -> u16 {
        match self {
            Self::Amd64 => pe::IMAGE_FILE_MACHINE_AMD64,
            Self::Arm64 => pe::IMAGE_FILE_MACHINE_ARM64,
        }
    }
}

fn name(bytes: &[u8]) -> Result<String, Error> {
    if bytes.is_empty()
        || bytes.len() > 128
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-~".contains(byte))
    {
        return Err(Error::Unsupported);
    }
    let name = std::str::from_utf8(bytes)
        .map_err(|_| Error::Unsupported)?
        .to_ascii_lowercase();
    if !std::path::Path::new(&name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
    {
        return Err(Error::Unsupported);
    }
    Ok(name)
}

pub(super) fn api_set(name: &str) -> bool {
    (name.starts_with("api-") || name.starts_with("ext-"))
        && std::path::Path::new(name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-.~".contains(&byte))
}

pub(super) fn dependencies(
    bytes: &[u8],
    executable: bool,
    machine: Machine,
) -> Result<BTreeSet<String>, Error> {
    let image = PeFile64::parse(bytes).map_err(|_| Error::Unsupported)?;
    let header = image.nt_headers();
    let flags = header.file_header().characteristics.get(LE);
    if header.file_header().machine.get(LE) != machine.header()
        || flags & pe::IMAGE_FILE_EXECUTABLE_IMAGE == 0
        || (flags & pe::IMAGE_FILE_DLL == 0) != executable
        || header.file_header().number_of_sections.get(LE) > 96
        || (executable && header.optional_header().subsystem() != pe::IMAGE_SUBSYSTEM_WINDOWS_CUI)
        || image
            .data_directory(pe::IMAGE_DIRECTORY_ENTRY_COM_DESCRIPTOR)
            .is_some_and(|directory| {
                directory.virtual_address.get(LE) != 0 || directory.size.get(LE) != 0
            })
    {
        return Err(Error::Unsupported);
    }
    let mut result = BTreeSet::new();
    if let Some(imports) = image.import_table().map_err(|_| Error::Unsupported)? {
        let mut descriptors = imports.descriptors().map_err(|_| Error::Unsupported)?;
        let mut count = 0;
        while let Some(descriptor) = descriptors.next().map_err(|_| Error::Unsupported)? {
            count += 1;
            if count > 256 {
                return Err(Error::Unsupported);
            }
            // Enumerate descriptors themselves: Object::imports omits libraries
            // whose only imported symbols use ordinals rather than names.
            result.insert(name(
                imports
                    .name(descriptor.name.get(LE))
                    .map_err(|_| Error::Unsupported)?,
            )?);
        }
    }
    if let Some(imports) = image
        .data_directories()
        .delay_load_import_table(bytes, &image.section_table())
        .map_err(|_| Error::Unsupported)?
    {
        let mut descriptors = imports.descriptors().map_err(|_| Error::Unsupported)?;
        let mut count = 0;
        while let Some(descriptor) = descriptors.next().map_err(|_| Error::Unsupported)? {
            count += 1;
            if count > 256 || descriptor.attributes.get(LE) != 1 {
                return Err(Error::Unsupported);
            }
            result.insert(name(
                imports
                    .name(descriptor.dll_name_rva.get(LE))
                    .map_err(|_| Error::Unsupported)?,
            )?);
        }
    }
    if let Some(exports) = image.export_table().map_err(|_| Error::Unsupported)? {
        if exports.addresses().len() > 16_384 {
            return Err(Error::Unsupported);
        }
        for address in exports.addresses() {
            if let Some(forward) = exports
                .forward_string(address.get(LE))
                .map_err(|_| Error::Unsupported)?
            {
                let end = forward
                    .iter()
                    .position(|byte| *byte == b'.')
                    .ok_or(Error::Unsupported)?;
                let mut library = forward[..end].to_vec();
                library.extend_from_slice(b".dll");
                result.insert(name(&library)?);
            }
        }
    }
    if result.len() > 256 || (executable && result.is_empty()) {
        return Err(Error::Unsupported);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(library: &str) -> Vec<u8> {
        let mut bytes = vec![0; 1024];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        for (offset, value) in [
            (0x84, 0x8664_u16),
            (0x86, 1),
            (0x94, 240),
            (0x96, 2),
            (0x98, 0x20b),
            (0xdc, 3),
        ] {
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        for (offset, value) in [
            (0xb8, 0x1000_u32),
            (0xbc, 0x200),
            (0xd0, 0x2000),
            (0xd4, 0x200),
            (0x104, 16),
            (0x110, 0x1000),
            (0x114, 40),
            (0x190, 0x200),
            (0x194, 0x1000),
            (0x198, 0x200),
            (0x19c, 0x200),
            (0x20c, 0x1040),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[0x188..0x18e].copy_from_slice(b".idata");
        bytes[0x240..0x240 + library.len()].copy_from_slice(library.as_bytes());
        bytes
    }

    #[test]
    fn windows_pe_descriptor_only_imports_are_retained_and_names_cannot_escape_system32() {
        assert_eq!(
            dependencies(&fixture("MSVCRT.dll"), true, Machine::Amd64).unwrap(),
            BTreeSet::from(["msvcrt.dll".into()])
        );
        for library in ["../evil.dll", r"C:\evil.dll", "evil/dll", "", "helper.exe"] {
            assert!(
                dependencies(&fixture(library), true, Machine::Amd64).is_err(),
                "{library}"
            );
        }
        for contract in [
            "api-ms-win-core-heap-l1-1-0.dll",
            "ext-ms-onecore-appmodel-deployment-internal-l1-1-0.dll",
            "api-win-core-sample~group.dll",
        ] {
            assert!(api_set(contract));
            assert!(
                dependencies(&fixture(contract), true, Machine::Amd64)
                    .unwrap()
                    .contains(contract)
            );
        }
        for ordinary in ["kernel32.dll", "ext-../evil.dll", "api-C:\\evil.dll"] {
            assert!(!api_set(ordinary));
        }
    }

    #[test]
    fn windows_pe_rejects_wrong_architecture_dlls_managed_images_and_truncation() {
        let bytes = fixture("kernel32.dll");
        for length in [0, 64, 128, 512, 600] {
            assert!(dependencies(&bytes[..length], true, Machine::Amd64).is_err());
        }
        for (offset, value) in [(0x84, 0xaa64_u16), (0x96, 0x2002), (0xdc, 2)] {
            let mut changed = bytes.clone();
            changed[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
            assert!(dependencies(&changed, true, Machine::Amd64).is_err());
        }
        let mut managed = bytes.clone();
        managed[0x178..0x17c].copy_from_slice(&0x1000_u32.to_le_bytes());
        managed[0x17c..0x180].copy_from_slice(&16_u32.to_le_bytes());
        assert!(dependencies(&managed, true, Machine::Amd64).is_err());
    }

    #[test]
    fn windows_pe_arm64_requires_matching_native_executables_and_libraries() {
        let amd64 = fixture("kernel32.dll");
        let mut arm64 = amd64.clone();
        arm64[0x84..0x86].copy_from_slice(&pe::IMAGE_FILE_MACHINE_ARM64.to_le_bytes());
        assert!(
            dependencies(&arm64, true, Machine::Arm64)
                .unwrap()
                .contains("kernel32.dll")
        );
        assert!(dependencies(&amd64, true, Machine::Arm64).is_err());
        assert!(dependencies(&arm64, true, Machine::Amd64).is_err());
        arm64[0x96..0x98].copy_from_slice(&0x2002_u16.to_le_bytes());
        assert!(dependencies(&arm64, false, Machine::Arm64).is_ok());
        assert!(dependencies(&arm64, false, Machine::Amd64).is_err());
        for foreign in [0x014c_u16, 0xa641, 0xa64e] {
            arm64[0x84..0x86].copy_from_slice(&foreign.to_le_bytes());
            assert!(dependencies(&arm64, false, Machine::Arm64).is_err());
        }
    }
}

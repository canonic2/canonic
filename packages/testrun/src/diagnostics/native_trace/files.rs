//! Private bounded native-file ownership. Archives are never extracted by Testrun.
use super::*;
use std::{
    fs::OpenOptions,
    io::{Read, Write},
};

pub(super) const MAX_ACTIVITY_BUNDLE: usize = 16 * 1024 * 1024;
pub(crate) fn read(path: &Path, max: usize) -> Result<Vec<u8>> {
    if !path.is_absolute() || !std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file()) {
        return Err(Error::invalid(
            "Select an absolute regular native file; symlinks and special files refused",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|_| Error::new("NATIVE_FILE", "Cannot open native artifact"))?;
    let before = file
        .metadata()
        .map_err(|_| Error::invalid("Cannot inspect native file"))?;
    if !before.is_file() || before.len() > max as u64 {
        return Err(Error::new(
            "NATIVE_TRACE_BOUNDS",
            "Native file exceeds bounded evidence size",
        ));
    }
    let mut bytes = vec![];
    (&file)
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::new("NATIVE_FILE", "Cannot read native artifact"))?;
    let after = file
        .metadata()
        .map_err(|_| Error::invalid("Cannot recheck native file"))?;
    if bytes.len() > max
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(Error::new(
            "NATIVE_FILE_CHANGED",
            "Native file changed during import",
        ));
    }
    Ok(bytes)
}
fn entries(root: &Path) -> Result<Vec<PathBuf>> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = vec![];
    let mut visited = 0;
    while let Some(dir) = pending.pop() {
        for item in std::fs::read_dir(&dir)
            .map_err(|_| Error::new("NATIVE_FILE", "Cannot read trace bundle"))?
        {
            let item = item.map_err(|_| Error::invalid("Cannot read trace entry"))?;
            visited += 1;
            if visited > 512 {
                return Err(Error::new(
                    "NATIVE_TRACE_BOUNDS",
                    "Trace bundle exceeds 512 entries",
                ));
            }
            let kind = item
                .file_type()
                .map_err(|_| Error::invalid("Cannot inspect trace entry"))?;
            if kind.is_dir() {
                pending.push(item.path());
            } else if kind.is_file() {
                files.push(item.path());
            } else {
                return Err(Error::invalid(
                    "Trace bundles refuse symlinks and special files",
                ));
            }
        }
    }
    files.sort();
    Ok(files)
}
pub(super) fn copy_input(
    source: &Path,
    destination: &Path,
    bundle_limit: Option<usize>,
) -> Result<()> {
    let Some(limit) = bundle_limit else {
        return crate::artifacts::write(destination, &read(source, MAX_ARTIFACT)?);
    };
    if !source.is_absolute() || !std::fs::symlink_metadata(source).is_ok_and(|m| m.is_dir()) {
        return Err(Error::invalid(
            "Select an absolute regular .trace directory",
        ));
    }
    crate::artifacts::directory(destination)?;
    let mut total = 0;
    for path in entries(source)? {
        let bytes = read(&path, limit - total)?;
        total += bytes.len();
        let target = destination.join(path.strip_prefix(source).unwrap());
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(target.parent().unwrap())
            .map_err(|_| Error::new("NATIVE_FILE", "Cannot create private trace directory"))?;
        crate::artifacts::write(&target, &bytes)?;
    }
    Ok(())
}
pub(super) fn artifact(
    path: &Path,
    bundle_limit: Option<usize>,
) -> Result<(Vec<u8>, &'static str)> {
    let Some(limit) = bundle_limit else {
        return Ok((read(path, MAX_ARTIFACT)?, "identity"));
    };
    let mut archive = tar::Builder::new(Vec::new());
    let mut total = 0;
    for file in entries(path)? {
        let bytes = read(&file, limit - total)?;
        total += bytes.len();
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o600);
        header.set_cksum();
        archive
            .append_data(
                &mut header,
                Path::new("capture.trace").join(file.strip_prefix(path).unwrap()),
                bytes.as_slice(),
            )
            .map_err(|_| Error::invalid("Cannot retain trace bundle"))?;
    }
    let bytes = archive
        .into_inner()
        .map_err(|_| Error::invalid("Cannot close trace archive"))?;
    if bytes.len() <= MAX_ARTIFACT {
        return Ok((bytes, "tar"));
    }
    if limit == MAX_ACTIVITY_BUNDLE && bytes.len() <= limit {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let compressed = encoder
            .write_all(&bytes)
            .and_then(|()| encoder.finish())
            .map_err(|_| Error::new("NATIVE_FILE", "Cannot compress the Activity Monitor trace"))?;
        if compressed.len() <= MAX_ARTIFACT {
            return Ok((compressed, "tar.gz"));
        }
    }
    Err(Error::new(
        "NATIVE_TRACE_BOUNDS",
        "Trace archive exceeds the 4 MiB retention limit",
    ))
}

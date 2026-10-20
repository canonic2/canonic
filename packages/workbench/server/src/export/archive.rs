//! Stored (uncompressed) ZIP archives, and the split of one export into
//! attachment-sized parts that each carry the README and the report. Pure.
use super::types::{Archive, ExportReport, Part, ZipEntry};
use anyhow::{Result, bail};

/// The largest archive part, in bytes: an attachment limit people meet.
pub(crate) const MAX_ARCHIVE_BYTES: usize = 10 * 1000 * 1000;

fn crc32(body: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for byte in body {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 == 1 { 0xedb8_8320 } else { 0 };
        }
    }
    crc ^ 0xffff_ffff
}

fn u16_of(value: usize) -> [u8; 2] {
    u16::try_from(value).unwrap_or(u16::MAX).to_le_bytes()
}

fn u32_of(value: usize) -> [u8; 4] {
    u32::try_from(value).unwrap_or(u32::MAX).to_le_bytes()
}

/// A ZIP of `entries`, stored, with UTF-8 names.
pub(crate) fn zip(entries: &[ZipEntry]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for entry in entries {
        let name = entry.name.replace('\\', "/");
        let name = name.as_bytes();
        let body = &entry.body;
        let crc = crc32(body).to_le_bytes();
        let offset = out.len();
        let mut header = [0_u8; 30];
        header[0..4].copy_from_slice(&0x0403_4b50_u32.to_le_bytes());
        header[4..6].copy_from_slice(&20_u16.to_le_bytes());
        header[6..8].copy_from_slice(&0x0800_u16.to_le_bytes());
        header[14..18].copy_from_slice(&crc);
        header[18..22].copy_from_slice(&u32_of(body.len()));
        header[22..26].copy_from_slice(&u32_of(body.len()));
        header[26..28].copy_from_slice(&u16_of(name.len()));
        out.extend_from_slice(&header);
        out.extend_from_slice(name);
        out.extend_from_slice(body);
        let mut record = [0_u8; 46];
        record[0..4].copy_from_slice(&0x0201_4b50_u32.to_le_bytes());
        record[4..6].copy_from_slice(&20_u16.to_le_bytes());
        record[6..8].copy_from_slice(&20_u16.to_le_bytes());
        record[8..10].copy_from_slice(&0x0800_u16.to_le_bytes());
        record[16..20].copy_from_slice(&crc);
        record[20..24].copy_from_slice(&u32_of(body.len()));
        record[24..28].copy_from_slice(&u32_of(body.len()));
        record[28..30].copy_from_slice(&u16_of(name.len()));
        record[42..46].copy_from_slice(&u32_of(offset));
        central.extend_from_slice(&record);
        central.extend_from_slice(name);
    }
    let central_at = out.len();
    let central_length = central.len();
    out.extend_from_slice(&central);
    let mut end = [0_u8; 22];
    end[0..4].copy_from_slice(&0x0605_4b50_u32.to_le_bytes());
    end[8..10].copy_from_slice(&u16_of(entries.len()));
    end[10..12].copy_from_slice(&u16_of(entries.len()));
    end[12..16].copy_from_slice(&u32_of(central_length));
    end[16..20].copy_from_slice(&u32_of(central_at));
    out.extend_from_slice(&end);
    out
}

/// The size `zip(entries)` will have.
pub(crate) fn zip_size(entries: &[ZipEntry]) -> usize {
    22 + entries
        .iter()
        .map(|entry| entry.body.len() + 76 + entry.name.replace('\\', "/").len() * 2)
        .sum::<usize>()
}

fn too_big(entry: &ZipEntry, prefix: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "“{}” cannot fit inside the 10 MB export ZIP limit",
        payload_name(entry, prefix)
    )
}

fn payload_name(entry: &ZipEntry, prefix: &str) -> String {
    entry
        .name
        .get(prefix.len() + 1..)
        .unwrap_or_default()
        .to_owned()
}

fn pretty(value: &impl serde::Serialize) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default() + "\n"
}

/// The payload in groups whose ZIPs, with the README and a report of the
/// largest size it can reach, stay within `maximum`.
fn grouped(
    prefix: &str,
    payload: Vec<ZipEntry>,
    readme: &str,
    base_report: &str,
    maximum: usize,
) -> Result<Vec<Vec<ZipEntry>>> {
    let provisional = |group: &[ZipEntry]| {
        let listed = pretty(&serde_json::json!({
            "part": 9999, "of": 9999,
            "files": group.iter().map(|entry| payload_name(entry, prefix)).collect::<Vec<_>>(),
        }));
        let mut entries = vec![
            ZipEntry::new(format!("{prefix}/README.md"), readme),
            ZipEntry::new(format!("{prefix}/canonic-export.json"), base_report),
            ZipEntry::new(format!("{prefix}/canonic-export-part-9999.json"), listed),
        ];
        entries.extend_from_slice(group);
        zip_size(&entries)
    };
    let mut groups: Vec<Vec<ZipEntry>> = Vec::new();
    let mut current: Vec<ZipEntry> = Vec::new();
    for entry in payload {
        current.push(entry);
        if current.len() > 1 && provisional(&current) > maximum {
            let entry = current.pop().expect("an entry was just added");
            groups.push(std::mem::take(&mut current));
            current.push(entry);
        }
        if provisional(&current) > maximum {
            return Err(too_big(&current[current.len() - 1], prefix));
        }
    }
    if !current.is_empty() || groups.is_empty() {
        groups.push(current);
    }
    Ok(groups)
}

/// One ZIP per group, each with the README, the report and its part list.
fn parts(
    prefix: &str,
    groups: &[Vec<ZipEntry>],
    readme: &str,
    report: &mut ExportReport,
) -> Vec<Archive> {
    let total = groups.len();
    let width = 2.max(total.to_string().len());
    let filename = |index: usize| {
        if total == 1 {
            format!("{prefix}.zip")
        } else {
            format!("{prefix}-part-{:0width$}-of-{total:0width$}.zip", index + 1)
        }
    };
    report.parts = Some(
        groups
            .iter()
            .enumerate()
            .map(|(index, group)| Part {
                number: index + 1,
                filename: filename(index),
                files: group.len(),
            })
            .collect(),
    );
    let report_body = pretty(report);
    let split_readme = format!(
        "{readme}\n## Archive parts\n\n{}",
        if total == 1 {
            "This export fits in one ZIP file.\n".to_owned()
        } else {
            format!(
                "This export is split into {total} ZIP files so every upload remains at or below 10 MB. Extract every part into the same directory; their shared top-level folder and repeated metadata are designed to merge.\n"
            )
        }
    );
    groups
        .iter()
        .enumerate()
        .map(|(index, group)| {
            let files: Vec<String> = group
                .iter()
                .map(|entry| payload_name(entry, prefix))
                .collect();
            let part = serde_json::json!({
                "part": index + 1, "of": total, "filename": filename(index), "files": files,
            });
            let mut entries = vec![
                ZipEntry::new(format!("{prefix}/README.md"), split_readme.as_str()),
                ZipEntry::new(
                    format!("{prefix}/canonic-export.json"),
                    report_body.as_str(),
                ),
                ZipEntry::new(
                    format!("{prefix}/canonic-export-part-{:0width$}.json", index + 1),
                    pretty(&part),
                ),
            ];
            entries.extend_from_slice(group);
            Archive {
                filename: filename(index),
                body: zip(&entries),
                content_type: "application/zip",
                files,
            }
        })
        .collect()
}

/// Groups `payload` under `prefix/` into ZIPs of at most `maximum` bytes,
/// each with the README, the report and its own part list. The report's
/// `parts` names them.
///
/// # Errors
/// A limit under 1024 bytes, or a file too big for any part.
pub(crate) fn split_archives(
    prefix: &str,
    payload: Vec<ZipEntry>,
    readme: &str,
    report: &mut ExportReport,
    maximum: Option<usize>,
) -> Result<Vec<Archive>> {
    let maximum = maximum
        .filter(|limit| *limit > 0)
        .unwrap_or(MAX_ARCHIVE_BYTES);
    if maximum < 1024 {
        bail!("Export ZIP limit must be at least 1024 bytes");
    }
    let reserve = 32_768.max(payload.len() * 160);
    let mut base = report.clone();
    base.parts = Some(Vec::new());
    let base_report = format!("{}{}", pretty(&base), " ".repeat(reserve));
    let mut groups = grouped(prefix, payload, readme, &base_report, maximum)?;
    loop {
        let archives = parts(prefix, &groups, readme, report);
        let Some(oversized) = archives
            .iter()
            .position(|archive| archive.body.len() > maximum)
        else {
            return Ok(archives);
        };
        if groups[oversized].len() <= 1 {
            return Err(too_big(&groups[oversized][0], prefix));
        }
        let moved = groups[oversized]
            .pop()
            .expect("the group has several entries");
        if let Some(next) = groups.get_mut(oversized + 1) {
            next.insert(0, moved);
        } else {
            groups.push(vec![moved]);
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    /// The names and bodies of a stored ZIP's entries, read from its local headers.
    pub(crate) fn read_zip(zip: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        let mut at = 0;
        let word = |at: usize| u32::from_le_bytes([zip[at], zip[at + 1], zip[at + 2], zip[at + 3]]);
        let half = |at: usize| usize::from(u16::from_le_bytes([zip[at], zip[at + 1]]));
        while at + 4 <= zip.len() && word(at) == 0x0403_4b50 {
            let size = word(at + 18) as usize;
            let name_length = half(at + 26);
            let extra = half(at + 28);
            let name = String::from_utf8_lossy(&zip[at + 30..at + 30 + name_length]).into_owned();
            let body_at = at + 30 + name_length + extra;
            out.push((name, zip[body_at..body_at + size].to_vec()));
            at = body_at + size;
        }
        out
    }

    pub(crate) fn names(zip: &[u8]) -> Vec<String> {
        read_zip(zip).into_iter().map(|(name, _)| name).collect()
    }

    pub(crate) fn report() -> ExportReport {
        ExportReport {
            request: None,
            name: Some("Acme".into()),
            generated_at: "2026-10-19T00:00:00.000Z".into(),
            selection: "space export".into(),
            files: Vec::new(),
            pages: Vec::new(),
            dependencies: Vec::new(),
            warnings: Vec::new(),
            capture_warnings: Vec::new(),
            browser: None,
            parts: None,
        }
    }

    #[test]
    fn a_zip_holds_its_entries_with_matching_sizes_and_checksums() {
        let entries = vec![
            ZipEntry::new("acme/a.txt", "Acme"),
            ZipEntry::new("acme\\b.bin", vec![0_u8, 1, 2]),
        ];
        let body = zip(&entries);
        assert_eq!(body.len(), zip_size(&entries));
        assert_eq!(
            read_zip(&body),
            [
                ("acme/a.txt".to_owned(), b"Acme".to_vec()),
                ("acme/b.bin".to_owned(), vec![0, 1, 2])
            ]
        );
        // The check value of the standard test input.
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn an_export_too_big_for_one_part_splits_into_parts_that_each_fit() {
        let payload: Vec<ZipEntry> = (0..4_u8)
            .map(|index| ZipEntry::new(format!("acme/shot-{index}.jpg"), vec![index; 25_000]))
            .collect();
        let mut report = report();
        let archives =
            split_archives("acme", payload, "# Acme", &mut report, Some(70_000)).expect("split");
        assert!(archives.len() > 1);
        assert_eq!(report.parts.as_ref().map(Vec::len), Some(archives.len()));
        for (index, archive) in archives.iter().enumerate() {
            assert!(archive.body.len() <= 70_000);
            assert!(
                archive
                    .filename
                    .contains(&format!("part-{:02}-of-", index + 1))
            );
            let names = names(&archive.body);
            assert!(names.contains(&"acme/README.md".to_owned()));
            assert!(names.contains(&"acme/canonic-export.json".to_owned()));
            assert!(
                names
                    .iter()
                    .any(|name| name.starts_with("acme/canonic-export-part-"))
            );
        }
        let all: Vec<String> = archives
            .iter()
            .flat_map(|archive| archive.files.clone())
            .collect();
        assert_eq!(
            all,
            ["shot-0.jpg", "shot-1.jpg", "shot-2.jpg", "shot-3.jpg"]
        );
    }

    #[test]
    fn a_small_export_is_one_archive_named_after_its_folder() {
        let mut report = report();
        let archives = split_archives(
            "acme",
            vec![ZipEntry::new("acme/a.txt", "a")],
            "# Acme",
            &mut report,
            None,
        )
        .expect("one part");
        assert_eq!(archives.len(), 1);
        assert_eq!(archives[0].filename, "acme.zip");
        let readme = &read_zip(&archives[0].body)[0].1;
        assert!(String::from_utf8_lossy(readme).contains("This export fits in one ZIP file."));
    }

    #[test]
    fn limits_and_files_too_big_for_any_part_are_errors() {
        let error = split_archives("acme", Vec::new(), "", &mut report(), Some(10))
            .expect_err("tiny limit");
        assert!(error.to_string().contains("Export ZIP limit"));
        let error = split_archives(
            "acme",
            vec![ZipEntry::new("acme/huge.bin", vec![0_u8; 50_000])],
            "",
            &mut report(),
            Some(40_000),
        )
        .expect_err("too big");
        assert_eq!(
            error.to_string(),
            "“huge.bin” cannot fit inside the 10 MB export ZIP limit"
        );
    }
}

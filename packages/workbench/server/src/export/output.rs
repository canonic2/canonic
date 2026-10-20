//! The visual exports: one PDF of every printed page, a folder of reference
//! images, and the portable browser viewer on its own, each as the archives a
//! download serves.
use super::archive::{split_archives, zip};
use super::paths::{iso_now, stem};
use super::request::ExportRequest;
use super::types::{Archive, ExportReport, ExportResult, PortableExport, Screenshot, ZipEntry};
use anyhow::{Result, anyhow, bail};
use base64::Engine as _;
use lopdf::{Dictionary, Document, Object, ObjectId};
use std::collections::BTreeMap;

fn report(name: &str, request: &ExportRequest, warnings: &[String]) -> ExportReport {
    let mut unique: Vec<String> = Vec::new();
    for warning in warnings {
        if !unique.contains(warning) {
            unique.push(warning.clone());
        }
    }
    ExportReport {
        request: Some(request.clone()),
        name: Some(name.to_owned()),
        generated_at: iso_now(),
        selection: format!("{} export", request.scope),
        files: Vec::new(),
        pages: Vec::new(),
        dependencies: Vec::new(),
        warnings: unique,
        capture_warnings: Vec::new(),
        browser: None,
        parts: None,
    }
}

fn package_files(
    name: &str,
    entries: Vec<ZipEntry>,
    mut report: ExportReport,
    max: Option<usize>,
) -> Result<ExportResult> {
    let readme = format!(
        "# {} export\n\nSee canonic-export.json for selection and warnings.\n",
        report.name.as_deref().unwrap_or_default()
    );
    let archives = split_archives(name, entries, &readme, &mut report, max)?;
    let bundle = (archives.len() > 1).then(|| Archive {
        filename: format!("{name}-parts.zip"),
        body: zip(&archives
            .iter()
            .map(|archive| ZipEntry::new(archive.filename.clone(), archive.body.clone()))
            .collect::<Vec<_>>()),
        content_type: "application/zip",
        files: Vec::new(),
    });
    Ok(ExportResult {
        archives,
        bundle,
        report,
    })
}

/// Attributes a page takes from its page tree when it doesn't set them.
const INHERITED: [&[u8]; 4] = [b"MediaBox", b"CropBox", b"Resources", b"Rotate"];

/// The pages of one printed document, each with its inherited attributes.
fn pages_of(document: &Document) -> Vec<(ObjectId, Dictionary)> {
    document
        .get_pages()
        .into_values()
        .filter_map(|id| {
            let mut page = document.get_dictionary(id).ok()?.clone();
            let mut parent = page.get(b"Parent").and_then(Object::as_reference).ok();
            while let Some(node) = parent.and_then(|id| document.get_dictionary(id).ok()) {
                for key in INHERITED {
                    if !page.has(key)
                        && let Ok(value) = node.get(key)
                    {
                        page.set(key, value.clone());
                    }
                }
                parent = node.get(b"Parent").and_then(Object::as_reference).ok();
            }
            Some((id, page))
        })
        .collect()
}

/// One PDF of every page of `documents`, in order, keeping each page's size.
fn merge(documents: &[&[u8]], title: &str, subject: &str) -> Result<Vec<u8>> {
    let mut merged = Document::with_version("1.7");
    let mut next = 1;
    let mut objects: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut pages: Vec<(ObjectId, Dictionary)> = Vec::new();
    for body in documents {
        let mut document = Document::load_mem(body)
            .map_err(|error| anyhow!("A printed page is not a PDF: {error}"))?;
        document.renumber_objects_with(next);
        next = document.max_id + 1;
        let own_pages = pages_of(&document);
        let skipped: Vec<ObjectId> = document
            .objects
            .iter()
            .filter(|(_, object)| {
                object
                    .as_dict()
                    .ok()
                    .and_then(|dict| dict.get(b"Type").ok())
                    .and_then(|kind| kind.as_name().ok())
                    .is_some_and(|kind| {
                        matches!(
                            kind,
                            b"Catalog" | b"Pages" | b"Outlines" | b"Outline" | b"Page"
                        )
                    })
            })
            .map(|(id, _)| *id)
            .collect();
        for (id, object) in document.objects {
            if !skipped.contains(&id) {
                objects.insert(id, object);
            }
        }
        pages.extend(own_pages);
    }
    let tree_id = (next, 0);
    next += 1;
    let catalog_id = (next, 0);
    next += 1;
    let info_id = (next, 0);
    merged.max_id = next;
    merged.objects = objects;
    for (id, mut page) in pages.iter().cloned() {
        page.set("Parent", tree_id);
        merged.objects.insert(id, Object::Dictionary(page));
    }
    let mut tree = Dictionary::new();
    tree.set("Type", Object::Name(b"Pages".to_vec()));
    tree.set("Count", i64::try_from(pages.len()).unwrap_or(i64::MAX));
    tree.set(
        "Kids",
        pages
            .iter()
            .map(|(id, _)| Object::Reference(*id))
            .collect::<Vec<_>>(),
    );
    merged.objects.insert(tree_id, Object::Dictionary(tree));
    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", tree_id);
    merged
        .objects
        .insert(catalog_id, Object::Dictionary(catalog));
    let mut info = Dictionary::new();
    info.set("Title", Object::string_literal(title));
    info.set("Subject", Object::string_literal(subject));
    merged.objects.insert(info_id, Object::Dictionary(info));
    merged.trailer.set("Root", catalog_id);
    merged.trailer.set("Info", info_id);
    merged.compress();
    let mut out = Vec::new();
    merged.save_to(&mut out)?;
    Ok(out)
}

/// One PDF of every printed reference.
///
/// # Errors
/// Nothing printed, or a printed page that is not a PDF.
pub(crate) fn pdf_output(
    name: &str,
    request: &ExportRequest,
    references: &[Screenshot],
    warnings: &[String],
) -> Result<ExportResult> {
    if references.is_empty() {
        bail!(
            "No pages could be rendered for this PDF. {}",
            warnings.join(" ")
        );
    }
    let bodies: Vec<&[u8]> = references
        .iter()
        .map(|reference| reference.body.as_slice())
        .collect();
    let body = merge(
        &bodies,
        name,
        &format!("Workbench {} export", request.scope),
    )?;
    let archive = Archive {
        filename: format!("{}.pdf", stem(name)),
        body,
        content_type: "application/pdf",
        files: Vec::new(),
    };
    Ok(ExportResult {
        archives: vec![archive],
        bundle: None,
        report: report(name, request, warnings),
    })
}

/// Every captured reference image, numbered in capture order.
///
/// # Errors
/// Nothing captured, or an image too big for any part.
pub(crate) fn images_output(
    name: &str,
    request: &ExportRequest,
    references: &[Screenshot],
    warnings: &[String],
    max: Option<usize>,
) -> Result<ExportResult> {
    if references.is_empty() {
        bail!(
            "No reference images could be captured. {}",
            warnings.join(" ")
        );
    }
    let prefix = format!("{}-images", stem(name));
    let extension = if request.image_format == "png" {
        "png"
    } else {
        "jpg"
    };
    let entries: Vec<ZipEntry> = references
        .iter()
        .enumerate()
        .map(|(index, reference)| {
            let variant = if reference.variant.is_empty() {
                &reference.label
            } else {
                &reference.variant
            };
            ZipEntry::new(
                format!(
                    "{prefix}/{:03}-{}-{}.{extension}",
                    index + 1,
                    stem(&reference.page),
                    stem(variant)
                ),
                reference.body.clone(),
            )
        })
        .collect();
    let mut report = report(name, request, warnings);
    report.files = entries
        .iter()
        .map(|entry| entry.name[prefix.len() + 1..].to_owned())
        .collect();
    package_files(&prefix, entries, report, max)
}

/// The portable viewer on its own.
///
/// # Errors
/// Nothing built for the selection, a file outside `browser/`, or one too big.
pub(crate) fn browser_output(
    name: &str,
    request: &ExportRequest,
    portable: Option<&PortableExport>,
    max: Option<usize>,
) -> Result<ExportResult> {
    let Some(portable) = portable.filter(|portable| {
        !portable.files.is_empty() && (!portable.previews.is_empty() || !portable.docs.is_empty())
    }) else {
        bail!("No portable browser previews could be built for this selection.");
    };
    let prefix = format!("{}-browser", stem(name));
    let mut report = report(name, request, &portable.warnings);
    let mut entries = Vec::new();
    for file in &portable.files {
        if !file.path.starts_with("browser/")
            || file.path.split('/').any(|part| part == ".." || part == ".")
        {
            bail!("Invalid portable export path");
        }
        let body = base64::engine::general_purpose::STANDARD
            .decode(&file.data)
            .map_err(|error| anyhow!("Invalid portable export file {}: {error}", file.path))?;
        entries.push(ZipEntry::new(format!("{prefix}/{}", file.path), body));
    }
    report.files = portable
        .files
        .iter()
        .map(|file| file.path.clone())
        .collect();
    package_files(&prefix, entries, report, max)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::export::archive::tests::names;
    use crate::export::read_request;
    use crate::export::types::PortableFile;
    use lopdf::content::Content;
    use serde_json::json;

    /// A PDF of one blank page of `width` × `height` points; `inherited` puts
    /// the size on the page tree, as some printers write it.
    pub(crate) fn blank_pdf(width: f32, height: f32, inherited: bool) -> Vec<u8> {
        let mut document = Document::with_version("1.5");
        let tree_id = document.new_object_id();
        let content = document.add_object(lopdf::Stream::new(
            Dictionary::new(),
            Content { operations: vec![] }.encode().unwrap_or_default(),
        ));
        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set("Parent", tree_id);
        page.set("Contents", content);
        let media: Vec<Object> = vec![
            0.into(),
            0.into(),
            Object::Real(width),
            Object::Real(height),
        ];
        if !inherited {
            page.set("MediaBox", media.clone());
        }
        let page_id = document.add_object(page);
        let mut tree = Dictionary::new();
        tree.set("Type", Object::Name(b"Pages".to_vec()));
        tree.set("Kids", vec![Object::Reference(page_id)]);
        tree.set("Count", 1);
        if inherited {
            tree.set("MediaBox", media);
        }
        document.objects.insert(tree_id, Object::Dictionary(tree));
        let mut catalog = Dictionary::new();
        catalog.set("Type", Object::Name(b"Catalog".to_vec()));
        catalog.set("Pages", tree_id);
        let catalog_id = document.add_object(catalog);
        document.trailer.set("Root", catalog_id);
        let mut out = Vec::new();
        document.save_to(&mut out).expect("pdf");
        out
    }

    pub(crate) fn page_sizes(body: &[u8]) -> Vec<(f32, f32)> {
        let document = Document::load_mem(body).expect("merged pdf");
        pages_of(&document)
            .into_iter()
            .map(|(_, page)| {
                let media = page
                    .get(b"MediaBox")
                    .and_then(Object::as_array)
                    .expect("media box");
                let number = |object: &Object| object.as_float().expect("number");
                (number(&media[2]), number(&media[3]))
            })
            .collect()
    }

    fn shot(body: Vec<u8>, page: &str, variant: &str) -> Screenshot {
        Screenshot {
            page: page.into(),
            variant: variant.into(),
            label: "Page".into(),
            body,
            ..Screenshot::default()
        }
    }

    #[test]
    fn pdf_merging_preserves_custom_visual_and_standard_documentation_page_sizes() {
        let references = vec![
            shot(blank_pdf(600.0, 4000.0, false), "a", ""),
            shot(blank_pdf(595.0, 842.0, true), "a", ""),
            shot(blank_pdf(612.0, 792.0, false), "a", ""),
        ];
        let request = read_request(&json!({ "format": "pdf" })).expect("request");
        let result = pdf_output(
            "Designs",
            &request,
            &references,
            &["One unavailable page".into()],
        )
        .expect("pdf");
        assert_eq!(
            page_sizes(&result.download().body),
            [(600.0, 4000.0), (595.0, 842.0), (612.0, 792.0)]
        );
        assert_eq!(result.download().content_type, "application/pdf");
        assert_eq!(result.download().filename, "designs.pdf");
        assert_eq!(result.report.warnings.len(), 1);
        let merged = Document::load_mem(&result.download().body).expect("merged");
        let info = merged
            .trailer
            .get(b"Info")
            .and_then(Object::as_reference)
            .and_then(|id| merged.get_dictionary(id))
            .expect("info");
        assert_eq!(
            info.get(b"Title").and_then(Object::as_str).expect("title"),
            b"Designs"
        );
    }

    #[test]
    fn nothing_printed_and_not_a_pdf_are_errors() {
        let request = read_request(&json!({ "format": "pdf" })).expect("request");
        let error =
            pdf_output("Designs", &request, &[], &["Card failed.".into()]).expect_err("empty");
        assert_eq!(
            error.to_string(),
            "No pages could be rendered for this PDF. Card failed."
        );
        assert!(pdf_output("Designs", &request, &[shot(b"jpeg".to_vec(), "a", "")], &[]).is_err());
    }

    #[test]
    fn images_are_numbered_by_page_and_variant() {
        let request =
            read_request(&json!({ "format": "images", "imageFormat": "jpeg" })).expect("request");
        let result = images_output(
            "Acme UI",
            &request,
            &[
                shot(b"one".to_vec(), "pages/Home.html", "default-mobile"),
                shot(b"two".to_vec(), "card", ""),
            ],
            &[],
            None,
        )
        .expect("images");
        assert_eq!(
            result.report.files,
            [
                "001-pages-home-html-default-mobile.jpg",
                "002-card-page.jpg"
            ]
        );
        assert!(
            names(&result.download().body)
                .contains(&"acme-ui-images/001-pages-home-html-default-mobile.jpg".to_owned())
        );
        assert_eq!(result.download().filename, "acme-ui-images.zip");
    }

    #[test]
    fn the_browser_export_ships_only_built_viewer_files() {
        let request = read_request(&json!({ "format": "browser" })).expect("request");
        let encoded = base64::engine::general_purpose::STANDARD.encode("viewer");
        let built = PortableExport {
            files: vec![PortableFile {
                path: "browser/index.html".into(),
                data: encoded.clone(),
            }],
            previews: vec![crate::export::PortablePreview::default()],
            warnings: vec!["Card: build failed".into()],
            ..PortableExport::default()
        };
        let result = browser_output("Acme", &request, Some(&built), None).expect("browser");
        assert_eq!(result.report.files, ["browser/index.html"]);
        assert_eq!(result.report.warnings, ["Card: build failed"]);
        assert!(
            names(&result.download().body).contains(&"acme-browser/browser/index.html".to_owned())
        );
        let empty = PortableExport {
            previews: vec![],
            ..built.clone()
        };
        assert!(browser_output("Acme", &request, Some(&empty), None).is_err());
        assert!(browser_output("Acme", &request, None, None).is_err());
        let outside = PortableExport {
            files: vec![PortableFile {
                path: "browser/./x".into(),
                data: encoded,
            }],
            ..built
        };
        assert_eq!(
            browser_output("Acme", &request, Some(&outside), None)
                .expect_err("path")
                .to_string(),
            "Invalid portable export path"
        );
    }
}

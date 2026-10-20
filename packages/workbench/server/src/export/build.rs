//! The source export with portable previews: the previews are built first,
//! and a build that fails becomes a warning, never a failed export.
use super::source::create;
use super::types::{ExportOptions, ExportResult, ExportView, PortableExport};
use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;

/// The pages, states and sizes a portable build covers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Selection {
    pub(crate) pages: Vec<String>,
    pub(crate) states: Vec<String>,
    pub(crate) sizes: Vec<String>,
}

/// The native compiler's portable build of a space's previews and docs pages.
#[async_trait]
pub(crate) trait PortableBuilder: Send + Sync {
    async fn export(&self, selection: Option<Selection>) -> Result<PortableExport>;
}

/// The source export, with portable previews from `previews` when given.
///
/// # Errors
/// Archive assembly failures; a failed portable build is only a warning.
pub(crate) async fn build(
    root: &Path,
    view: &ExportView,
    mut options: ExportOptions,
    previews: Option<&dyn PortableBuilder>,
) -> Result<ExportResult> {
    if let Some(previews) = previews {
        let selection = options.request.as_ref().map(|request| Selection {
            pages: view.pages.iter().map(|(src, _)| src.clone()).collect(),
            states: request.states.clone(),
            sizes: request.sizes.clone(),
        });
        match previews.export(selection).await {
            Ok(portable) => options.portable = Some(portable),
            Err(error) => {
                options.portable = None;
                options
                    .warnings
                    .push(format!("Portable browser previews: {error:#}"));
            }
        }
    }
    create(root, view, &options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::archive::tests::names;
    use crate::export::source::tests::Project;
    use crate::export::types::{CodeEntry, ExportPage, PortableFile, Screenshot};
    use anyhow::anyhow;
    use base64::Engine as _;

    struct Failing;

    #[async_trait]
    impl PortableBuilder for Failing {
        async fn export(&self, _selection: Option<Selection>) -> Result<PortableExport> {
            Err(anyhow!("Preview worker stopped"))
        }
    }

    struct Viewer;

    #[async_trait]
    impl PortableBuilder for Viewer {
        async fn export(&self, _selection: Option<Selection>) -> Result<PortableExport> {
            Ok(PortableExport {
                files: vec![PortableFile {
                    path: "browser/index.html".into(),
                    data: base64::engine::general_purpose::STANDARD.encode("viewer"),
                }],
                warnings: vec!["Card: build failed".into()],
                ..PortableExport::default()
            })
        }
    }

    #[tokio::test]
    async fn a_failed_portable_build_still_exports_sources_and_reference_screenshots_with_its_cause()
     {
        let project = Project::new("fallback");
        project.write("workbench.yaml", "name: Acme\n");
        let button = project.write("button.html", "<button>Acme</button>");
        let view = ExportView {
            name: Some("Acme".into()),
            implementations: vec![],
            pages: vec![(
                "button".into(),
                ExportPage {
                    label: "Button".into(),
                    design: Some(button),
                    code: Vec::<CodeEntry>::new(),
                },
            )],
        };
        let options = ExportOptions {
            screenshots: vec![Screenshot {
                page: "button".into(),
                width: 393,
                height: 852,
                body: b"jpeg".to_vec(),
                ..Screenshot::default()
            }],
            ..ExportOptions::default()
        };
        let result = build(&project.0, &view, options, Some(&Failing))
            .await
            .expect("export");
        let archived = names(&result.download().body);
        assert!(archived.contains(&"acme-design-system/button.html".to_owned()));
        assert!(
            archived
                .iter()
                .any(|name| name.rsplit('.').next() == Some("jpg"))
        );
        assert!(result.report.browser.is_none());
        assert_eq!(
            result.report.warnings,
            ["Portable browser previews: Preview worker stopped"]
        );
        assert_eq!(result.report.capture_warnings, Vec::<String>::new());
    }

    #[tokio::test]
    async fn successful_portable_builds_retain_their_files_and_individual_preview_warnings() {
        let project = Project::new("viewer");
        project.write("workbench.yaml", "name: Acme\n");
        let product = Project::new("viewer-product");
        product.write("sdk/api.d.ts", "export {};");
        let options = ExportOptions {
            product: Some(product.0.clone()),
            ..ExportOptions::default()
        };
        let view = ExportView {
            name: Some("Acme".into()),
            ..ExportView::default()
        };
        let result = build(&project.0, &view, options, Some(&Viewer))
            .await
            .expect("export");
        assert!(
            names(&result.download().body)
                .contains(&"acme-design-system/browser/index.html".to_owned())
        );
        assert_eq!(
            result
                .report
                .browser
                .as_ref()
                .map(|browser| browser.warnings.clone()),
            Some(vec!["Card: build failed".to_owned()])
        );
        assert_eq!(result.report.warnings, Vec::<String>::new());
    }

    #[tokio::test]
    async fn portable_fallback_does_not_hide_an_archive_assembly_failure() {
        let project = Project::new("assembly");
        let options = ExportOptions {
            max_archive_bytes: Some(1),
            ..ExportOptions::default()
        };
        let error = build(&project.0, &ExportView::default(), options, Some(&Failing))
            .await
            .expect_err("limit");
        assert!(error.to_string().contains("Export ZIP limit"));
    }
}

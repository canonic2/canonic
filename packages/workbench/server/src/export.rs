//! The design export: a space's sources, reference screenshots and portable
//! previews as ZIP archives, a PDF or images, assembled from the resolved
//! configuration. Capture and preview compilation are supplied by the
//! caller ([`Capturer`], [`PortableBuilder`], [`ExportHost`]); this module
//! decides what is captured, what is archived and how jobs are reported.
mod archive;
mod build;
mod capture;
mod jobs;
mod output;
mod paths;
mod pipeline;
mod plan;
mod references;
mod request;
mod selection;
pub(crate) mod sizes;
mod source;
mod types;

pub(crate) use build::{PortableBuilder, Selection};
pub(crate) use capture::{CapturePool, CaptureWorker, Capturer};
pub(crate) use jobs::{Jobs, archive_part};
pub(crate) use pipeline::{DocsLensPlan, DocsPagePlan, ExportHost, ExportInput, Story, start};
pub(crate) use request::{read_request, select_view};
pub(crate) use types::{Archive, PortableExport, PortableFile, PortablePreview};

//! Verified fixture records and owned chooser/download metadata.
use super::{contract::Reference, objects::ObjectRef, session::Error};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

pub const FIXTURE_BYTES: usize = 4 * 1024 * 1024;
const FIXTURE_BASE64_BYTES: usize = FIXTURE_BYTES.div_ceil(3) * 4;
pub(crate) const LEGACY_FIXTURE_BASE64_BYTES: usize = (256_usize * 1024).div_ceil(3) * 4;
// A full base64 fixture plus MCP, manager, or CDP framing and metadata.
pub(crate) const FIXTURE_FRAME_BYTES: usize = 6 * 1024 * 1024;
pub(crate) fn argument_byte_limit(tool: &str) -> usize {
    match tool {
        "browser_send" | "act" => FIXTURE_FRAME_BYTES,
        _ => canonic_mcp_kit::schema::MAX_VALUE_BYTES,
    }
}

pub(crate) fn fixture_path_guidance(tool: &str, args: &Value) -> crate::error::Result<()> {
    let (request, batch, discriminator) = match tool {
        "browser_send" => (
            args.get("request"),
            &args["request"]["requests"],
            "operation",
        ),
        "act" => (None, &args["actions"], "action"),
        _ => return Ok(()),
    };
    let batch = batch.as_array().into_iter().flatten().take(8);
    let supplied = request.into_iter().chain(batch).any(|request| {
        request[discriminator] == "fixture_register" && request["fixture"].get("path").is_some()
    });
    if supplied {
        return Err(crate::error::Error::invalid(
            "fixture_register accepts fixture.dataBase64. To register a local file, use testrun browser fixture-register PATH",
        ));
    }
    Ok(())
}
pub const DOWNLOAD_BYTES: usize = 8 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    SyntheticGenerated,
    BlankTemplate,
    SeedAsset,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clearance {
    pub by: String,
    pub method: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Registration {
    pub name: String,
    pub mime_type: String,
    pub data_base64: String,
    pub provenance: Provenance,
    pub cleared: Clearance,
}
impl Registration {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.name.is_empty()
            || self.name.len() > 128
            || self.name.contains(['/', '\\', '\0'])
            || matches!(self.name.as_str(), "." | "..")
            || self.mime_type.len() > 128
            || !self.mime_type.contains('/')
            || !self
                .mime_type
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/.-+".contains(&b))
            || self.data_base64.len() > FIXTURE_BASE64_BYTES
            || self.cleared.by.is_empty()
            || self.cleared.by.len() > 256
            || self.cleared.method.is_empty()
            || self.cleared.method.len() > 1024
        {
            return Err("INVALID_FIXTURE");
        }
        Ok(())
    }
}
#[derive(Clone)]
pub(crate) struct Fixture {
    pub path: PathBuf,
    pub summary: Value,
}
impl Fixture {
    /// Publish a validated registration, including the driver's directory flush when requested.
    pub(crate) async fn store(
        root: &Path,
        reference: Reference,
        input: Registration,
        flush_directory: bool,
    ) -> Result<Self, Error> {
        let bytes = STANDARD
            .decode(input.data_base64)
            .map_err(|_| "INVALID_FIXTURE")?;
        if bytes.len() > FIXTURE_BYTES {
            return Err("FIXTURE_BOUNDS".into());
        }
        let directory = root.join("fixtures");
        let path = directory.join(format!("{}.bin", reference.id));
        let summary = json!({
            "reference":reference,"name":input.name,"mimeType":input.mime_type,
            "bytes":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),
            "provenance":input.provenance,"cleared":input.cleared
        });
        tokio::task::spawn_blocking(move || {
            if !directory.exists() {
                crate::artifacts::directory(&directory)
                    .map_err(|error| Error::caused("FIXTURE_IO", error))?;
            }
            crate::artifacts::write(&path, &bytes)
                .map_err(|error| Error::caused("FIXTURE_IO", error))?;
            crate::artifacts::write(
                &directory.join(format!("{}.json", reference.id)),
                &serde_json::to_vec(&summary).map_err(|_| "FIXTURE_IO")?,
            )
            .map_err(|error| Error::caused("FIXTURE_IO", error))?;
            if flush_directory {
                crate::artifacts::sync_directory(&directory)
                    .map_err(|error| Error::caused("FIXTURE_IO", error))?;
            }
            Ok(Self { path, summary })
        })
        .await
        .map_err(|_| "FIXTURE_IO")?
    }

    pub(crate) async fn read(self) -> Result<(Value, Vec<u8>), Error> {
        let Self { path, summary } = self;
        tokio::task::spawn_blocking(move || {
            let metadata =
                fs::symlink_metadata(&path).map_err(|error| Error::caused("FIXTURE_IO", error))?;
            if !metadata.is_file() || metadata.len() > FIXTURE_BYTES as u64 {
                return Err("FIXTURE_CHANGED".into());
            }
            let mut bytes = Vec::new();
            fs::File::open(path)
                .map_err(|error| Error::caused("FIXTURE_IO", error))?
                .take(FIXTURE_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| Error::caused("FIXTURE_IO", error))?;
            if summary["bytes"] != bytes.len()
                || summary["sha256"] != format!("{:x}", Sha256::digest(&bytes))
            {
                return Err("FIXTURE_CHANGED".into());
            }
            Ok((summary, bytes))
        })
        .await
        .map_err(|_| "FIXTURE_IO")?
    }
}

pub(crate) async fn payloads<F>(
    fixtures: Vec<Reference>,
    read: impl Fn(Reference) -> F,
) -> Result<(Vec<Value>, Vec<Value>), Error>
where
    F: Future<Output = Result<(Value, Vec<u8>), Error>>,
{
    let mut payloads = Vec::new();
    let mut summaries = Vec::new();
    let mut total = 0;
    for fixture in fixtures {
        let (summary, bytes) = read(fixture).await?;
        total += bytes.len();
        if total > FIXTURE_BYTES {
            return Err("FIXTURE_BOUNDS".into());
        }
        // Insert the buffer directly: json! would copy the encoded fixture.
        let mut payload = json!({"name":summary["name"],"mimeType":summary["mimeType"]});
        payload["buffer"] = Value::String(STANDARD.encode(bytes));
        payloads.push(payload);
        summaries.push(summary);
    }
    Ok((payloads, summaries))
}

pub(crate) struct Chooser {
    pub page: Reference,
    pub multiple: bool,
}
pub(crate) struct Download {
    pub wire: ObjectRef,
    pub incoming: PathBuf,
    pub summary: Value,
}
#[derive(Default)]
pub(crate) struct Transfers {
    pub directories: BTreeMap<Reference, super::directory::Directory>,
    pub fixtures: BTreeMap<Reference, Fixture>,
    pub choosers: BTreeMap<Reference, Chooser>,
    pub downloads: BTreeMap<Reference, Download>,
    /// The latest downloads and choosers refused on arrival, newest last.
    pub refused: VecDeque<Value>,
}
/// Downloads held at once; deleted records make room when it is reached.
pub const DOWNLOADS: usize = 32;
const REFUSED: usize = 16;
impl Transfers {
    pub fn snapshot(&self) -> Value {
        json!({"fixtures":self.fixtures.values().map(|r|r.summary.clone()).chain(self.directories.values().map(|directory|directory.summary.clone())).collect::<Vec<_>>(),"choosers":self.choosers.iter().map(|(reference,c)|json!({"reference":reference,"page":c.page,"multiple":c.multiple})).collect::<Vec<_>>(),"downloads":self.downloads.values().map(|r|r.summary.clone()).collect::<Vec<_>>(),"refused":self.refused})
    }
    /// Record a download or chooser refused on arrival; the session goes on.
    pub fn refuse(&mut self, refusal: Value) {
        if self.refused.len() == REFUSED {
            self.refused.pop_front();
        }
        self.refused.push_back(refusal);
    }
    /// Whether another download fits, freeing deleted records first.
    pub fn download_room(&mut self) -> bool {
        while self.downloads.len() >= DOWNLOADS {
            let Some(deleted) = self
                .downloads
                .iter()
                .find(|(_, download)| download.summary["state"] == "deleted")
                .map(|(reference, _)| *reference)
            else {
                return false;
            };
            self.downloads.remove(&deleted);
        }
        true
    }
}

pub(crate) fn schemas(reference: &Value) -> Vec<Value> {
    let string = |max| json!({"type":"string","minLength":1,"maxLength":max});
    let cleared = object(
        json!({"by":string(256),"method":string(1024)}),
        &["by", "method"],
    );
    let mut registration = object(
        json!({"name":string(128),"mimeType":string(128),"dataBase64":{"type":"string","maxLength":FIXTURE_BASE64_BYTES},"provenance":{"enum":["synthetic_generated","blank_template","seed_asset"]},"cleared":cleared}),
        &["name", "mimeType", "dataBase64", "provenance", "cleared"],
    );
    registration["description"] = json!(
        "Register up to 4 MiB of cleared fixture bytes as dataBase64, with provenance and clearance. To import a local file path, use testrun browser fixture-register PATH."
    );
    let fixtures = json!({"type":"array","maxItems":16,"items":reference,
        "description":"Registered fixture references; attaching files does not prove server upload"});
    let mut result = vec![object(
        json!({"operation":{"const":"fixture_register"},"fixture":registration}),
        &["operation", "fixture"],
    )];
    result.push(object(json!({"operation":{"const":"directory_register"},"name":string(128),"entries":{"type":"array","minItems":1,"maxItems":16,"items":object(json!({"path":string(512),"fixture":reference}),&["path","fixture"])}}),&["operation","name","entries"]));
    for operation in ["fixture_list", "download_list"] {
        result.push(object(
            json!({"operation":{"const":operation}}),
            &["operation"],
        ));
    }
    for operation in [
        "fixture_verify",
        "download_status",
        "download_cancel",
        "download_delete",
    ] {
        let field = if operation.starts_with("fixture") {
            "fixture"
        } else {
            "download"
        };
        let mut props = json!({"operation":{"const":operation}});
        props[field] = reference.clone();
        result.push(object(props, &["operation", field]));
    }
    result.push(object(json!({"operation":{"const":"page_upload"},"page":reference,"frame":reference,"locator":{"$ref":"#/$defs/browserLocator"},"fixtures":fixtures}),&["operation","page","locator","fixtures"]));
    result.push(object(
        json!({"operation":{"const":"chooser_resolve"},"chooser":reference,"fixtures":fixtures}),
        &["operation", "chooser", "fixtures"],
    ));
    result
}
fn object(props: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":props,"required":required,"additionalProperties":false})
}

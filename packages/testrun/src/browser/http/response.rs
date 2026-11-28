//! Decode HTTP content coding independently of transfer framing and evidence I/O.
use crate::browser::{artifacts::Artifact, session::Error};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{self, Read};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyFormat {
    Base64,
    Text,
    Json,
    JsonValue,
}

pub(crate) fn read(
    artifact: &Artifact,
    format: BodyFormat,
    max_bytes: usize,
) -> Result<Value, Error> {
    if !(1..=65536).contains(&max_bytes) {
        return Err("HTTP_RESPONSE_RESULT_BOUNDS".into());
    }
    let bytes = artifact.read()?;
    let value = match format {
        BodyFormat::Base64 => {
            use base64::Engine as _;
            json!(base64::engine::general_purpose::STANDARD.encode(&bytes))
        }
        BodyFormat::Text => json!(String::from_utf8_lossy(&bytes)),
        BodyFormat::Json | BodyFormat::JsonValue => {
            let graph = super::ecmascript_json::parse(&bytes).map_err(json_error)?;
            if matches!(format, BodyFormat::JsonValue) {
                graph.project(max_bytes).map_err(json_error)?
            } else {
                graph.plain().map_err(|cause| {
                let mut error=json_error(cause);
                if cause==super::ecmascript_json::Error::NonJson {
                    error.partial=Some(json!({"evidenceId":artifact.id,"format":"json_value","nextAction":"read-same-evidence-with-json_value","private":true,"replayed":false}));
                }
                error
            })?
            }
        }
    };
    let result = json!({"evidenceId":artifact.id,"format":format,"value":value,"bodyBytes":bytes.len(),"private":true,"replayed":false});
    crate::browser::schema::encoded_len(&result, max_bytes)
        .map_err(|_| Error::from("HTTP_RESPONSE_RESULT_BOUNDS"))?;
    Ok(result)
}
fn json_error(error: super::ecmascript_json::Error) -> Error {
    match error {
        super::ecmascript_json::Error::Bounds => "HTTP_RESPONSE_RESULT_BOUNDS",
        _ => "HTTP_RESPONSE_JSON",
    }
    .into()
}

pub(super) struct Body {
    pub bytes: Vec<u8>,
    pub decoded: bool,
}

pub(super) fn decode(bytes: Vec<u8>, encoding: &str, limit: usize) -> Result<Body, &'static str> {
    // Like the pinned request client, decode only a single recognized coding.
    // Unknown or stacked codings retain their bytes and are identified by metadata.
    let coding = encoding.to_ascii_lowercase();
    let decoded = matches!(coding.as_str(), "gzip" | "x-gzip" | "deflate" | "br");
    if !decoded || bytes.is_empty() {
        if bytes.len() > limit {
            return Err("HTTP_BODY_BOUNDS");
        }
        return Ok(Body { bytes, decoded });
    }
    if coding == "deflate" {
        let mut decoder = flate2::Decompress::new(true);
        let mut output = vec![0; limit + 1];
        let status = decoder.decompress(&bytes, &mut output, flate2::FlushDecompress::Finish);
        let length = decoder.total_out() as usize;
        if length > limit {
            return Err("HTTP_BODY_BOUNDS");
        }
        if !matches!(status, Ok(flate2::Status::StreamEnd)) {
            return Err("HTTP_BODY_DECODE");
        }
        output.truncate(length);
        return Ok(Body {
            bytes: output,
            decoded: true,
        });
    }
    let reader: Box<dyn Read + '_> = match coding.as_str() {
        "gzip" | "x-gzip" => Box::new(flate2::read::MultiGzDecoder::new(bytes.as_slice())),
        "br" => Box::new(brotli_decompressor::Decompressor::new(
            bytes.as_slice(),
            4096,
        )),
        _ => unreachable!(),
    };
    let mut output = Vec::new();
    let result = reader.take(limit as u64 + 1).read_to_end(&mut output);
    if output.len() > limit {
        return Err("HTTP_BODY_BOUNDS");
    }
    // Gzip's pinned Z_SYNC_FLUSH accepts a stream ending before its trailer.
    // Invalid headers, checksums and corrupt compressed data still refuse.
    if let Err(error) = result
        && !(matches!(coding.as_str(), "gzip" | "x-gzip")
            && error.kind() == io::ErrorKind::UnexpectedEof)
    {
        return Err("HTTP_BODY_DECODE");
    }
    Ok(Body {
        bytes: output,
        decoded,
    })
}

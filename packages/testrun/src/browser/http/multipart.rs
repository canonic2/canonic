//! Bounded multipart entities with one boundary shared by headers and bytes.
use super::{Error, MAX_BODY};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Part {
    Text {
        name: String,
        value: String,
    },
    File {
        name: String,
        filename: String,
        #[serde(default, rename = "mimeType")]
        mime_type: String,
        data: String,
    },
    FilePath {
        name: String,
        path: String,
        #[serde(default)]
        filename: Option<String>,
        #[serde(default, rename = "mimeType")]
        mime_type: String,
    },
}
fn quoted(value: &str) -> String {
    value
        .replace('\\', "%5C")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
        .replace('"', "%22")
}
fn append(body: &mut Vec<u8>, bytes: &[u8]) -> Result<(), Error> {
    if body.len().saturating_add(bytes.len()) > MAX_BODY {
        return Err("HTTP_REQUEST_BODY_BOUNDS".into());
    }
    body.extend_from_slice(bytes);
    Ok(())
}
pub(super) fn encode(parts: &[Part]) -> Result<(Vec<u8>, Option<String>), Error> {
    encode_inner(parts, false)
}
pub(super) fn validate(parts: &[Part]) -> Result<(), Error> {
    encode_inner(parts, true).map(|_| ())
}
pub(super) fn filename<'a>(path: &'a str, filename: Option<&'a str>) -> Result<&'a str, Error> {
    if path.is_empty()
        || path.len() > 4096
        || path.chars().any(char::is_control)
        || !std::path::Path::new(path).is_absolute()
    {
        return Err("HTTP_FILE_PATH_INVALID".into());
    }
    filename
        .or_else(|| {
            std::path::Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
        })
        .ok_or("HTTP_FILE_PATH_INVALID".into())
}
fn file_mime<'a>(filename: &str, mime_type: &'a str) -> Result<&'a str, Error> {
    if filename.len() > 1024 || mime_type.len() > 256 {
        return Err("HTTP_REQUEST_BODY_BOUNDS".into());
    }
    if !mime_type.is_ascii() || mime_type.bytes().any(|byte| byte < 32 || byte == 127) {
        return Err("HTTP_MULTIPART_INVALID".into());
    }
    Ok(if mime_type.is_empty() {
        super::mime::infer(filename)
    } else {
        mime_type
    })
}
fn encode_inner(parts: &[Part], metadata_only: bool) -> Result<(Vec<u8>, Option<String>), Error> {
    if parts.len() > 128 {
        return Err("HTTP_REQUEST_BODY_BOUNDS".into());
    }
    let boundary = format!("testrun-{}", uuid::Uuid::new_v4().simple());
    let mut body = Vec::new();
    for part in parts {
        let (name, filename, mime, bytes): (&String, Option<&str>, Option<&str>, Vec<u8>) =
            match part {
                Part::Text { name, value } => {
                    if value.len() > MAX_BODY {
                        return Err("HTTP_REQUEST_BODY_BOUNDS".into());
                    }
                    (name, None, None, value.as_bytes().to_vec())
                }
                Part::File {
                    name,
                    filename,
                    mime_type,
                    data,
                } => {
                    if data.len() > MAX_BODY.div_ceil(3) * 4 {
                        return Err("HTTP_REQUEST_BODY_BOUNDS".into());
                    }
                    let bytes = STANDARD
                        .decode(data)
                        .map_err(|_| Error::from("HTTP_REQUEST_BODY_INVALID"))?;
                    let mime = file_mime(filename, mime_type)?;
                    (name, Some(filename), Some(mime), bytes)
                }
                Part::FilePath {
                    name,
                    path,
                    filename: selected_name,
                    mime_type,
                } => {
                    let filename = filename(path, selected_name.as_deref())?;
                    let mime = file_mime(filename, mime_type)?;
                    if !metadata_only {
                        return Err("HTTP_FILE_PREPARATION_REQUIRED".into());
                    }
                    (name, Some(filename), Some(mime), Vec::new())
                }
            };
        if name.len() > 1024 {
            return Err("HTTP_REQUEST_BODY_BOUNDS".into());
        }
        if bytes
            .windows(boundary.len())
            .any(|part| part == boundary.as_bytes())
        {
            return Err("HTTP_MULTIPART_BOUNDARY_CONFLICT".into());
        }
        let mut header = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"",
            quoted(name)
        );
        if let Some(filename) = filename {
            header.push_str(&format!("; filename=\"{}\"", quoted(filename)));
        }
        if let Some(mime) = mime {
            header.push_str(&format!("\r\nContent-Type: {mime}"));
        }
        header.push_str("\r\n\r\n");
        append(&mut body, header.as_bytes())?;
        append(&mut body, &bytes)?;
        append(&mut body, b"\r\n")?;
    }
    append(&mut body, format!("--{boundary}--\r\n").as_bytes())?;
    Ok((
        body,
        Some(format!("multipart/form-data; boundary={boundary}")),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multipart_framing_preserves_binary_and_escapes_disposition_values() {
        let (bytes, mime) = encode(&[
            Part::Text {
                name: "label".into(),
                value: "Acme 東京".into(),
            },
            Part::File {
                name: "label".into(),
                filename: "Acme\"\r\n.bin".into(),
                mime_type: "application/octet-stream".into(),
                data: "AAEC/w==".into(),
            },
        ])
        .unwrap();
        let mime = mime.unwrap();
        let boundary = mime.strip_prefix("multipart/form-data; boundary=").unwrap();
        assert!(bytes.starts_with(format!("--{boundary}\r\n").as_bytes()));
        assert!(bytes.ends_with(format!("--{boundary}--\r\n").as_bytes()));
        assert!(bytes.windows(4).any(|part| part == [0, 1, 2, 255]));
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(text.matches("name=\"label\"").count(), 2);
        assert!(text.contains("filename=\"Acme%22%0D%0A.bin\""));
        assert!(
            encode(&[Part::Text {
                name: "large".into(),
                value: "x".repeat(MAX_BODY)
            }])
            .is_err()
        );
        assert!(
            encode(&[Part::File {
                name: "file".into(),
                filename: "Acme".into(),
                mime_type: "text/plain\r\nInjected: yes".into(),
                data: String::new()
            }])
            .is_err()
        );
    }
}

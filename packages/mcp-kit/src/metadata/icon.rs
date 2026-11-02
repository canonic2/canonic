//! Validated icon hints with no fetching, rendering or image decoding.
use super::MetadataError;
use serde::{Deserialize, Deserializer, Serialize, de};

/// Preferred icon background theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IconTheme {
    /// Light backgrounds.
    Light,
    /// Dark backgrounds.
    Dark,
}
/// Immutable bounded image descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Icon {
    src: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    mime_type: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    sizes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    theme: Option<IconTheme>,
}
impl Icon {
    /// Checks HTTPS/data-image source, image MIME type and positive size hints.
    ///
    /// # Errors
    /// Returns [`MetadataError`] for invalid syntax or exceeded bounds.
    pub fn new(
        src: impl Into<String>,
        mime_type: Option<String>,
        sizes: Vec<String>,
        theme: Option<IconTheme>,
    ) -> Result<Self, MetadataError> {
        let src = src.into();
        let image_mime = |mime: &str| {
            mime.strip_prefix("image/").is_some_and(|s| {
                !s.is_empty()
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
            })
        };
        if src.len() > 65536
            || (!web_url(&src, true)
                && !src
                    .strip_prefix("data:")
                    .and_then(|s| s.split_once(','))
                    .is_some_and(|(header, data)| {
                        header.strip_suffix(";base64").is_some_and(image_mime)
                            && crate::content::decoded_size(data, 49152).is_ok()
                    }))
            || mime_type
                .as_deref()
                .is_some_and(|mime| mime.len() > 256 || !image_mime(mime))
        {
            return Err(MetadataError::Field("icon source or MIME type"));
        }
        if sizes.len() > 16
            || sizes.iter().any(|size| {
                size != "any"
                    && !size.split_once('x').is_some_and(|(w, h)| {
                        [w, h].iter().all(|s| {
                            !s.is_empty()
                                && s.bytes().all(|b| b.is_ascii_digit())
                                && s.parse::<u16>().is_ok_and(|n| n > 0)
                        })
                    })
            })
        {
            return Err(MetadataError::Field("icon sizes"));
        }
        let icon = Self {
            src,
            mime_type,
            sizes,
            theme,
        };
        crate::json::measure(&icon, super::MAX_METADATA_BYTES, &|| Ok(()))
            .map_err(|_| MetadataError::Limit("icon bytes"))?;
        Ok(icon)
    }
    /// Exact source URI; the library never fetches it.
    #[must_use]
    pub fn src(&self) -> &str {
        &self.src
    }
}
impl<'de> Deserialize<'de> for Icon {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            src: String,
            mime_type: Option<String>,
            #[serde(default)]
            sizes: Vec<String>,
            theme: Option<IconTheme>,
        }
        let w = Wire::deserialize(d)?;
        Self::new(w.src, w.mime_type, w.sizes, w.theme).map_err(de::Error::custom)
    }
}
pub(super) fn web_url(url: &str, https_only: bool) -> bool {
    url.len() <= 4096
        && iri_string::types::UriStr::new(url).is_ok_and(|uri| {
            (uri.scheme_str().eq_ignore_ascii_case("https")
                || (!https_only && uri.scheme_str().eq_ignore_ascii_case("http")))
                && uri.authority_components().is_some_and(|authority| {
                    !authority.host().is_empty() && authority.userinfo().is_none()
                })
        })
}

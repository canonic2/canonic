//! Immutable MCP Apps documents, namespace composition and client negotiation; no I/O.
mod metadata;
pub use metadata::{ToolUi, UiCsp, UiMetadata, UiPermission, UiPermissions, UiVisibility};

use crate::{
    BundledRead, Catalog, ReadResourceResult, Resource, ResourceCatalog, ResourceContents,
    rpc::RpcError,
};
use serde_json::Value;
use std::collections::HashMap;

/// Stable MCP Apps extension identifier.
pub const EXTENSION: &str = "io.modelcontextprotocol/ui";
/// HTML media type understood by Apps hosts.
pub const MIME_TYPE: &str = "text/html;profile=mcp-app";
/// Maximum documents, including unlisted documents, per catalog.
pub const MAX_RESOURCES: usize = 64;
/// Maximum aggregate HTML bytes per catalog.
pub const MAX_BYTES: usize = 16 * 1024 * 1024;

/// Invalid Apps declaration. The URI identifies the affected document or tool link.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid MCP Apps declaration {uri:?}: {field}")]
pub struct AppsError {
    /// Document or linked resource URI, empty for a catalog-wide failure.
    pub uri: String,
    /// Invalid field or exceeded bound.
    pub field: &'static str,
}

impl AppsError {
    pub(crate) fn new(uri: &str, field: &'static str) -> Self {
        Self {
            uri: uri.into(),
            field,
        }
    }
}

/// Mutable UI document definition; construction of [`AppsCatalog`] validates it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiResource {
    /// Listing identity and optional presentation metadata; MIME type is fixed.
    pub resource: Resource,
    /// Exact UTF-8 HTML, validated without parsing or executing it.
    pub html: Vec<u8>,
    /// Host-enforced security and presentation requests.
    pub ui: UiMetadata,
    /// Whether this document appears in resource listings. Reads remain available.
    pub listed: bool,
}

impl UiResource {
    /// Defines a listed document; the catalog validates bytes, URI and metadata.
    #[must_use]
    pub fn new(uri: impl Into<String>, name: impl Into<String>, html: impl AsRef<[u8]>) -> Self {
        Self {
            resource: Resource::new(uri, name),
            html: html.as_ref().to_vec(),
            ui: UiMetadata::default(),
            listed: true,
        }
    }
}

/// Validated static documents. Clones share prepared HTML and read results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppsCatalog {
    reads: HashMap<String, BundledRead>,
    listed: Vec<Resource>,
}

impl AppsCatalog {
    /// Validates the entire catalog and prepares its immutable reads once.
    ///
    /// # Errors
    /// Returns [`AppsError`] for invalid or repeated URIs, non-UTF-8/empty HTML,
    /// invalid metadata or exceeded document, catalog and metadata limits.
    pub fn new(resources: Vec<UiResource>) -> Result<Self, AppsError> {
        if resources.len() > MAX_RESOURCES {
            return Err(AppsError::new("", "resource count"));
        }
        let mut catalog = Self {
            reads: HashMap::new(),
            listed: Vec::new(),
        };
        let mut bytes = 0usize;
        for definition in resources {
            let UiResource {
                mut resource,
                html,
                ui,
                listed,
            } = definition;
            let error = |field| AppsError::new(&resource.uri, field);
            validate_uri(&resource.uri)?;
            if catalog.reads.contains_key(&resource.uri) {
                return Err(error("duplicate URI"));
            }
            bytes = bytes.saturating_add(html.len());
            if html.is_empty() || html.len() > crate::resource::MAX_BYTES || bytes > MAX_BYTES {
                return Err(error("HTML bytes"));
            }
            if resource
                .mime_type
                .as_deref()
                .is_some_and(|mime| mime != MIME_TYPE)
            {
                return Err(error("mimeType"));
            }
            ui.validate().map_err(error)?;
            let metadata = metadata::merged(resource.meta.as_ref(), &ui).map_err(error)?;
            let text = String::from_utf8(html).map_err(|_| error("HTML UTF-8"))?;
            resource.mime_type = Some(MIME_TYPE.into());
            resource.size = Some(text.len() as u64);
            resource.meta = Some(metadata.clone());
            crate::ResourceLink::new(resource.clone())
                .map_err(|_| AppsError::new(&resource.uri, "resource metadata"))?;
            let mut contents = ResourceContents::text(&resource.uri, Some(MIME_TYPE.into()), text)
                .map_err(|_| AppsError::new(&resource.uri, "resource contents"))?;
            contents.set_metadata(metadata);
            let read = ReadResourceResult::new(vec![contents])
                .map_err(|_| AppsError::new(&resource.uri, "read result"))?;
            catalog.reads.insert(
                resource.uri.clone(),
                BundledRead::prepared(resource.uri.clone(), read),
            );
            if listed {
                catalog.listed.push(resource);
            }
        }
        Ok(catalog)
    }

    /// Looks up only the selected prepared document; never performs I/O.
    #[must_use]
    pub fn read(&self, uri: &str) -> Option<BundledRead> {
        self.reads.get(uri).cloned()
    }

    pub(crate) fn validate_tools(&self, catalog: &Catalog) -> Result<(), AppsError> {
        for tool in catalog.tools() {
            if let Some(uri) = tool.ui.as_ref().and_then(|ui| ui.resource_uri.as_deref())
                && !self.reads.contains_key(uri)
            {
                return Err(AppsError::new(uri, "tool links to missing UI resource"));
            }
        }
        Ok(())
    }

    pub(crate) fn compose(
        &self,
        ordinary: Option<&ResourceCatalog>,
        skills: Option<&crate::SkillCatalog>,
    ) -> Result<ResourceCatalog, crate::ConfigurationError> {
        let (mut resources, templates) =
            ordinary.map_or_else(|| (vec![], vec![]), ResourceCatalog::definitions);
        if resources.iter().any(|resource| is_ui(&resource.uri))
            || templates
                .iter()
                .any(|template| is_ui(&template.uri_template))
        {
            return Err(crate::ConfigurationError(
                "Apps reserves the ui resource scheme".into(),
            ));
        }
        if let Some(skills) = skills
            && skills.root_uris().iter().any(|uri| is_ui(uri))
        {
            return Err(crate::ConfigurationError(
                "Apps reserves ui skill roots".into(),
            ));
        }
        resources.extend(self.listed.iter().cloned());
        ResourceCatalog::new(resources, templates)
            .map_err(|error| crate::ConfigurationError(error.to_string()))
    }
}

pub(crate) fn is_ui(uri: &str) -> bool {
    uri.split_once(':')
        .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case("ui"))
}

pub(crate) fn validate_uri(uri: &str) -> Result<(), AppsError> {
    crate::resource::validate_uri(uri).map_err(|_| AppsError::new(uri, "URI"))?;
    let valid = uri
        .strip_prefix("ui://")
        .and_then(|rest| rest.split_once('/'))
        .is_some_and(|(authority, path)| !authority.is_empty() && !path.is_empty());
    if !valid || uri.contains(['?', '#']) {
        return Err(AppsError::new(
            uri,
            "UI URI requires authority and path, without query or fragment",
        ));
    }
    Ok(())
}

/// Validates a client declaration and reports support for the fixed HTML MIME type.
/// No declaration means no Apps support; this does not establish tool authority.
///
/// # Errors
/// Returns invalid parameters for malformed or excessive Apps declarations.
pub fn client_support(capabilities: &Value) -> Result<bool, RpcError> {
    let Some(declaration) = capabilities
        .get("extensions")
        .and_then(|extensions| extensions.get(EXTENSION))
    else {
        return Ok(false);
    };
    crate::modern::bounded(declaration, crate::metadata::MAX_METADATA_BYTES, 16)?;
    let types = declaration
        .as_object()
        .and_then(|map| map.get("mimeTypes"))
        .and_then(Value::as_array)
        .filter(|types| types.len() <= 32)
        .ok_or_else(|| crate::modern::invalid("Invalid Apps mimeTypes"))?;
    if types.iter().any(|mime| {
        mime.as_str()
            .is_none_or(|mime| mime.is_empty() || mime.len() > 256 || mime.contains('\0'))
    }) {
        return Err(crate::modern::invalid("Invalid Apps mimeTypes"));
    }
    Ok(types.iter().any(|mime| mime == MIME_TYPE))
}

pub(crate) fn validate_configuration(
    catalog: &Catalog,
    apps: Option<&AppsCatalog>,
) -> Result<(), crate::ConfigurationError> {
    match apps {
        Some(apps) => apps
            .validate_tools(catalog)
            .map_err(|error| crate::ConfigurationError(error.to_string())),
        None if catalog.tools().iter().any(|tool| tool.ui.is_some()) => Err(
            crate::ConfigurationError("tool UI metadata requires an Apps catalog".into()),
        ),
        None => Ok(()),
    }
}

pub(crate) fn configure(
    options: &mut crate::SessionOptions,
) -> Result<(), crate::ConfigurationError> {
    if let Some(apps) = &options.apps {
        if options
            .subscriptions
            .as_ref()
            .is_some_and(|policy| policy.resource_uris.iter().any(|uri| is_ui(uri)))
        {
            return Err(crate::ConfigurationError(
                "Apps UI resources cannot be subscribed to".into(),
            ));
        }
        options.resources = Some(std::sync::Arc::new(
            apps.compose(options.resources.as_deref(), options.skills.as_deref())?,
        ));
    }
    Ok(())
}

pub(crate) use metadata::{WithUi, tool_metadata};

#[cfg(test)]
mod tests;

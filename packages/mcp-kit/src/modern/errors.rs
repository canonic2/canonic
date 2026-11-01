//! Revision-specific error allocation, payload validation and legacy resource mapping.

use crate::rpc::{self, RpcError};
use std::borrow::Cow;

pub(super) fn normalize<'a>(method: &str, error: &'a RpcError) -> Cow<'a, RpcError> {
    if method == "resources/read"
        && error.code == rpc::RESOURCE_NOT_FOUND
        && error
            .data
            .as_ref()
            .and_then(|data| data["uri"].as_str())
            .is_some_and(|uri| {
                crate::resource::validate_uri(uri).is_ok()
                    && *error == crate::resource::not_found(uri)
            })
    {
        let mut mapped = error.clone();
        mapped.code = rpc::INVALID_PARAMS;
        return Cow::Owned(mapped);
    }
    let valid = match error.code {
        rpc::PARSE_ERROR
        | rpc::INVALID_REQUEST
        | rpc::METHOD_NOT_FOUND
        | rpc::INVALID_PARAMS
        | rpc::INTERNAL_ERROR => true,
        // Retain the library's existing server/deadline codes and grandfathered
        // consumer allocations. The retired resource code is never forwarded.
        -32019..=-32000 => error.code != rpc::RESOURCE_NOT_FOUND,
        -32021 => error.data.as_ref().is_some_and(|data| {
            super::capabilities(&data["requiredCapabilities"]).is_ok_and(|map| !map.is_empty())
        }),
        -32022 => error.data.as_ref().is_some_and(|data| {
            data["requested"].as_str().is_some_and(|version| {
                !version.is_empty() && version.len() <= crate::session::MAX_VERSION_BYTES
            }) && data["supported"].as_array().is_some_and(|versions| {
                !versions.is_empty()
                    && versions.iter().all(|version| {
                        version.as_str().is_some_and(|version| {
                            !version.is_empty()
                                && version.len() <= crate::session::MAX_VERSION_BYTES
                        })
                    })
            }) && super::bounded(data, 65536, 32).is_ok()
        }),
        // HeaderMismatch is HTTP-only; all other MCP-reserved values, retired
        // codes and unallocated JSON-RPC codes are invalid on this stdio profile.
        -32768..=-32000 => false,
        _ => true,
    };
    if valid {
        Cow::Borrowed(error)
    } else {
        Cow::Owned(RpcError::new(
            rpc::INTERNAL_ERROR,
            "Invalid protocol error for modern request",
        ))
    }
}

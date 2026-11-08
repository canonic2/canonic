//! Exact compatibility with read grants captured before normalized resources.
//! Their original request/grant hashes stay unchanged; the derived file input
//! is used only to evaluate the same read under the active engine.
use super::{Error, Path, Request, json, policy};
use crate::review::runtime::consumer::ResourceRequest;

pub(super) fn resource(request: &Request) -> Result<ResourceRequest, Error> {
    if !crate::enforcement::read_text::valid_request(request) {
        return Err(Error::Invalid("invalid legacy read request"));
    }
    let path = Path::new(&request.arguments[0]);
    let root = Path::new(&request.project);
    if !path.starts_with(root)
        || path.starts_with(root.join(".shield"))
        || path.extension().is_none_or(|extension| extension != "txt")
        || [path, Path::new(&request.cwd)].into_iter().any(|path| {
            path.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        })
    {
        return Err(Error::Invalid("legacy read must remain inside its project"));
    }
    let mut operation = json!({"kind":"read-text","path":request.arguments[0],
        "offset":null,"limit":null});
    for pair in request.arguments[1..].chunks_exact(2) {
        let name = pair[0]
            .strip_prefix("--")
            .ok_or(Error::Invalid("invalid legacy read window"))?;
        let value = pair[1]
            .parse::<u64>()
            .map_err(|_| Error::Invalid("invalid legacy read window"))?;
        operation[name] = json!(value);
    }
    Ok(ResourceRequest {
        mcp: None,
        input: policy::normalized::Input::File {
            action: "read".into(),
            sources: request.arguments[..1].to_vec(),
            destination: None,
            recursive: false,
            original_input: operation,
        },
        context: policy::normalized::Context {
            cwd: request.cwd.clone(),
            environment: None,
            working_files: None,
        },
    })
}

pub(super) fn matches(request: &Request, incoming: &ResourceRequest) -> bool {
    let Ok(expected) = resource(request) else {
        return false;
    };
    let policy::normalized::Input::File {
        original_input: expected_input,
        ..
    } = &expected.input
    else {
        return false;
    };
    let mut sdk_input = json!({"file_path":request.arguments[0]});
    for name in ["offset", "limit"] {
        if !expected_input[name].is_null() {
            sdk_input[name] = expected_input[name].clone();
        }
    }
    let policy::normalized::Input::File {
        action,
        sources,
        destination: None,
        recursive: false,
        original_input,
    } = &incoming.input
    else {
        return false;
    };
    action == "read"
        && sources.as_slice() == &request.arguments[..1]
        && incoming.context.cwd == request.cwd
        && incoming.context.environment.is_none()
        && incoming.context.working_files.is_none()
        && incoming.mcp.is_none()
        && (original_input == expected_input || original_input == &sdk_input)
}

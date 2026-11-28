//! Pinned `addScriptTag`/`addStyleTag` become one handle-returning evaluation.
//! Like the SDK client, a caller `path` is read here and sent as content.
use super::*;

/// Inline content and file bytes (before the `sourceURL` comment) share one
/// UTF-8 byte bound.
const MAX_CONTENT_BYTES: usize = 512 * 1024;
/// The encoded evaluation must fit the 1 MiB manager frame and driver
/// argument limits with room for their envelopes; escaping can grow content.
const MAX_REQUEST_BYTES: usize = 960 * 1024;
const CONTENT_BOUNDS: &str = "Tag content exceeds 512 KiB or its encoded request exceeds 960 KiB";
/// The pinned SDK rejects when a Content Security Policy console error races
/// the tag. Pages report the same block as an enforced
/// `securitypolicyviolation`, which is queued as a task, so the guard waits one
/// more task after the tag settles. A block also ends an inline style's wait
/// for `load`, which would otherwise last until the deadline.
const GUARD: &str = "const guard=async run=>{let fail;const failed=new Promise((_,reject)=>fail=reject);failed.catch(()=>{});const listen=event=>{if(event.disposition==='enforce')fail(new Error('Content-Security-Policy blocked the tag: '+event.violatedDirective));};document.addEventListener('securitypolicyviolation',listen,true);try{const element=await Promise.race([run(),failed]);await Promise.race([new Promise(resolve=>setTimeout(resolve,0)),failed]);return element;}finally{document.removeEventListener('securitypolicyviolation',listen,true);}};";
const SCRIPT: &str = "async ({url,content,type})=>{GUARD return guard(async()=>{const script=document.createElement('script');if(type)script.type=type;if(url!==null){script.src=url;const loaded=new Promise((resolve,reject)=>{script.onload=resolve;script.onerror=()=>reject(new Error('Failed to load script at '+url));});document.head.appendChild(script);await loaded;return script;}script.text=content;let error=null;script.onerror=event=>error=event;document.head.appendChild(script);if(error)throw error;return script;});}";
const STYLE: &str = "async ({url,content})=>{GUARD return guard(async()=>{if(url!==null){const link=document.createElement('link');link.rel='stylesheet';link.href=url;const loaded=new Promise((resolve,reject)=>{link.onload=resolve;link.onerror=()=>reject(new Error('Failed to load stylesheet at '+url));});document.head.appendChild(link);await loaded;return link;}const style=document.createElement('style');style.type='text/css';style.appendChild(document.createTextNode(content));const loaded=new Promise((resolve,reject)=>{style.onload=resolve;style.onerror=reject;});document.head.appendChild(style);await loaded;return style;});}";

fn refusal(code: &'static str, message: &str) -> Error {
    Error::new(code, message).details(json!({"admitted":false,"dispatched":false}))
}

/// Rewrites a `page_add_script_tag`/`page_add_style_tag` request in place.
pub(super) fn rewrite(
    request: &mut Value,
    base: &mut Option<std::path::PathBuf>,
    cwd: &mut impl FnMut() -> std::io::Result<std::path::PathBuf>,
) -> Result<()> {
    let script = request["operation"] == "page_add_script_tag";
    let url = request
        .get("url")
        .and_then(Value::as_str)
        .map(str::to_owned);
    // As in the pinned client, a `path` replaces inline `content`.
    let content = match request.get("path").and_then(Value::as_str) {
        Some(path) => Some(read(path, script, base, cwd)?),
        None => request
            .get("content")
            .and_then(Value::as_str)
            .map(str::to_owned),
    };
    if url.is_none() && content.is_none() {
        return Err(refusal(
            "INVALID_ARGUMENT",
            "Provide url, path or content for the tag",
        ));
    }
    if request.get("path").is_none()
        && content
            .as_ref()
            .is_some_and(|content| content.len() > MAX_CONTENT_BYTES)
    {
        return Err(refusal("TAG_CONTENT_BOUNDS", CONTENT_BOUNDS));
    }
    let mut argument = json!({"url":url,"content":content});
    if script {
        argument["type"] = request.get("type").cloned().unwrap_or(json!(""));
    }
    let expression = if script { SCRIPT } else { STYLE }.replacen("GUARD", GUARD, 1);
    let rewritten = json!({"operation":"evaluate","scope":request["scope"].clone(),"expression":expression,"isFunction":true,"argument":{"kind":"json","value":argument},"returnHandle":true});
    if serde_json::to_vec(&rewritten).map_or(true, |encoded| encoded.len() > MAX_REQUEST_BYTES) {
        return Err(refusal("TAG_CONTENT_BOUNDS", CONTENT_BOUNDS));
    }
    *request = rewritten;
    Ok(())
}

/// Pinned client: the file is read relative to the caller and labeled with
/// a `sourceURL` comment so its code keeps a readable origin.
fn read(
    path: &str,
    script: bool,
    base: &mut Option<std::path::PathBuf>,
    cwd: &mut impl FnMut() -> std::io::Result<std::path::PathBuf>,
) -> Result<String> {
    if path.is_empty() || path.len() > 4096 || path.contains('\0') {
        return Err(refusal(
            "TAG_PATH_INVALID",
            "Tag path requires a bounded host file path",
        ));
    }
    let selected = if std::path::Path::new(path).is_absolute() {
        std::path::PathBuf::from(path)
    } else {
        let directory = match base {
            Some(directory) => directory,
            None => base.insert(
                cwd().map_err(|_| refusal("HOST_IO", "Cannot bind the relative tag path"))?,
            ),
        };
        directory.join(path)
    };
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Special files refuse below without blocking on a missing writer.
        options.custom_flags(libc::O_NONBLOCK);
    }
    let io = |_| refusal("TAG_FILE_IO", "Cannot read the selected tag file");
    let file = options.open(&selected).map_err(io)?;
    if !file.metadata().map_err(io)?.is_file() {
        return Err(refusal(
            "TAG_FILE_NOT_REGULAR",
            "The tag path is not a regular file",
        ));
    }
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut std::io::Read::take(file, MAX_CONTENT_BYTES as u64 + 1),
        &mut bytes,
    )
    .map_err(io)?;
    if bytes.len() > MAX_CONTENT_BYTES {
        return Err(refusal("TAG_CONTENT_BOUNDS", CONTENT_BOUNDS));
    }
    let mut content = String::from_utf8(bytes)
        .map_err(|_| refusal("TAG_FILE_ENCODING", "Tag files must be UTF-8 text"))?;
    let label = path.replace('\n', "");
    if script {
        content.push_str(&format!("\n//# sourceURL={label}"));
    } else {
        content.push_str(&format!("\n/*# sourceURL={label}*/"));
    }
    Ok(content)
}

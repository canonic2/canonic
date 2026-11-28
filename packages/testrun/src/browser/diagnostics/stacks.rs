//! Private, independently bounded engine stack summaries without source retrieval.
use super::*;

pub(crate) fn engine(value: &str) -> Value {
    let (mut text, mut truncated) = super::text(value);
    while serde_json::to_vec(&text).expect("Stack text").len() > 1900 {
        text.pop();
        truncated = true;
    }
    json!({"format":"engine-error-stack","text":text,"truncated":truncated})
}

pub(crate) fn cdp(trace: &Value) -> Value {
    let mut frames = Vec::new();
    let mut cursor = trace;
    let mut truncated = false;
    let mut async_depth = 0;
    loop {
        for frame in cursor["callFrames"].as_array().into_iter().flatten() {
            let name = frame["functionName"].as_str().unwrap_or("");
            let source = url(frame["url"].as_str().unwrap_or(""));
            let short = |value: &str, maximum: usize| {
                let mut end = value.len().min(maximum);
                while !value.is_char_boundary(end) {
                    end -= 1;
                }
                value[..end].to_owned()
            };
            let row = json!({"functionName":short(name,128),"functionNameTruncated":name.len()>128,"url":short(&source,256),"urlTruncated":source.len()>256,"line":frame["lineNumber"],"column":frame["columnNumber"],"asyncDepth":async_depth});
            frames.push(row);
            if frames.len() > 16 || serde_json::to_vec(&frames).expect("Stack frames").len() > 1900
            {
                frames.pop();
                truncated = true;
                break;
            }
        }
        if truncated {
            break;
        }
        if cursor.get("parentId").is_some() {
            truncated = true;
        }
        let Some(parent) = cursor.get("parent") else {
            break;
        };
        async_depth += 1;
        if async_depth > 4 {
            truncated = true;
            break;
        }
        cursor = parent;
    }
    json!({"format":"cdp-call-frames","frames":frames,"truncated":truncated})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stacks_bound_escaped_text_and_async_frames_without_leaking_url_credentials() {
        let raw = engine(&"\0é".repeat(3000));
        assert!(raw["truncated"].as_bool().unwrap());
        assert!(serde_json::to_vec(&raw).unwrap().len() < 2048);
        let frame = json!({"functionName":"Acme diagnostic","url":"https://user:Acme-secret@example.com/app.js?secret=Acme-secret#private","lineNumber":12,"columnNumber":4});
        let trace = cdp(&json!({"callFrames":vec![frame;100],"parentId":{"id":"unavailable"}}));
        assert_eq!(trace["truncated"], true);
        assert!(!trace.to_string().contains("Acme-secret"));
        assert_eq!(trace["frames"][0]["line"], 12);
        assert!(serde_json::to_vec(&trace).unwrap().len() < 2048);
    }
}

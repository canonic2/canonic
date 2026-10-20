//! Storybook implementations: reading a Storybook's story index, the stories
//! under a title, finding one the user runs, and running one the server owns
//! on a local socket. Storybook is not an adapter; its pages are framed
//! through an implementation proxy.

mod fetch;
mod index;
mod owned;

pub(crate) use index::{Story, Storybook, detect, index, stories_titled};
pub(crate) use owned::{Options, OwnedStorybook};

use crate::refused::refused;
use anyhow::Result;
use serde_json::Value;

/// The stories titled `title` in the Storybook implementation `key` of the
/// resolved config's `implementations`, read from its index each time:
/// stories are added while the workbench is open.
///
/// # Errors
/// 400 for an implementation that isn't an available Storybook, or no
/// title; otherwise as [`index()`] and [`stories_titled`].
pub(crate) async fn stories(implementations: &Value, key: &str, title: &str) -> Result<Vec<Story>> {
    let storybook = implementations
        .get(key)
        .filter(|implementation| {
            implementation.get("kind").and_then(Value::as_str) == Some("storybook")
        })
        .and_then(Storybook::of)
        .filter(|storybook| !storybook.url.is_empty() && storybook.url != "auto")
        .ok_or_else(|| {
            refused(
                400,
                format!("there is no available Storybook implementation called “{key}”"),
            )
        })?;
    if title.is_empty() {
        return Err(refused(400, "which title?"));
    }
    let index = index(storybook).await?;
    stories_titled(&index, title, storybook)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::refused::status_of;
    use serde_json::json;

    #[tokio::test]
    async fn only_an_available_storybook_implementation_lists_stories() {
        let implementations = json!({
            "dev": { "kind": "url", "base": "http://127.0.0.1:5173" },
            "auto": { "kind": "storybook", "url": "auto", "auto": true },
            "storybook": { "kind": "storybook", "url": "http://127.0.0.1:6006" },
        });
        for key in ["dev", "auto", "missing"] {
            let error = stories(&implementations, key, "Button")
                .await
                .expect_err("not available");
            assert_eq!(status_of(&error, 500), 400);
            assert_eq!(
                error.to_string(),
                format!("there is no available Storybook implementation called “{key}”")
            );
        }
        let error = stories(&implementations, "storybook", "")
            .await
            .expect_err("no title");
        assert_eq!(
            (status_of(&error, 500), error.to_string().as_str()),
            (400, "which title?")
        );
    }
}

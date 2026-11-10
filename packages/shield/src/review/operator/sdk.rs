//! Explicit terminal consent for the fixed general-runtime read-only consumer.
use super::terminal::Terminal;
use crate::review::{Context, Error, sdk_read};
use serde_json::Value;

pub(super) fn setup(context: &Context, adapter: &str, json_mode: bool) -> Result<Value, Error> {
    let mut terminal = Terminal::required(json_mode)?;
    let prepared = sdk_read::Prepared::new(context, adapter)?;
    terminal.confirm(&prepared.review())?;
    prepared.commit(context, &|| Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sdk_registration_refuses_json_before_preparing_or_writing() {
        let root = assert_fs::TempDir::new().unwrap();
        let context = Context::new("a".repeat(64), root.path().to_string_lossy().into(), "cli");
        assert!(matches!(
            setup(&context, "claude-read-only", true),
            Err(Error::Invalid(
                "JSON is read-only; run this command interactively"
            ))
        ));
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }
}

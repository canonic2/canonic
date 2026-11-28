//! One-shot Android inspection and guarded replacement with bounded owned cleanup.
use super::{Android, adb::validate_serial};
use crate::{
    error::{Error, Result},
    process,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::time::Duration;

const DEX: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/focus.dex"));

impl Android {
    pub(crate) async fn focused_editable(&self, serial: &str) -> Result<bool> {
        parse(
            &self
                .inspection(serial, "focus", &[], 4096, Duration::from_secs(12))
                .await?,
        )
    }

    pub async fn snapshot(&self, serial: &str) -> Result<serde_json::Value> {
        let bytes = self
            .inspection(
                serial,
                "snapshot",
                &[],
                2 * 1024 * 1024,
                Duration::from_secs(12),
            )
            .await?;
        if bytes.first() == Some(&b'{') {
            parse(&bytes)?;
            return Err(Error::new(
                "PLATFORM_PROTOCOL",
                "Expected an Android tree observation",
            ));
        }
        let mut tree = super::hierarchy(&bytes)?;
        tree["nativeDigest"] = serde_json::json!(native_digest(&bytes)?);
        Ok(tree)
    }

    pub(crate) async fn replace_text(
        &self,
        serial: &str,
        proof: &serde_json::Value,
        text: &str,
        timeout: Duration,
    ) -> Result<serde_json::Value> {
        let input = serde_json::to_vec(
            &serde_json::json!({"digest":proof["digest"],"index":proof["index"],"text":text}),
        )
        .map_err(|_| Error::invalid("Invalid replacement request"))?;
        let bytes = self
            .inspection(
                serial,
                "replace",
                &input,
                4096,
                timeout.min(Duration::from_secs(12)),
            )
            .await
            .map_err(|error| {
                Error::new(
                    "OUTCOME_UNKNOWN",
                    "Replacement reply was lost; inspect without replaying text",
                )
                .details(serde_json::json!({"cause":error.code}))
            })?;
        replacement_reply(&bytes)
    }

    async fn inspection(
        &self,
        serial: &str,
        mode: &str,
        request: &[u8],
        max_bytes: usize,
        timeout: Duration,
    ) -> Result<Vec<u8>> {
        validate_serial(serial)?;
        let name = format!("testrun-focus-{}", uuid::Uuid::new_v4().simple());
        let script = command(
            &name,
            DEX.len(),
            &format!("{:x}", Sha256::digest(DEX)),
            mode,
        );
        let mut input = DEX.to_vec();
        input.extend_from_slice(request);
        let output = process::run_input(
            &self.adb,
            &["-s", serial, "shell", "-T", &script],
            &input,
            timeout,
            max_bytes,
        )
        .await?;
        Ok(output.bytes)
    }
}

/// Query proofs cover exposed node content; window identity remains in reference
/// revalidation, but some targets change its number between helper connections.
fn native_digest(bytes: &[u8]) -> Result<String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid native tree encoding"))?;
    let source = text.find("<hierarchy").map(|start| &text[start..]);
    let (header, remainder) = source
        .and_then(|source| source.split_once('>'))
        .ok_or_else(|| Error::new("PLATFORM_PROTOCOL", "Native query proof is unavailable"))?;
    let body = if header.trim_end().ends_with('/') && remainder.trim().is_empty() {
        ""
    } else {
        remainder
            .trim_end()
            .strip_suffix("</hierarchy>")
            .ok_or_else(|| Error::new("PLATFORM_PROTOCOL", "Native query proof is unavailable"))?
    };
    Ok(format!("{:x}", Sha256::digest(body.as_bytes())))
}

/// Generated nonce, byte count and digest only: no caller text enters shell code.
fn command(name: &str, bytes: usize, digest: &str, mode: &str) -> String {
    format!(
        r#"umask 077
dir=/data/local/tmp/{name}
mkdir "$dir" || exit 1
child=
cleanup() {{
  if [ -n "$child" ] && [ -d /proc/$child ]; then
    owner=$(tr '\000' '\n' < /proc/$child/cmdline | head -n 1)
    [ "$owner" = '{name}' ] || return 1
    kill -KILL "$child" || return 1
    wait "$child" 2>/dev/null || :
  fi
  rm -f "$dir/focus.dex" && rmdir "$dir"
}}
trap 'cleanup || exit 125' EXIT
trap 'exit 143' HUP INT TERM
dd bs=1 count={bytes} of="$dir/focus.dex" 2>/dev/null || exit 1
[ "$(sha256sum "$dir/focus.dex" | cut -d ' ' -f 1)" = '{digest}' ] || exit 1
chmod 400 "$dir/focus.dex" || exit 1
export CLASSPATH="$dir/focus.dex"
exec 3<&0
app_process / --nice-name={name} io.canonic.testrun.focus.Main {mode} <&3 &
exec 3<&-
child=$!
wait "$child"
status=$?
child=
exit "$status"
"#
    )
}

fn replacement_reply(bytes: &[u8]) -> Result<serde_json::Value> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Replacement {
        version: u32,
        completed: Option<bool>,
        code: Option<String>,
    }
    let reply: Replacement = serde_json::from_slice(bytes).map_err(|_| {
        Error::new(
            "OUTCOME_UNKNOWN",
            "Invalid replacement reply; inspect without replaying text",
        )
    })?;
    if reply.version == 1 && reply.completed == Some(true) && reply.code.is_none() {
        return Ok(
            serde_json::json!({"completed":true,"observed":false,"backend":"accessibility_set_text","focusVerified":true}),
        );
    }
    let code = reply
        .code
        .as_deref()
        .filter(|code| {
            reply.version == 1
                && reply.completed.is_none()
                && matches!(
                    *code,
                    "STALE_ELEMENT"
                        | "FOCUS_NOT_PROVEN"
                        | "CAPABILITY_UNAVAILABLE"
                        | "ACTION_REFUSED"
                        | "AUTOMATION_UNAVAILABLE"
                        | "ROOT_UNAVAILABLE"
                        | "CLEANUP_FAILED"
                )
        })
        .unwrap_or("OUTCOME_UNKNOWN");
    Err(Error::new(
        code,
        "Android replacement did not complete; inspect the current field before another action",
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reply {
    version: u32,
    focused_editable: Option<bool>,
    code: Option<String>,
}
fn parse(bytes: &[u8]) -> Result<bool> {
    let reply: Reply = serde_json::from_slice(bytes)
        .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid Android focus probe reply"))?;
    if reply.version != 1 || reply.code.is_some() == reply.focused_editable.is_some() {
        return Err(Error::new(
            "PLATFORM_PROTOCOL",
            "Invalid Android focus probe result",
        ));
    }
    if reply.code.as_deref().is_some_and(|code| {
        !matches!(
            code,
            "AUTOMATION_UNAVAILABLE" | "ROOT_UNAVAILABLE" | "FOCUS_STALE" | "CLEANUP_FAILED"
        )
    }) {
        return Err(Error::new(
            "PLATFORM_PROTOCOL",
            "Unknown Android focus probe error",
        ));
    }
    reply.focused_editable.ok_or_else(|| {
        Error::new(
            if reply.code.as_deref() == Some("CLEANUP_FAILED") {
                "CLEANUP_FAILED"
            } else {
                "UI_UNAVAILABLE"
            },
            "Android focus probe could not complete inspection and cleanup",
        )
        .details(serde_json::json!({"probeCode":reply.code}))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn query_proofs_ignore_connection_window_numbers_but_not_node_changes() {
        let original =
            b"<hierarchy window-id=\"1\"><node package=\"com.example.acme\"/></hierarchy>";
        let reconnected =
            b"<hierarchy window-id=\"2\"><node package=\"com.example.acme\"/></hierarchy>\n";
        let changed =
            b"<hierarchy window-id=\"2\"><node package=\"com.example.other\"/></hierarchy>";
        assert_eq!(
            native_digest(original).unwrap(),
            native_digest(reconnected).unwrap()
        );
        assert_ne!(
            native_digest(original).unwrap(),
            native_digest(changed).unwrap()
        );
    }
    #[tokio::test]
    #[cfg(unix)]
    async fn private_request_survives_dex_framing_and_background_process_stdin() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let dex = b"owned fixture dex";
        let request = br#"{"text":"Acme private request","index":1}"#;
        let digest = format!("{:x}", Sha256::digest(dex));
        for (name, source) in [
            ("sha256sum", format!("#!/bin/sh\nprintf '%s  ignored\\n' '{digest}'\n")),
            ("app_process", "#!/bin/sh\ncat > \"$(dirname \"$0\")/received\"\nprintf '{\"version\":1,\"completed\":true}'\n".into()),
        ] {
            let path = directory.path().join(name);
            std::fs::write(&path, source).unwrap();
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let script = command("owned", dex.len(), &digest, "replace")
            .replace("/data/local/tmp", directory.path().to_str().unwrap());
        let script = format!(
            "PATH='{}':/usr/bin:/bin\n{script}",
            directory.path().display()
        );
        let mut input = dex.to_vec();
        input.extend_from_slice(request);
        let result = process::run_input(
            "/bin/sh",
            &["-c", &script],
            &input,
            Duration::from_secs(3),
            4096,
        )
        .await
        .unwrap();
        assert_eq!(
            std::fs::read(directory.path().join("received")).unwrap(),
            request
        );
        assert!(replacement_reply(&result.bytes).is_ok());
        assert!(!directory.path().join("owned").exists());
    }
    #[test]
    fn focus_requires_an_explicit_versioned_boolean_without_content() {
        assert!(parse(br#"{"version":1,"focusedEditable":true}"#).unwrap());
        assert!(!parse(br#"{"version":1,"focusedEditable":false}"#).unwrap());
        for reply in [
            br#"{"version":1}"#.as_slice(),
            br#"{"version":2,"focusedEditable":true}"#,
            br#"{"version":1,"focusedEditable":true,"text":"private"}"#,
            br#"{"version":1,"focusedEditable":true,"code":"failed"}"#,
        ] {
            assert_eq!(parse(reply).unwrap_err().code, "PLATFORM_PROTOCOL");
        }
        assert_eq!(
            parse(br#"{"version":1,"code":"CLEANUP_FAILED"}"#)
                .unwrap_err()
                .code,
            "CLEANUP_FAILED"
        );
    }
}

//! Exact public schemas for managed playback operations.
use serde_json::{Value, json};
fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}
fn tool(name: &str, description: &str, input: Value, read: bool) -> Value {
    json!({"name":name,"description":description,"inputSchema":input,"annotations":{"readOnlyHint":read,"destructiveHint":!read,"idempotentHint":read,"openWorldHint":true}})
}
/// Where a media or volume command goes: an Android media session from
/// `media_status`, Android's music stream, or the player iOS chooses.
fn destination(kinds: &[&str]) -> Value {
    let mut value = object(
        json!({"kind":{"enum":kinds},"id":{"type":"string","minLength":1,"maxLength":128}}),
        &["kind"],
    );
    value["description"] =
        json!("media_session needs the id of a session from media_status; other kinds take no id");
    value
}
pub(crate) fn tools() -> Vec<Value> {
    let window = json!({"type":"integer","minimum":0,"maximum":2000,"default":0,"description":"How long to watch for the effect; Android only"});
    let volume = destination(&["music_stream", "media_session"]);
    vec![
        tool(
            "media_status",
            "Return the Android device's active media sessions, metadata and playback positions",
            object(json!({}), &[]),
            true,
        ),
        tool(
            "media_send",
            "Send one play, pause, skip, stop or volume button command to a media player",
            object(
                json!({"destination":destination(&["media_session","system_player"]),"command":{"enum":["play","pause","play_pause","next","previous","stop","volume_up","volume_down"]},"observeForMs":window}),
                &["destination", "command"],
            ),
            false,
        ),
        tool(
            "volume_get",
            "Return the Android music-stream or media-session volume",
            object(json!({"destination":volume}), &["destination"]),
            true,
        ),
        tool(
            "volume_set",
            "Set the Android music-stream or media-session volume, or step it up or down",
            object(
                json!({"destination":volume,"value":{"type":"integer","minimum":0,"maximum":2147483647},"adjust":{"enum":["up","down"]},"observeForMs":window}),
                &["destination"],
            ),
            false,
        ),
        tool(
            "audio_probe",
            "Measure the device's audio output for a few seconds; on Android this reroutes output and needs acceptRouteChange",
            object(
                json!({"durationMs":{"type":"integer","minimum":100,"maximum":10000,"default":2000},"retainClip":{"type":"boolean","default":false},"acceptRouteChange":{"const":true}}),
                &[],
            ),
            false,
        ),
    ]
}

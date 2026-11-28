//! Mobile playback capability: validation, per-controller native owners and bounded results.
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

mod android;
/// The shell helper, which also answers the one-time Android package read.
pub(crate) use android::DEX as ANDROID_HELPER;
#[cfg(target_os = "macos")]
mod ios;
pub(crate) mod probe;
pub(crate) mod schema;
#[cfg(test)]
mod tests;

pub(crate) fn operation(name: &str) -> bool {
    matches!(
        name,
        "media_status" | "media_send" | "volume_get" | "volume_set" | "audio_probe"
    )
}

/// What the selected target's platform supports. Android capture reroutes
/// output, so it needs the caller's explicit consent.
pub(crate) fn validate_platform(name: &str, args: &Value, android: bool) -> Result<()> {
    let unavailable = |message: &str| Err(Error::new("CAPABILITY_UNAVAILABLE", message));
    match name {
        "media_status" | "volume_get" | "volume_set" if !android => {
            unavailable("Media state and volume are available on Android only")
        }
        "media_send" if android && args["destination"]["kind"] == "system_player" => {
            unavailable("Android media commands need a media_session destination from media_status")
        }
        "media_send" if !android && args["destination"]["kind"] != "system_player" => {
            unavailable("iOS media commands go to the system_player destination")
        }
        "media_send" if !android && args.get("observeForMs").is_some() => {
            unavailable("iOS media commands cannot observe their effect")
        }
        "media_send"
            if android
                && args["command"]
                    .as_str()
                    .is_some_and(|c| c.starts_with("volume_")) =>
        {
            unavailable("Android volume buttons are sent with volume_set adjust")
        }
        "audio_probe" if android && args["acceptRouteChange"] != true => Err(Error::invalid(
            "Android audio capture changes output routing; supply acceptRouteChange: true",
        )),
        _ => Ok(()),
    }
}
pub(crate) fn mutation(name: &str) -> bool {
    matches!(name, "media_send" | "volume_set" | "audio_probe")
}
pub(crate) fn validate(name: &str, args: &Value) -> Result<()> {
    let definition = schema::tools()
        .into_iter()
        .find(|v| v["name"] == name)
        .ok_or_else(|| Error::invalid("Media operation is not registered"))?;
    crate::schema::validate(&definition["inputSchema"], args)?;
    let destination = &args["destination"];
    if destination.is_object()
        && (destination["kind"] == "media_session") != destination["id"].is_string()
    {
        return Err(Error::invalid(
            "destination needs an id for media_session and no id otherwise",
        ));
    }
    if name == "volume_set" && args.get("value").is_some() == args.get("adjust").is_some() {
        return Err(Error::invalid("Supply exactly one of value or adjust"));
    }
    Ok(())
}

#[derive(Default)]
pub(crate) struct Owners {
    probes: HashMap<String, probe::SharedJob>,
    #[cfg(target_os = "macos")]
    audio: HashMap<String, Arc<crate::device_media::audio::Owner>>,
    android: HashMap<String, Arc<android::Helper>>,
    #[cfg(target_os = "macos")]
    ios: HashMap<String, Arc<ios::Buttons>>,
}
#[derive(Clone)]
pub(crate) struct ReadHandle(std::sync::Weak<android::Helper>);
impl ReadHandle {
    pub(crate) async fn call(&self, name: &str, args: &Value) -> Result<Value> {
        validate(name, args)?;
        if mutation(name) {
            return Err(Error::invalid("Read handle cannot mutate playback"));
        }
        self.0
            .upgrade()
            .ok_or_else(|| Error::new("SESSION_CLOSING", "Media helper is no longer owned"))?
            .call(name, args, false)
            .await
    }
}
impl Owners {
    pub(crate) fn read_handle(&self, session: &str) -> Option<ReadHandle> {
        self.android
            .get(session)
            .map(|helper| ReadHandle(Arc::downgrade(helper)))
    }
    pub(crate) async fn prepare(
        &mut self,
        session: &str,
        target: &crate::targets::Target,
        name: &str,
        args: &Value,
    ) -> Result<()> {
        if target.kind == "android" {
            let helper = self
                .android
                .entry(session.into())
                .or_insert_with(|| android::Helper::start(&target.platform_id))
                .clone();
            helper.ready().await?;
            if mutation(name) && !name.ends_with("audio_probe") {
                helper.call(name, args, true).await?;
            }
            return Ok(());
        }
        Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "This target has no media adapter",
        ))
    }
    #[cfg(target_os = "macos")]
    pub(crate) fn prepare_audio(
        &mut self,
        session: &str,
        serial: &str,
        source: Option<Arc<crate::device_media::Source>>,
    ) -> Result<()> {
        let owner = self
            .audio
            .entry(session.into())
            .or_insert_with(|| crate::device_media::audio::Owner::new(serial, None));
        owner.refresh_source(source)
    }
    #[cfg(target_os = "macos")]
    pub(crate) async fn prepare_ios(
        &mut self,
        session: &str,
        serial: &str,
        source: Option<Arc<crate::device_media::Source>>,
    ) -> Result<()> {
        let buttons = self
            .ios
            .entry(session.into())
            .or_insert_with(|| ios::Buttons::new(serial, source))
            .clone();
        buttons.ready().await
    }
    pub(crate) async fn call(&self, session: &str, name: &str, args: &Value) -> Result<Value> {
        if let Some(helper) = self.android.get(session) {
            let mut result = helper.call(name, args, false).await?;
            if name == "volume_set" {
                result["dispatch"]["command"] = json!("volume_set");
                for field in ["value", "adjust"] {
                    if let Some(value) = args.get(field) {
                        result["dispatch"][field] = value.clone();
                    }
                }
            }
            return Ok(result);
        }
        #[cfg(target_os = "macos")]
        if let Some(buttons) = self.ios.get(session) {
            return buttons.press(args["command"].as_str().unwrap()).await;
        }
        Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Media buttons are unavailable",
        ))
    }
    pub(crate) async fn close(&mut self, session: &str) -> Result<()> {
        if let Some(job) = self.probes.get(session) {
            let _ = job.close().await;
        }
        #[cfg(target_os = "macos")]
        if let Some(owner) = self.audio.get(session) {
            owner.close().await?;
        }
        self.probes.remove(session);
        #[cfg(target_os = "macos")]
        self.audio.remove(session);
        if let Some(helper) = self.android.get(session) {
            helper.close().await?;
        }
        self.android.remove(session);
        #[cfg(target_os = "macos")]
        {
            if let Some(buttons) = self.ios.get(session) {
                buttons.close().await?;
            }
            self.ios.remove(session);
        }
        Ok(())
    }
    pub(crate) async fn probe(
        &mut self,
        session: &str,
        args: &Value,
        context: probe::Context,
    ) -> Result<Value> {
        let job = self.probes.entry(session.into()).or_default().clone();
        if context.target.kind == "android" {
            let helper = self
                .android
                .get(session)
                .ok_or_else(|| probe::failed("owner_missing"))?
                .clone();
            let args = args.clone();
            job.run(async move {
                if context.deadline.saturating_duration_since(tokio::time::Instant::now())<std::time::Duration::from_millis(args["durationMs"].as_u64().unwrap_or(2000)+13000) {
                    return Err(probe::failed("insufficient_capture_budget"));
                }
                let observed_start = probe::utc();
                let capture=helper.probe(&args,context.deadline-std::time::Duration::from_secs(10)).await;
                let mut result=match capture {
                    Err(error) if error.code=="MEDIA_HELPER_FAILED"=>{
                        let cleanup=helper.close().await;
                        return Err(probe::failed("helper_capture_or_cleanup").details(json!({"cause":error,"cleanup":cleanup.as_ref().err(),"cleanupVerified":cleanup.is_ok()})));
                    },
                    result=>result?,
                };
                // Audio bytes must never enter errors, journals or automatic tool attachments.
                let encoded = result.as_object_mut().and_then(|v| v.remove("pcmBase64"));
                stamp(&mut result, &context.target);
                result["actionId"] = json!(context.command);
                result["observedStart"] = json!(observed_start);
                result["observedEnd"] = json!(probe::utc());
                probe::limitations(&mut result, true);
                let frames = args["durationMs"].as_u64().unwrap_or(2000) * 48;
                if result["sampleRate"] != 48000
                    || result["channels"] != 2
                    || result["sampleCount"] != frames * 2
                    || result["frameCount"] != frames
                    || result["cleanupVerified"] != true
                {
                    return Err(probe::failed("invalid_measurement_response").details(json!({"phase":"invalid_measurement_response","cleanupVerified":result["cleanupVerified"]==true,"partial":probe::diagnostics(&result)})));
                }
                if (args["retainClip"] == true) != encoded.is_some() {
                    return Err(probe::failed("clip_consent_or_missing_samples"));
                }
                if let Some(encoded) = encoded {
                    use base64::Engine;
                    let pcm = base64::engine::general_purpose::STANDARD
                        .decode(encoded.as_str().unwrap_or(""))
                        .map_err(|_| probe::failed("clip_protocol"))?;
                    if pcm.len() as u64 != frames * 4 {
                        return Err(probe::failed("clip_sample_count"));
                    }
                    probe::clip(&mut result, &pcm)?;
                }
                Ok(result)
            })
            .await
        } else {
            #[cfg(target_os = "macos")]
            {
                let owner = self
                    .audio
                    .get(session)
                    .ok_or_else(|| probe::failed("owner_missing"))?
                    .clone();
                let args = args.clone();
                job.run(async move { owner.probe(args, context).await })
                    .await
            }
            #[cfg(not(target_os = "macos"))]
            {
                Err(Error::new("UNSUPPORTED_HOST", "iOS audio requires macOS"))
            }
        }
    }
}

pub(crate) fn stamp(value: &mut Value, target: &crate::targets::Target) {
    value["targetId"] = json!(target.identity.target_id);
    value["generation"] = json!(target.identity.generation);
    value["observedAt"] = json!(
        time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default()
    );
    value["audibility"] = json!("unverified");
}

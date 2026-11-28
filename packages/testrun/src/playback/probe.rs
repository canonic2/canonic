//! Bounded PCM measurements, private opt-in clips and cancellation-independent probe jobs.
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::{Mutex, watch};

pub(crate) const CLIP_LIMIT: usize = 16 * 1024 * 1024;
pub(crate) struct Context {
    pub(crate) target: crate::targets::Target,
    pub(crate) command: String,
    pub(crate) deadline: tokio::time::Instant,
}
pub(crate) fn utc() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}
#[derive(Default)]
#[cfg(any(test, target_os = "macos"))]
pub(crate) struct Samples {
    count: u64,
    zero: u64,
    squares: f64,
    peak: f64,
    pcm: Option<Vec<u8>>,
}
#[cfg(any(test, target_os = "macos"))]
impl Samples {
    pub(crate) fn new(retain: bool) -> Self {
        Self {
            pcm: retain.then(Vec::new),
            ..Self::default()
        }
    }
    pub(crate) fn push(&mut self, samples: &[i16]) -> Result<()> {
        if !samples.len().is_multiple_of(2) {
            return Err(failed("incomplete_stereo_frame"));
        }
        if let Some(pcm) = &mut self.pcm {
            if pcm.len() + samples.len() * 2 > CLIP_LIMIT {
                return Err(failed("clip_limit"));
            }
            pcm.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
        }
        for &sample in samples {
            let value = f64::from(sample) / 32768.;
            self.count += 1;
            self.zero += u64::from(sample == 0);
            self.squares += value * value;
            self.peak = self.peak.max(value.abs());
        }
        Ok(())
    }
    pub(crate) fn frames(&self) -> u64 {
        self.count / 2
    }
    pub(crate) fn result(&self) -> Value {
        json!({"sampleRate":48000,"channels":2,"sampleCount":self.count,"frameCount":self.frames(),
            "decodedDurationMs":self.frames() as f64 / 48.,"digitalZeroSampleCount":self.zero,
            "digitalZero":self.count>0 && self.count==self.zero,
            "rmsDbfs":if self.squares>0. {Some(10.*(self.squares/self.count as f64).log10())}else{None},
            "peakDbfs":if self.peak>0. {Some(20.*self.peak.log10())}else{None}})
    }
    pub(crate) fn pcm(&self) -> Option<&[u8]> {
        self.pcm.as_deref()
    }
}
pub(crate) fn failed(phase: &str) -> Error {
    Error::new("AUDIO_PROBE_FAILED", "Audio probe did not complete").details(json!({"phase":phase}))
}
/// Only documented scalar metrics enter partial diagnostics; native payloads and
/// arbitrary helper fields are excluded even when the response is malformed.
pub(crate) fn diagnostics(value: &Value) -> Value {
    let mut result = scalar_fields(
        value,
        &[
            "backend",
            "sourceEpoch",
            "helperEpoch",
            "targetId",
            "generation",
            "observedAt",
            "observedStart",
            "observedEnd",
            "sampleRate",
            "channels",
            "sampleCount",
            "frameCount",
            "decodedDurationMs",
            "digitalZeroSampleCount",
            "digitalZero",
            "rmsDbfs",
            "peakDbfs",
            "wallDurationMs",
            "firstPacketAgeMs",
            "lastPacketAgeMs",
            "cleanupVerified",
            "observed",
            "experimental",
            "attribution",
            "volumeStage",
            "audibility",
            "integrity",
            "limitations",
        ],
    );
    if value["health"].is_object() {
        result["health"] = scalar_fields(
            &value["health"],
            &[
                "packets",
                "reads",
                "packetLoss",
                "decodeErrors",
                "captureHealthy",
                "packetLossUnavailableReason",
                "firstPacketAgeMs",
                "lastPacketAgeMs",
            ],
        );
    }
    if value["routes"].is_object() {
        let routes = &value["routes"];
        let mut summary = scalar_fields(routes, &["restoration", "unavailableReason"]);
        for phase in ["before", "during", "after"] {
            if routes[phase].is_null() {
                summary[phase] = Value::Null;
                continue;
            }
            let route = &routes[phase];
            let mut info = scalar_fields(route, &["source", "unavailableReason"]);
            info["devices"] = route["devices"].as_array().map_or(Value::Null, |devices| {
                json!(
                    devices
                        .iter()
                        .take(16)
                        .map(|device| scalar_fields(device, &["type", "role"]))
                        .collect::<Vec<_>>()
                )
            });
            summary[phase] = info;
        }
        result["routes"] = summary;
    }
    result
}
pub(crate) fn failure_diagnostics(value: &Value) -> Value {
    let mut result = scalar_fields(
        value,
        &["phase", "cause", "cleanupCause", "cleanupVerified"],
    );
    if value["partial"].is_object() {
        result["partial"] = diagnostics(&value["partial"]);
    }
    result
}
fn scalar_fields(value: &Value, fields: &[&str]) -> Value {
    let mut result = json!({});
    for &field in fields {
        if let Some(value) = value.get(field) {
            match value {
                Value::String(text) => {
                    result[field] = json!(text.chars().take(256).collect::<String>())
                }
                Value::Null | Value::Bool(_) | Value::Number(_) => result[field] = value.clone(),
                _ => {}
            }
        }
    }
    result
}
pub(crate) fn limitations(result: &mut Value, android: bool) {
    result["attribution"] = json!("system_mix");
    result["volumeStage"] = json!("pre_output_volume");
    result["audibility"] = json!("unverified");
    result["integrity"] = json!("not_fully_verifiable");
    result["experimental"] = json!(android);
    result["limitations"] = json!(if android {
        "Remote submix changes output routing and captured policy-restricted fixture audio on API 31; policy behavior varies by OS. This does not prove app identity or physical audibility."
    } else {
        "System mix before output volume; AirPlay is not captured on the lab-tested OS. Zero samples cannot identify the route. Successful AAC decoding cannot detect every corruption."
    });
}
pub(crate) fn clip(result: &mut Value, pcm: &[u8]) -> Result<()> {
    if pcm.len() > CLIP_LIMIT || pcm.is_empty() || !pcm.len().is_multiple_of(4) {
        return Err(failed("clip_format_or_limit"));
    }
    let directory = std::env::temp_dir().join(format!("testrun-audio-{}", uuid::Uuid::new_v4()));
    crate::artifacts::directory(&directory)?;
    let path = directory.join("audio.s16le");
    let manifest = json!({"format":"pcm_s16le","sampleRate":48000,"channels":2,"complete":true,
        "evidence":result,"bytes":pcm.len(),"privacy":"private"});
    let manifest_path = directory.join("manifest.json");
    let publication = crate::artifacts::write(&path, pcm).and_then(|()| {
        crate::artifacts::write(
            &manifest_path,
            &serde_json::to_vec_pretty(&manifest).unwrap(),
        )
    });
    if let Err(error) = publication {
        let cleanup = std::fs::remove_dir_all(&directory);
        return Err(failed("clip_publication")
            .details(json!({"cause":error,"artifactCleanupVerified":cleanup.is_ok()})));
    }
    result["clip"] = json!({"path":path,"manifest":manifest_path,"mimeType":"application/octet-stream","bytes":pcm.len(),"attached":false});
    Ok(())
}

type Completion = Option<Result<Value>>;
#[derive(Default)]
pub(super) struct Job {
    state: Mutex<Option<watch::Receiver<Completion>>>,
}
impl Job {
    pub(super) async fn run<F>(&self, work: F) -> Result<Value>
    where
        F: Future<Output = Result<Value>> + Send + 'static,
    {
        let mut state = self.state.lock().await;
        if let Some(previous) = state.as_ref() {
            let result = previous.borrow().clone();
            match result {
                None => {
                    return Err(Error::new(
                        "CAPTURE_CONFLICT",
                        "An audio probe is already running",
                    ));
                }
                Some(Err(ref e))
                    if e.details
                        .as_ref()
                        .is_some_and(|v| v["cleanupVerified"] == false) =>
                {
                    return Err(e.clone());
                }
                _ => {}
            }
        }
        let (sender, receiver) = watch::channel(None);
        *state = Some(receiver.clone());
        tokio::spawn(async move {
            sender.send_replace(Some(work.await));
        });
        drop(state);
        wait(receiver).await
    }
    pub(super) async fn close(&self) -> Result<()> {
        let state = self.state.lock().await.clone();
        if let Some(receiver) = state {
            match wait(receiver).await {
                Err(error)
                    if error
                        .details
                        .as_ref()
                        .is_some_and(|v| v["cleanupVerified"] == false) =>
                {
                    Err(error)
                }
                _ => Ok(()),
            }
        } else {
            Ok(())
        }
    }
}
async fn wait(mut receiver: watch::Receiver<Completion>) -> Result<Value> {
    loop {
        if let Some(value) = receiver.borrow().clone() {
            return value;
        }
        receiver
            .changed()
            .await
            .map_err(|_| failed("owner_ended"))?;
    }
}
pub(super) type SharedJob = Arc<Job>;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_diagnostics_keep_bounded_metrics_and_exclude_audio_at_every_level() {
        let data = json!({"sampleCount":0,"backend":"a".repeat(10000),"pcmBase64":"private-audio".repeat(10000),"extra":"private-audio","health":{"reads":0,"payload":"private-audio"},"routes":{"before":{"source":"fixture","devices":[{"type":2,"role":2,"pcmBase64":"private-audio"}]},"during":null,"after":null}});
        let result = failure_diagnostics(
            &json!({"phase":"read","cleanupVerified":true,"pcmBase64":"private-audio","partial":data}),
        );
        let encoded = result.to_string();
        assert!(!encoded.contains("private-audio"));
        assert!(!encoded.contains("pcmBase64"));
        assert!(encoded.len() < 4096);
        assert_eq!(result["partial"]["sampleCount"], 0);
        assert_eq!(result["partial"]["health"]["reads"], 0);
        assert_eq!(
            result["partial"]["routes"]["before"]["devices"][0]["type"],
            2
        );
    }
    #[test]
    fn zeros_are_finite_json_and_channels_do_not_cancel() {
        let mut samples = Samples::new(false);
        samples.push(&[0; 960]).unwrap();
        assert_eq!(samples.result()["digitalZero"], true);
        assert!(samples.result()["rmsDbfs"].is_null());
        assert!(samples.pcm().is_none());
        samples.push(&[16384, -16384]).unwrap();
        assert_eq!(samples.result()["digitalZero"], false);
        assert!((samples.result()["peakDbfs"].as_f64().unwrap() + 6.0206).abs() < 0.001);
    }
    #[test]
    fn known_signal_rms_peak_and_sample_bounds() {
        let mut samples = Samples::new(true);
        let wave = [0, 0, 16384, 16384, 0, 0, -16384, -16384];
        samples.push(&wave.repeat(12000)).unwrap();
        let result = samples.result();
        assert_eq!(result["decodedDurationMs"], 1000.);
        assert_eq!(result["sampleCount"], 96000);
        assert!((result["rmsDbfs"].as_f64().unwrap() + 9.0309).abs() < 0.001);
        assert_eq!(samples.pcm().unwrap().len(), 192000);
        assert!(samples.push(&[1]).is_err());
        assert!(
            Samples::new(true)
                .push(&vec![0; CLIP_LIMIT / 2 + 2])
                .is_err()
        );
    }
    #[tokio::test]
    async fn cancelled_caller_keeps_cleanup_owned_and_rejects_overlap() {
        let job = Arc::new(Job::default());
        let (finish, done) = tokio::sync::oneshot::channel();
        let task = {
            let job = job.clone();
            tokio::spawn(async move {
                job.run(async {
                    done.await.unwrap();
                    Ok(json!({"cleaned":true}))
                })
                .await
            })
        };
        tokio::task::yield_now().await;
        task.abort();
        assert_eq!(
            job.run(async { Ok(json!({})) }).await.unwrap_err().code,
            "CAPTURE_CONFLICT"
        );
        finish.send(()).unwrap();
        job.close().await.unwrap();
    }
}

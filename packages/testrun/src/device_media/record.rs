//! HEVC evidence recording from the shared source, with independently finalized segments.
use super::{Source, VideoFrame, apple::Writer};
use crate::{
    artifacts,
    error::{Error, Result},
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{broadcast, oneshot};

async fn finalize(writer: Writer, path: PathBuf, final_path: PathBuf) -> Result<Value> {
    tokio::task::spawn_blocking(move || writer.finish())
        .await
        .map_err(|_| Error::new("RECORDING_FAILED", "HEVC writer task failed"))??;
    let verify_path = path.clone();
    let verification = tokio::task::spawn_blocking(move || super::apple::verify(&verify_path))
        .await
        .map_err(|_| Error::new("RECORDING_FAILED", "HEVC verification task failed"))??;
    let mut artifact = artifacts::register(&path, &final_path, "video/mp4")?;
    artifact["verification"] = verification;
    Ok(artifact)
}
#[expect(
    clippy::too_many_lines,
    reason = "Recording segmentation, finalization and publication share one evidence transaction"
)]
pub async fn run(
    source: Arc<Source>,
    evidence: &Path,
    id: &str,
    duration: u64,
    mut stop: oneshot::Receiver<()>,
) -> Result<Value> {
    let mut input = source.subscribe_frames();
    source.keyframe();
    let deadline = tokio::time::sleep(Duration::from_millis(duration));
    tokio::pin!(deadline);
    let mut writer = None;
    let mut config = 0;
    let mut segment_start = 0;
    let mut frame_count = 0u64;
    let mut last_pts = 0;
    let mut dropped = 0u64;
    let mut startup_skipped = 0u64;
    let mut waiting = true;
    let mut segments = Vec::new();
    let mut temporary = evidence.join(format!(".{id}.partial.mp4"));
    let mut final_path = evidence.join(format!("{id}.mp4"));
    let mut total_bytes = 0u64;
    let mut reason = None;
    let mut geometry = None;
    loop {
        let frame = tokio::select! {
            _=&mut stop=>break,
            _=&mut deadline=>break,
            value=input.recv()=>match value{
                Ok(frame)=>frame,
                Err(broadcast::error::RecvError::Lagged(count))=>{dropped+=count;waiting=true;source.keyframe();continue;},
                Err(_)=>{reason=Some("SOURCE_LOST");break;},
            },
            _=tokio::time::sleep(Duration::from_secs(10))=>{reason=Some("SOURCE_STALLED");break;},
        };
        let VideoFrame {
            key,
            pts,
            configuration_sequence,
            format,
            payload,
            ..
        } = frame.as_ref();
        // Keep full-track verification within the stop deadline. Each segment
        // starts on a keyframe; request one before reaching the duration bound.
        let elapsed = pts.saturating_sub(segment_start);
        if writer.is_some() && elapsed >= 1_500_000_000 {
            source.keyframe();
        }
        if writer.is_some() && elapsed >= 5_000_000_000 {
            reason = Some("KEYFRAME_TIMEOUT");
            break;
        }
        if waiting && !*key {
            if writer.is_none() {
                startup_skipped += 1;
            } else {
                dropped += 1;
            }
            continue;
        }
        if config != *configuration_sequence || (*key && elapsed >= 2_000_000_000) {
            if !*key {
                dropped += 1;
                waiting = true;
                source.keyframe();
                continue;
            }
            if let Some(previous) = writer.take() {
                let artifact = finalize(previous, temporary.clone(), final_path.clone()).await?;
                total_bytes += artifact["bytes"].as_u64().unwrap_or(0);
                segments.push(json!({"artifact":artifact,"startPtsNs":segment_start.to_string(),"endPtsNs":last_pts.to_string(),"geometry":geometry}));
                let index = segments.len();
                temporary = evidence.join(format!(".{id}-{index}.partial.mp4"));
                final_path = evidence.join(format!("{id}-{index}.mp4"));
            }
            writer = Some(Writer::open(&temporary, format)?);
            segment_start = *pts;
            config = *configuration_sequence;
            geometry = Some(json!({"width":format.width,"height":format.height}));
        }
        let output = writer
            .as_mut()
            .ok_or_else(|| Error::new("RECORDING_FAILED", "No recording writer"))?;
        if output.append(format, payload, pts - segment_start, *key)? {
            frame_count += 1;
            last_pts = *pts;
            waiting = false;
        } else {
            dropped += 1;
            waiting = true;
            source.keyframe();
        }
        if total_bytes + temporary.metadata().map_or(0, |m| m.len()) >= 512 * 1024 * 1024 {
            reason = Some("SIZE_LIMIT");
            break;
        }
    }
    if let Some(writer) = writer.take() {
        let artifact = finalize(writer, temporary, final_path).await?;
        segments.push(json!({"artifact":artifact,"startPtsNs":segment_start.to_string(),"endPtsNs":last_pts.to_string(),"geometry":geometry}));
    }
    if segments.is_empty() {
        return Err(Error::new(
            "FIRST_FRAME_TIMEOUT",
            "Recording ended without a decodable device keyframe",
        ));
    }
    let status = if reason.is_some() || dropped > 0 {
        "partial"
    } else {
        "complete"
    };
    let decoded = segments
        .iter()
        .map(|s| {
            s["artifact"]["verification"]["decodedFrames"]
                .as_u64()
                .unwrap_or(0)
        })
        .sum::<u64>();
    if decoded != frame_count {
        return Err(Error::new(
            "RECORDING_FAILED",
            "Decoded HEVC frame count does not match acknowledged recording frames",
        ));
    }
    let result = json!({"recordingId":id,"status":status,"codec":"hevc","container":"mp4","segments":segments,"artifact":segments[0]["artifact"],"statistics":{"frames":frame_count,"decodedFrames":decoded,"startupSkippedFrames":startup_skipped,"droppedFrames":dropped,"reason":reason,"audio":false}});
    artifacts::write(
        &evidence.join(format!("{id}.json")),
        &serde_json::to_vec_pretty(&result)
            .map_err(|_| Error::new("INTERNAL", "Cannot serialize recording manifest"))?,
    )?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_frame(pts: u64, key: bool) -> Arc<VideoFrame> {
        let nals: Vec<Vec<u8>> = include_str!("../../tests/fixtures/ios/hevc-keyframe.hex")
            .lines()
            .map(|line| {
                line.as_bytes()
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                    .collect()
            })
            .collect();
        let format = Arc::new(
            super::super::apple::Format::new(&[nals[0].clone(), nals[1].clone(), nals[2].clone()])
                .unwrap(),
        );
        let mut payload = (nals[3].len() as u32).to_be_bytes().to_vec();
        payload.extend_from_slice(&nals[3]);
        Arc::new(VideoFrame {
            media: crate::media::Frame {
                packet: Vec::new(),
                metadata: json!({}),
                configuration: None,
                received: std::time::Instant::now(),
            },
            payload,
            key,
            pts,
            format,
            configuration_sequence: 1,
        })
    }

    #[tokio::test]
    async fn recording_segments_preserve_every_acknowledged_frame() {
        let source = super::super::source::recording_fixture();
        let directory = tempfile::tempdir().unwrap();
        let (stop, receiver) = oneshot::channel();
        let job = tokio::spawn({
            let source = source.clone();
            let path = directory.path().to_owned();
            async move { run(source, &path, "fixture", 10000, receiver).await }
        });
        while source.frames.receiver_count() == 0 {
            tokio::task::yield_now().await;
        }
        for seq in 0..70 {
            assert!(
                source
                    .frames
                    .send(fixture_frame(seq * 100_000_000, true))
                    .is_ok()
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // Force an observable segment boundary before stopping. Stop intentionally
        // does not promise to drain source frames that have not been admitted yet.
        let mut boundary = fixture_frame(7_000_000_000, true);
        Arc::get_mut(&mut boundary).unwrap().configuration_sequence = 2;
        assert!(source.frames.send(boundary).is_ok());
        tokio::time::timeout(Duration::from_secs(10), async {
            while !directory.path().join("fixture-3.mp4").is_file() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        drop(source);
        let _ = stop.send(());
        let result = job.await.unwrap().unwrap();
        assert_eq!(result["status"], "complete");
        assert_eq!(result["statistics"]["frames"], 71);
        assert_eq!(result["statistics"]["decodedFrames"], 71);
        assert_eq!(result["segments"].as_array().unwrap().len(), 5);
        assert!(directory.path().join("fixture.json").is_file());
    }

    #[tokio::test]
    async fn missing_segment_keyframe_preserves_verified_partial_evidence() {
        let source = super::super::source::recording_fixture();
        let directory = tempfile::tempdir().unwrap();
        let (_stop, receiver) = oneshot::channel();
        let job = tokio::spawn({
            let source = source.clone();
            let path = directory.path().to_owned();
            async move { run(source, &path, "fixture", 10000, receiver).await }
        });
        while source.frames.receiver_count() == 0 {
            tokio::task::yield_now().await;
        }
        assert!(source.frames.send(fixture_frame(0, true)).is_ok());
        assert!(
            source
                .frames
                .send(fixture_frame(5_100_000_000, false))
                .is_ok()
        );
        let result = job.await.unwrap().unwrap();
        assert_eq!(result["status"], "partial");
        assert_eq!(result["statistics"]["reason"], "KEYFRAME_TIMEOUT");
        assert_eq!(result["statistics"]["decodedFrames"], 1);
        assert!(directory.path().join("fixture.mp4").is_file());
        assert!(directory.path().join("fixture.json").is_file());
    }
}

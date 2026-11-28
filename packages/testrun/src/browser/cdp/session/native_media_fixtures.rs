//! Live Chrome capture using generated audio and camera files on a loopback page.
use super::*;
use crate::browser::{
    contract::MediaFixtures,
    evaluation::EvaluationValue,
    page::{Load, Navigation},
    settings::{ContextSetting, Permission},
};

const CAPTURE: &str = r#"async () => {
    const audio = new AudioContext();
    let stream;
    const video = document.createElement('video');
    try {
        await audio.resume();
        stream = await navigator.mediaDevices.getUserMedia({
            audio: {echoCancellation:false, noiseSuppression:false, autoGainControl:false},
            video: true
        });
        const analyser = audio.createAnalyser();
        analyser.fftSize = 2048;
        const muted = audio.createGain();
        muted.gain.value = 0;
        audio.createMediaStreamSource(stream).connect(analyser).connect(muted).connect(audio.destination);

        video.muted = true;
        video.playsInline = true;
        video.srcObject = stream;
        document.body.append(video);
        const firstFrame = new Promise(resolve => video.requestVideoFrameCallback(resolve));
        await video.play();
        await firstFrame;
        const canvas = document.createElement('canvas');
        canvas.width = canvas.height = 1;
        const pixels = canvas.getContext('2d');
        pixels.drawImage(video, 0, 0, 1, 1);
        const pixel = Array.from(pixels.getImageData(0, 0, 1, 1).data);

        const bins = new Float32Array(analyser.frequencyBinCount);
        const end = performance.now() + 2500;
        let peakHz = 0, peakDb = -Infinity;
        do {
            await new Promise(resolve => setTimeout(resolve, 40));
            analyser.getFloatFrequencyData(bins);
            const peak = bins.reduce((best, db, index) => db > bins[best] ? index : best, 0);
            peakHz = peak * audio.sampleRate / analyser.fftSize;
            peakDb = bins[peak];
        } while ((Math.abs(peakHz - 1000) > 80 || peakDb < -55) && performance.now() < end);
        return JSON.stringify({peakHz, peakDb, pixel});
    } finally {
        stream?.getTracks().forEach(track => track.stop());
        video.remove();
        await audio.close();
    }
}"#;

fn tone() -> Vec<u8> {
    const RATE: u32 = 48_000;
    const SAMPLES: u32 = RATE * 2;
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(36 + 2 * SAMPLES).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
    bytes.extend_from_slice(&RATE.to_le_bytes());
    bytes.extend_from_slice(&(2 * RATE).to_le_bytes());
    bytes.extend_from_slice(b"\x02\0\x10\0data");
    bytes.extend_from_slice(&(2 * SAMPLES).to_le_bytes());
    for index in 0..SAMPLES {
        let sample: i16 = if index % 48 < 24 { 16000 } else { -16000 };
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

fn red_video() -> Vec<u8> {
    let mut bytes = b"YUV4MPEG2 W16 H16 F15:1 Ip A1:1 C420\n".to_vec();
    for _ in 0..4 {
        bytes.extend_from_slice(b"FRAME\n");
        bytes.extend_from_slice(&[81; 256]);
        bytes.extend_from_slice(&[90; 64]);
        bytes.extend_from_slice(&[240; 64]);
    }
    bytes
}

async fn send(session: &Session, request: Request) -> Value {
    successful(
        session,
        session.submit(Uuid::new_v4(), request, 15000).unwrap(),
    )
    .await
}

#[tokio::test]
async fn native_media_fixtures_reach_get_user_media_in_both_modes() {
    assert!(crate::browser::processes::initial_host());
    let cache = std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    assert!(Path::new(&cache).is_absolute());
    let bundle = crate::browser::preparation::prepare_chrome(|_| {})
        .await
        .unwrap();
    let site = targets::Site::start().await;
    let origin = format!("http://127.0.0.1:{}", site.port);
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("private");
    crate::artifacts::directory(&directory).unwrap();
    let wav = root.path().join("tone.wav");
    let camera = root.path().join("red.Y4M");
    std::fs::write(&wav, tone()).unwrap();
    std::fs::write(&camera, red_video()).unwrap();
    for mode in [Mode::Headless, Mode::Headed] {
        let session = Session::launch_connection(
            bundle.clone(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            LaunchOptions {
                media_fixtures: MediaFixtures {
                    microphone_path: Some(wav.to_str().unwrap().to_owned()),
                    camera_path: Some(camera.to_str().unwrap().to_owned()),
                },
                proxy: None,
                engine: Engine::Chrome,
                mode,
                context: Default::default(),
            },
            &directory,
        )
        .unwrap();
        let result = std::panic::AssertUnwindSafe(async {
            let ready = successful(&session, session.startup()).await;
            let context = serde_json::from_value(ready["context"].clone()).unwrap();
            let page = serde_json::from_value(ready["page"].clone()).unwrap();
            send(
                &session,
                Request::ContextConfigure {
                    context,
                    setting: ContextSetting::PermissionsGrant {
                        origin: origin.clone(),
                        permissions: vec![Permission::Microphone, Permission::Camera],
                    },
                },
            )
            .await;
            send(
                &session,
                Request::PageNavigate {
                    page,
                    navigation: Navigation::Goto {
                        url: format!("{origin}/frame"),
                        wait_until: Load::Load,
                    },
                },
            )
            .await;
            let result = send(
                &session,
                Request::Evaluate {
                    scope: page,
                    expression: CAPTURE.into(),
                    is_function: Some(true),
                    argument: EvaluationValue::default(),
                    return_handle: false,
                },
            )
            .await;
            let capture: Value =
                serde_json::from_str(result["value"]["value"].as_str().unwrap()).unwrap();
            let peak_hz = capture["peakHz"].as_f64().unwrap();
            let peak_db = capture["peakDb"].as_f64().unwrap_or(f64::NEG_INFINITY);
            assert!(
                (peak_hz - 1000.0).abs() <= 80.0 && peak_db >= -55.0,
                "{mode:?}: {capture}"
            );
            let pixel: [u8; 4] = serde_json::from_value(capture["pixel"].clone()).unwrap();
            assert!(
                pixel[0] > 180 && pixel[1] < 80 && pixel[2] < 80 && pixel[3] == 255,
                "{mode:?}: {capture}"
            );
        })
        .catch_unwind()
        .await;
        successful(&session, session.disconnect(30000).unwrap()).await;
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }
}

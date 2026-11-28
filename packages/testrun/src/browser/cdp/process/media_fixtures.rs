//! Copy explicitly selected media into the owned Chrome profile before spawn.
use crate::browser::contract::{MAX_MEDIA_FIXTURE_BYTES, MediaFixtures};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::{process::Command, task::JoinHandle};

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Default)]
pub(in crate::browser::cdp) struct Staged {
    microphone: Option<PathBuf>,
    camera: Option<PathBuf>,
}

pub(in crate::browser::cdp) struct Pending {
    pub(in crate::browser::cdp) task: JoinHandle<Result<Staged>>,
    stopped: Arc<AtomicBool>,
}

impl Pending {
    pub(in crate::browser::cdp) fn start(profile: &Path, fixtures: MediaFixtures) -> Option<Self> {
        if fixtures.is_empty() {
            return None;
        }
        let profile = profile.to_owned();
        Some(Self::run(move |stopped| {
            Staged::prepare(&profile, &fixtures, stopped)
        }))
    }

    pub(in crate::browser::cdp) fn run(
        stage: impl FnOnce(&AtomicBool) -> Result<Staged> + Send + 'static,
    ) -> Self {
        let stopped = Arc::new(AtomicBool::new(false));
        let worker = stopped.clone();
        let task = tokio::task::spawn_blocking(move || stage(&worker));
        Self { task, stopped }
    }

    pub(in crate::browser::cdp) fn cancel(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        self.task.abort(); // Only stops work that has yet to enter the blocking pool.
    }
}

impl Staged {
    fn prepare(profile: &Path, fixtures: &MediaFixtures, stopped: &AtomicBool) -> Result<Self> {
        let microphone = fixtures
            .microphone_path
            .as_deref()
            .map(|path| stage(Path::new(path), profile, Input::Microphone, stopped))
            .transpose()?;
        let camera = fixtures
            .camera_path
            .as_deref()
            .map(|path| stage(Path::new(path), profile, Input::Camera, stopped))
            .transpose()?;
        Ok(Self { microphone, camera })
    }

    pub(super) fn configure(&self, command: &mut Command) -> Result<()> {
        if self.microphone.is_some() || self.camera.is_some() {
            command.arg("--use-fake-device-for-media-stream");
        }
        if let Some(path) = &self.microphone {
            // Chrome treats % as a delimiter in its WAV switch value. The
            // sandboxed macOS audio utility cannot open private profile files.
            let path = path
                .to_str()
                .filter(|path| !path.contains('%'))
                .ok_or("MEDIA_FIXTURE_PROFILE_PATH_INVALID")?;
            command
                .arg("--disable-features=AudioServiceOutOfProcess")
                .arg(format!("--use-file-for-fake-audio-capture={path}"));
        }
        if let Some(path) = &self.camera {
            let mut arg = std::ffi::OsString::from("--use-file-for-fake-video-capture=");
            arg.push(path);
            command.arg(arg);
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Input {
    Microphone,
    Camera,
}

impl Input {
    fn filename(self, header: &[u8; 12]) -> Option<&'static str> {
        match self {
            Self::Microphone if header.starts_with(b"RIFF") && &header[8..] == b"WAVE" => {
                Some("microphone.wav")
            }
            Self::Camera if header.starts_with(b"YUV4MPEG2 ") => Some("camera.y4m"),
            Self::Camera if header.starts_with(&[0xff, 0xd8, 0xff]) => Some("camera.mjpeg"),
            _ => None,
        }
    }
}

struct StoppableRead<'a, R> {
    input: R,
    stopped: &'a AtomicBool,
}

impl<R: Read> Read for StoppableRead<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.stopped.load(Ordering::Relaxed) {
            return Err(io::Error::other("Media fixture staging cancelled"));
        }
        self.input.read(buffer)
    }
}

fn stage(
    source: &Path,
    profile: &Path,
    input_kind: Input,
    stopped: &AtomicBool,
) -> Result<PathBuf> {
    if stopped.load(Ordering::Relaxed) {
        return Err("CANCELLED");
    }
    #[cfg(windows)]
    let local_source = {
        if source.components().any(|part| {
            matches!(part, std::path::Component::Normal(name) if name.as_encoded_bytes().contains(&b':'))
        }) {
            return Err("MEDIA_FIXTURE_NOT_REGULAR");
        }
        canonic_process_kit::endpoint::local_path(source).map_err(|_| "MEDIA_FIXTURE_IO")?
    };
    #[cfg(windows)]
    let source = local_source.as_path();
    if !fs::symlink_metadata(source)
        .map_err(|_| "MEDIA_FIXTURE_IO")?
        .is_file()
    {
        return Err("MEDIA_FIXTURE_NOT_REGULAR");
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    crate::artifacts::no_follow(&mut options);
    let mut input = options.open(source).map_err(|_| "MEDIA_FIXTURE_IO")?;
    let metadata = input.metadata().map_err(|_| "MEDIA_FIXTURE_IO")?;
    if !metadata.is_file() {
        return Err("MEDIA_FIXTURE_NOT_REGULAR");
    }
    #[cfg(windows)]
    if crate::artifacts::is_reparse_point(&metadata) {
        return Err("MEDIA_FIXTURE_NOT_REGULAR");
    }
    if metadata.len() > MAX_MEDIA_FIXTURE_BYTES {
        return Err("MEDIA_FIXTURE_SIZE");
    }
    let mut header = [0; 12];
    input.read_exact(&mut header).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            "MEDIA_FIXTURE_FORMAT"
        } else {
            "MEDIA_FIXTURE_IO"
        }
    })?;
    let filename = input_kind.filename(&header).ok_or("MEDIA_FIXTURE_FORMAT")?;
    let destination = profile.join(filename);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut output = options.open(&destination).map_err(|_| "MEDIA_FIXTURE_IO")?;
    output.write_all(&header).map_err(|_| "MEDIA_FIXTURE_IO")?;
    let header_bytes = header.len() as u64;
    let reader = StoppableRead {
        input: &mut input,
        stopped,
    };
    let copied = io::copy(
        &mut reader.take(MAX_MEDIA_FIXTURE_BYTES + 1 - header_bytes),
        &mut output,
    )
    .map_err(|_| "MEDIA_FIXTURE_IO")?
        + header_bytes;
    if copied > MAX_MEDIA_FIXTURE_BYTES {
        return Err("MEDIA_FIXTURE_SIZE");
    }
    let after = input.metadata().map_err(|_| "MEDIA_FIXTURE_IO")?;
    if copied != metadata.len()
        || after.len() != copied
        || after.modified().ok() != metadata.modified().ok()
    {
        return Err("MEDIA_FIXTURE_CHANGED");
    }
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn media_fixture_stages_large_wav_and_camera_as_private_launch_inputs() {
        let files = tempfile::tempdir().unwrap();
        let profile = tempfile::tempdir().unwrap();
        let microphone = files.path().join("dictation % sample.wav");
        let camera = files.path().join("sample video.Y4M");
        let pcm_bytes = 2_400_000_u32;
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(&(36 + pcm_bytes).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt \x10\0\0\0\x01\0\x01\0");
        wav.extend_from_slice(&16_000_u32.to_le_bytes());
        wav.extend_from_slice(&32_000_u32.to_le_bytes());
        wav.extend_from_slice(b"\x02\0\x10\0data");
        wav.extend_from_slice(&pcm_bytes.to_le_bytes());
        wav.resize(wav.len() + pcm_bytes as usize, 0);
        let y4m = b"YUV4MPEG2 W2 H2 F1:1 Ip A1:1 C420\nFRAME\n\x10\x10\x10\x10\x80\x80";
        std::fs::write(&microphone, &wav).unwrap();
        std::fs::write(&camera, y4m).unwrap();
        let pending = Pending::start(
            profile.path(),
            MediaFixtures {
                microphone_path: Some(microphone.to_str().unwrap().into()),
                camera_path: Some(camera.to_str().unwrap().into()),
            },
        )
        .unwrap();
        let staged = pending.task.await.unwrap().unwrap();
        std::fs::write(microphone, b"changed").unwrap();
        let microphone = profile.path().join("microphone.wav");
        let camera = profile.path().join("camera.y4m");
        assert_eq!(std::fs::read(&microphone).unwrap(), wav);
        assert_eq!(std::fs::read(&camera).unwrap(), y4m);
        let mut command = Command::new("chrome");
        staged.configure(&mut command).unwrap();
        let expected_audio = format!("--use-file-for-fake-audio-capture={}", microphone.display());
        let expected_video = format!("--use-file-for-fake-video-capture={}", camera.display());
        assert_eq!(
            command.as_std().get_args().collect::<Vec<_>>(),
            [
                "--use-fake-device-for-media-stream",
                "--disable-features=AudioServiceOutOfProcess",
                &expected_audio,
                &expected_video,
            ]
        );
    }

    #[test]
    fn media_fixture_refuses_wrong_format_oversize_and_symlinks_without_publishing() {
        let files = tempfile::tempdir().unwrap();
        let profile = tempfile::tempdir().unwrap();
        let wrong = files.path().join("invalid.wav");
        std::fs::write(&wrong, b"invalid capture file").unwrap();
        assert_eq!(
            stage(
                &wrong,
                profile.path(),
                Input::Microphone,
                &AtomicBool::new(false)
            ),
            Err("MEDIA_FIXTURE_FORMAT")
        );
        #[cfg(unix)]
        {
            let link = files.path().join("link.wav");
            std::os::unix::fs::symlink(&wrong, &link).unwrap();
            assert_eq!(
                stage(
                    &link,
                    profile.path(),
                    Input::Microphone,
                    &AtomicBool::new(false)
                ),
                Err("MEDIA_FIXTURE_NOT_REGULAR")
            );
        }
        let large = files.path().join("large.wav");
        fs::File::create(&large)
            .unwrap()
            .set_len(MAX_MEDIA_FIXTURE_BYTES + 1)
            .unwrap();
        assert_eq!(
            stage(
                &large,
                profile.path(),
                Input::Microphone,
                &AtomicBool::new(false)
            ),
            Err("MEDIA_FIXTURE_SIZE")
        );
        assert_eq!(std::fs::read_dir(profile.path()).unwrap().count(), 0);
    }

    #[test]
    fn media_fixture_camera_uses_the_signature_for_chromes_extension() {
        let files = tempfile::tempdir().unwrap();
        let source = files.path().join("sample.mjpg");
        let profile = tempfile::tempdir().unwrap();
        let header = b"\xff\xd8\xff\xe0\x00\x10JFIF\x00\x01";
        fs::write(&source, header).unwrap();
        let staged = stage(
            &source,
            profile.path(),
            Input::Camera,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(staged, profile.path().join("camera.mjpeg"));
        assert_eq!(fs::read(staged).unwrap(), header);
    }
}

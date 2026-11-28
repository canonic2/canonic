//! Selected-target adb transport, discovery and current capture geometry.
use super::{Android, Device};
use crate::{
    error::{Error, Result},
    process,
};
use std::{collections::BTreeMap, time::Duration};

impl Android {
    pub async fn devices(&self) -> Result<Vec<Device>> {
        self.devices_bounded(Duration::from_secs(10)).await
    }
    pub(crate) async fn devices_bounded(&self, timeout: Duration) -> Result<Vec<Device>> {
        let output = process::run(&self.adb, &["devices", "-l"], timeout, 1024 * 1024).await?;
        let text = std::str::from_utf8(&output.bytes)
            .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid adb discovery response"))?;
        Ok(text
            .lines()
            .filter_map(|line| {
                if line.starts_with("List of devices") || line.trim().is_empty() {
                    return None;
                }
                let mut fields = line.split_whitespace();
                let serial = fields.next()?.to_owned();
                let state = fields.next()?.to_owned();
                let extra: BTreeMap<_, _> = fields.filter_map(|f| f.split_once(':')).collect();
                // adb reports `usb:<bus>` for cabled devices and nothing for
                // emulators or wireless debugging; the serial names an emulator.
                let link = if extra.contains_key("usb") {
                    "usb"
                } else if serial.starts_with("emulator-") {
                    "emulator"
                } else {
                    "other"
                };
                Some(Device {
                    serial: serial.clone(),
                    link: link.into(),
                    state,
                    name: extra
                        .get("model")
                        .copied()
                        .unwrap_or(&serial)
                        .replace('_', " "),
                    transport: extra
                        .get("transport_id")
                        .copied()
                        .unwrap_or("unknown")
                        .to_owned(),
                })
            })
            .collect())
    }
    pub async fn shell(
        &self,
        serial: &str,
        args: &[&str],
        timeout: Duration,
        max_bytes: usize,
    ) -> Result<Vec<u8>> {
        validate_serial(serial)?;
        let command = args
            .iter()
            .map(|arg| quote(arg))
            .collect::<Vec<_>>()
            .join(" ");
        Ok(process::run(
            &self.adb,
            &["-s", serial, "shell", &command],
            timeout,
            max_bytes,
        )
        .await?
        .bytes)
    }
    /// Run one fixed, caller-independent shell script on the selected target.
    /// Callers pass constants only; caller text goes through [`Self::shell`].
    pub(crate) async fn script(
        &self,
        serial: &str,
        script: &str,
        timeout: Duration,
        max_bytes: usize,
    ) -> Result<Vec<u8>> {
        validate_serial(serial)?;
        Ok(process::run(
            &self.adb,
            &["-s", serial, "shell", script],
            timeout,
            max_bytes,
        )
        .await?
        .bytes)
    }
    /// A PNG of the screen. The device's own PNG encoder takes most of a
    /// capture's time, so the raw pixels are fetched and encoded here; a raw
    /// layout this host does not recognize falls back to the device's PNG.
    pub async fn screenshot(&self, serial: &str) -> Result<Vec<u8>> {
        Ok(self.capture(serial).await?.0)
    }
    /// [`Self::screenshot`] with where its time went: the path taken and the
    /// transfer and host encode times, in milliseconds.
    pub(crate) async fn capture(&self, serial: &str) -> Result<(Vec<u8>, serde_json::Value)> {
        validate_serial(serial)?;
        let started = std::time::Instant::now();
        let ms = |since: std::time::Instant| since.elapsed().as_secs_f64() * 1000.0;
        let raw = process::run(
            &self.adb,
            &["-s", serial, "exec-out", "screencap"],
            Duration::from_secs(5),
            64 * 1024 * 1024,
        )
        .await?
        .bytes;
        if raw.is_empty() {
            let error = Error::new("CAPTURE_FAILED", "Screen capture returned no image");
            return Err(self.capture_refusal(serial, &raw, error).await);
        }
        let transfer = ms(started);
        let raw_bytes = raw.len();
        let encoding = std::time::Instant::now();
        let encoded = tokio::task::spawn_blocking(move || raw_png(&raw))
            .await
            .ok()
            .flatten();
        if let Some(png) = encoded {
            let timings = serde_json::json!({"backend":"adb_screencap_raw","transferMs":transfer,"transferBytes":raw_bytes,"encodeMs":ms(encoding),"totalMs":ms(started)});
            return Ok((png, timings));
        }
        let fallback = std::time::Instant::now();
        let output = process::run(
            &self.adb,
            &["-s", serial, "exec-out", "screencap", "-p"],
            Duration::from_secs(5),
            64 * 1024 * 1024,
        )
        .await?;
        if let Err(error) = crate::ui::png_dimensions(&output.bytes) {
            return Err(self.capture_refusal(serial, &output.bytes, error).await);
        }
        let timings = serde_json::json!({"backend":"adb_screencap_png","rawAttemptMs":transfer,"transferMs":ms(fallback),"transferBytes":output.bytes.len(),"totalMs":ms(started)});
        Ok((output.bytes, timings))
    }
    /// Explains a capture with no usable image. Android returns nothing for a
    /// focused window that sets `FLAG_SECURE`; that is the app's choice, not a
    /// broken device, so it gets its own code.
    async fn capture_refusal(&self, serial: &str, bytes: &[u8], error: Error) -> Error {
        let windows = self
            .script(serial, WINDOWS, Duration::from_secs(5), 4 * 1024 * 1024)
            .await
            .ok();
        if let Some(package) =
            windows.and_then(|text| secure_focus(&String::from_utf8_lossy(&text)))
        {
            return Error::new(
                "CAPTURE_BLOCKED",
                format!("{package} marks its window as secure, so Android returns no screenshot while it is in front; actions still run, and screenshots work again when another app is in front"),
            )
            .details(serde_json::json!({"package":package,"reason":"flag_secure"}));
        }
        if bytes.is_empty() {
            return Error::new("CAPTURE_FAILED", "Screen capture returned no image");
        }
        error
    }
}

/// Raw `screencap` output encoded as PNG. The header holds width, height and
/// pixel format as little-endian words, plus a color space word on newer
/// releases; RGBA and RGBX pixels follow. Anything else is not recognized.
pub(super) fn raw_png(bytes: &[u8]) -> Option<Vec<u8>> {
    use image::{
        ExtendedColorType, ImageEncoder,
        codecs::png::{CompressionType, FilterType, PngEncoder},
    };
    let word = |index: usize| {
        let field = bytes.get(index * 4..index * 4 + 4)?;
        Some(u32::from_le_bytes(field.try_into().ok()?))
    };
    let (width, height, format) = (word(0)?, word(1)?, word(2)?);
    const RGBA_8888: u32 = 1;
    const RGBX_8888: u32 = 2;
    if !(1..=8192).contains(&width)
        || !(1..=8192).contains(&height)
        || !matches!(format, RGBA_8888 | RGBX_8888)
    {
        return None;
    }
    let size = width as usize * height as usize * 4;
    let header = bytes.len().checked_sub(size)?;
    if !matches!(header, 12 | 16) {
        return None;
    }
    let mut pixels = bytes[header..].to_vec();
    if format == RGBX_8888 {
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
    }
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::Sub)
        .write_image(&pixels, width, height, ExtendedColorType::Rgba8)
        .ok()?;
    Some(png)
}

/// Window records and the focused window, in one round trip.
const WINDOWS: &str = "dumpsys window windows; dumpsys window | grep -m 1 mCurrentFocus=";

/// The focused window's package when its flags include `FLAG_SECURE`.
pub(super) fn secure_focus(dump: &str) -> Option<String> {
    let focus = dump
        .lines()
        .find_map(|line| line.trim().strip_prefix("mCurrentFocus=Window{"))?;
    let mut fields = focus.split_whitespace();
    let token = fields.next()?;
    let package = fields.nth(1)?.split('/').next()?.trim_end_matches('}');
    let mut lines = dump.lines().skip_while(|line| {
        !(line.trim().starts_with("Window #") && line.contains(&format!("Window{{{token} ")))
    });
    lines.next()?;
    let secure = lines
        .take_while(|line| !line.trim().starts_with("Window #"))
        .any(|line| super::window::flags(line) == Some(super::window::Flags::Secure));
    secure.then(|| package.to_owned())
}

impl Android {
    /// The current, rotation-following display size that screencap and input
    /// use. The input service's viewport costs one small shell read; a full
    /// screencap (over a second on some phones) is the fallback when the
    /// viewport cannot be read. `wm size` reports the natural orientation and
    /// cannot map points from a landscape image.
    pub async fn dimensions(&self, serial: &str) -> Result<(u32, u32)> {
        if let Ok(output) = self
            .shell(
                serial,
                &["dumpsys", "input"],
                Duration::from_secs(3),
                8 * 1024 * 1024,
            )
            .await
            && let Some(size) = viewport_size(&String::from_utf8_lossy(&output))
        {
            return Ok(size);
        }
        crate::ui::png_dimensions(&self.screenshot(serial).await?)
    }
}

/// The internal display's logical frame from `dumpsys input`, as width and
/// height. Newer releases label it `Viewport INTERNAL:`, older ones
/// `Viewport:`; the logical frame already follows rotation.
fn viewport_size(dump: &str) -> Option<(u32, u32)> {
    let line = dump
        .lines()
        .find(|line| line.contains("Viewport INTERNAL:"))
        .or_else(|| {
            dump.lines()
                .find(|line| line.contains("Viewport:") && line.contains("displayId=0"))
        })?;
    let frame = line.split("logicalFrame=[").nth(1)?.split(']').next()?;
    let values: Vec<i64> = frame
        .split(',')
        .map(|value| value.trim().parse().ok())
        .collect::<Option<_>>()?;
    let [left, top, right, bottom] = values[..] else {
        return None;
    };
    let width = u32::try_from(right - left).ok().filter(|v| *v > 0)?;
    let height = u32::try_from(bottom - top).ok().filter(|v| *v > 0)?;
    Some((width, height))
}

pub(super) fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
pub(super) fn validate_serial(serial: &str) -> Result<()> {
    if serial.is_empty() || serial.len() > 256 || serial.chars().any(char::is_control) {
        return Err(Error::invalid("Invalid Android target identity"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::viewport_size;

    #[test]
    fn viewport_size_follows_the_logical_frame_and_rotation() {
        let portrait = "  Viewport INTERNAL: displayId=0, uniqueId=local:1, port=129, orientation=0, logicalFrame=[0, 0, 1080, 2280], physicalFrame=[0, 0, 1440, 3040], deviceSize=[1440, 3040], isActive=[1]";
        assert_eq!(viewport_size(portrait), Some((1080, 2280)));
        let landscape = "Viewport INTERNAL: displayId=0, orientation=1, logicalFrame=[0, 0, 2280, 1080], physicalFrame=[0, 0, 3040, 1440]";
        assert_eq!(viewport_size(landscape), Some((2280, 1080)));
        let older = "Viewport: displayId=0, orientation=0, logicalFrame=[0, 0, 720, 1280], physicalFrame=[0, 0, 720, 1280]";
        assert_eq!(viewport_size(older), Some((720, 1280)));
        for unusable in [
            "",
            "Viewport INTERNAL: logicalFrame=[0, 0, 0, 0]",
            "Viewport INTERNAL: logicalFrame=[a, b]",
        ] {
            assert_eq!(viewport_size(unusable), None, "{unusable}");
        }
    }
}

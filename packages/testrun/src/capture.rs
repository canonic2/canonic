//! Portable image conversion, strict crop/scale bounds and model-sized attachments.
use crate::error::{Error, Result};
use image::{DynamicImage, ImageFormat, ImageReader, imageops::FilterType};
use serde::Deserialize;
use std::io::Cursor;

pub(crate) mod settling;

/// A crop in normalized display coordinates.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
pub struct Image {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub mime: &'static str,
}
fn decode(bytes: &[u8]) -> Result<DynamicImage> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| Error::new("INVALID_ARTIFACT", "Cannot identify captured image format"))?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(128 * 1024 * 1024);
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    reader.limits(limits);
    reader.decode().map_err(|_| {
        Error::new(
            "CAPTURE_TOO_LARGE",
            "Captured image could not be decoded within its resource limits",
        )
    })
}
pub fn dimensions(bytes: &[u8]) -> Result<(u32, u32)> {
    let dimensions = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| Error::new("INVALID_ARTIFACT", "Cannot identify captured image"))?
        .into_dimensions()
        .map_err(|_| Error::new("INVALID_ARTIFACT", "Cannot read capture dimensions"))?;
    if dimensions.0 == 0 || dimensions.1 == 0 || dimensions.0 > 8192 || dimensions.1 > 8192 {
        return Err(Error::new(
            "CAPTURE_TOO_LARGE",
            "Capture dimensions exceed resource limits",
        ));
    }
    Ok(dimensions)
}
fn encode(image: DynamicImage, format: &str) -> Result<Image> {
    let (codec, mime) = match format {
        "png" => (ImageFormat::Png, "image/png"),
        "jpeg" => (ImageFormat::Jpeg, "image/jpeg"),
        _ => return Err(Error::invalid("Image format must be png or jpeg")),
    };
    let width = image.width();
    let height = image.height();
    let mut bytes = Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, codec)
        .map_err(|_| Error::new("CAPTURE_ENCODING_FAILED", "Cannot encode captured image"))?;
    let bytes = bytes.into_inner();
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(Error::new(
            "CAPTURE_TOO_LARGE",
            "Encoded capture exceeds 64 MiB",
        ));
    }
    Ok(Image {
        bytes,
        width,
        height,
        mime,
    })
}
pub fn transform(bytes: &[u8], format: &str, scale: f64, region: Option<&Region>) -> Result<Image> {
    if !scale.is_finite() || scale <= 0.0 || scale > 1.0 {
        return Err(Error::invalid(
            "scale must be greater than zero and at most one",
        ));
    }
    let mut image = decode(bytes)?;
    if let Some(region) = region {
        if [region.x, region.y, region.width, region.height]
            .into_iter()
            .any(|v| !v.is_finite())
            || region.x < 0.0
            || region.y < 0.0
            || region.width <= 0.0
            || region.height <= 0.0
            || region.x + region.width > 1.0
            || region.y + region.height > 1.0
        {
            return Err(Error::invalid(
                "Region must fit wholly within the current display",
            ));
        }
        let x = (region.x * f64::from(image.width())).floor() as u32;
        let y = (region.y * f64::from(image.height())).floor() as u32;
        let right = ((region.x + region.width) * f64::from(image.width())).ceil() as u32;
        let bottom = ((region.y + region.height) * f64::from(image.height())).ceil() as u32;
        image = image.crop_imm(x, y, right - x, bottom - y);
    }
    if scale < 1.0 {
        let width = (f64::from(image.width()) * scale).floor().max(1.0) as u32;
        let height = (f64::from(image.height()) * scale).floor().max(1.0) as u32;
        image = image.resize_exact(width, height, FilterType::Triangle);
    }
    encode(image, format)
}
pub fn thumbnail(bytes: &[u8]) -> Result<Image> {
    let original = decode(bytes)?;
    let mut bound = 1600.min(original.width().max(original.height()));
    loop {
        let image = original.resize(bound, bound, FilterType::Triangle);
        let encoded = encode(image, "png")?;
        if encoded.bytes.len() <= 2 * 1024 * 1024 {
            return Ok(encoded);
        }
        if bound <= 400 {
            return Err(Error::new(
                "CAPTURE_TOO_LARGE",
                "Cannot fit screenshot attachment within 2 MiB",
            ));
        }
        bound = bound * 3 / 4;
    }
}
/// Mean per-pixel brightness change (0–255) below which two samples count as
/// the same picture. Encoder refinement and blinking cursors stay below it.
const STILL_DIFFERENCE: f64 = 1.5;

/// Tracks whether successive screen samples stay visually unchanged.
/// Each sample is compared with the first picture of the current still
/// period, so slow fades that differ little frame to frame still count as change.
pub struct Stability {
    quiet: std::time::Duration,
    reference: Option<(image::GrayImage, std::time::Instant)>,
    differences: Vec<f64>,
}
impl Stability {
    pub fn new(quiet: std::time::Duration) -> Self {
        Self {
            quiet,
            reference: None,
            differences: Vec::new(),
        }
    }
    /// Adds one encoded sample taken at `at`; true once the picture has stayed
    /// the same for the quiet period. Undecodable samples restart the period.
    #[cfg(test)]
    pub fn sample(&mut self, bytes: &[u8], at: std::time::Instant) -> bool {
        let Ok(image) = decode(bytes) else {
            self.reference = None;
            return false;
        };
        let small = image.resize_exact(60, 130, FilterType::Triangle).to_luma8();
        self.sample_image(small, at)
    }
    fn sample_image(&mut self, small: image::GrayImage, at: std::time::Instant) -> bool {
        match &self.reference {
            Some((reference, since)) => {
                let difference = difference(reference, &small);
                self.differences.push((difference * 100.0).round() / 100.0);
                if difference > STILL_DIFFERENCE {
                    self.reference = Some((small, at));
                    false
                } else {
                    at.duration_since(*since) >= self.quiet
                }
            }
            None => {
                self.reference = Some((small, at));
                false
            }
        }
    }
}
/// Mean absolute brightness difference of two equally sized grayscale images.
fn difference(a: &image::GrayImage, b: &image::GrayImage) -> f64 {
    let total: u64 = a
        .as_raw()
        .iter()
        .zip(b.as_raw())
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum();
    total as f64 / a.as_raw().len().max(1) as f64
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn png(shade: impl Fn(u32, u32) -> u8) -> Vec<u8> {
        let image = image::GrayImage::from_fn(120, 260, |x, y| image::Luma([shade(x, y)]));
        let mut bytes = Vec::new();
        DynamicImage::ImageLuma8(image)
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        bytes
    }

    #[test]
    fn a_still_picture_settles_after_the_quiet_period_and_motion_restarts_it() {
        let quiet = std::time::Duration::from_millis(300);
        let start = std::time::Instant::now();
        let at = |ms| start + std::time::Duration::from_millis(ms);
        let still = png(|x, _| (x * 2) as u8);
        // A sheet sliding in: the lower half turns white.
        let moved = png(|x, y| if y > 130 { 255 } else { (x * 2) as u8 });
        let mut stability = Stability::new(quiet);
        assert!(!stability.sample(&still, at(0)));
        assert!(!stability.sample(&still, at(100)));
        assert!(
            !stability.sample(&moved, at(200)),
            "motion restarts the period"
        );
        assert!(!stability.sample(&moved, at(400)));
        assert!(stability.sample(&moved, at(500)));
        assert!(stability.differences[1] > STILL_DIFFERENCE);
    }

    #[test]
    fn slow_fades_count_as_change_even_when_each_step_is_small() {
        let quiet = std::time::Duration::from_millis(300);
        let start = std::time::Instant::now();
        let mut stability = Stability::new(quiet);
        let mut settled = false;
        // Brightness rises one level per sample: each step is below the
        // threshold, but the drift from the reference is not.
        for step in 0..10u8 {
            let frame = png(|_, _| 100 + step);
            settled |= stability.sample(
                &frame,
                start + std::time::Duration::from_millis(u64::from(step) * 100),
            );
        }
        assert!(!settled);
    }

    #[test]
    fn registered_capture_bounds_match_the_image_transformer() {
        let source = encode(DynamicImage::new_rgb8(4, 4), "png").unwrap();
        let definition = crate::catalog::definition("screenshot").unwrap();
        let schema = &definition["inputSchema"];
        for scale in [-1.0, 0.0, 0.25, 1.0, 1.1] {
            let input = json!({"runId":"run","goalId":"goal","sessionId":"session","scale":scale});
            assert_eq!(
                crate::schema::validate(schema, &input).is_ok(),
                transform(&source.bytes, "png", scale, None).is_ok(),
                "scale {scale}"
            );
        }
        for (width, height) in [(0.0, 1.0), (1.0, 0.0), (0.5, 0.5), (1.0, 1.0)] {
            let region = json!({"x":0,"y":0,"width":width,"height":height});
            let input =
                json!({"runId":"run","goalId":"goal","sessionId":"session","region":region});
            let crop: Region = serde_json::from_value(region).unwrap();
            assert_eq!(
                crate::schema::validate(schema, &input).is_ok(),
                transform(&source.bytes, "png", 1.0, Some(&crop)).is_ok(),
                "crop {width}x{height}"
            );
        }
    }

    #[test]
    fn compact_jpeg_attachments_keep_their_size_and_geometry_headers() {
        let jpeg = encode(DynamicImage::new_rgb8(443, 960), "jpeg").unwrap();
        assert_eq!(dimensions(&jpeg.bytes).unwrap(), (443, 960));
        let attachment = thumbnail(&jpeg.bytes).unwrap();
        assert_eq!((attachment.width, attachment.height), (443, 960));
        assert!(dimensions(b"not an image").is_err());
        let smaller = transform(&jpeg.bytes, "jpeg", 0.5, None).unwrap();
        assert_eq!((smaller.width, smaller.height), (221, 480));
    }
    #[test]
    fn crops_do_not_clip_and_thumbnails_preserve_original_resolution() {
        let source = DynamicImage::new_rgb8(2000, 1000);
        let original = encode(source, "png").unwrap();
        let thumbnail = thumbnail(&original.bytes).unwrap();
        assert_eq!((thumbnail.width, thumbnail.height), (1600, 800));
        let region = Region {
            x: 0.5,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        };
        let crop = transform(&original.bytes, "png", 0.5, Some(&region)).unwrap();
        assert_eq!((crop.width, crop.height), (500, 500));
        let invalid = Region {
            x: 0.8,
            y: 0.0,
            width: 0.5,
            height: 1.0,
        };
        assert!(transform(&original.bytes, "png", 1.0, Some(&invalid)).is_err());
        assert_eq!(
            (
                decode(&original.bytes).unwrap().width(),
                decode(&original.bytes).unwrap().height()
            ),
            (2000, 1000)
        );
    }
}

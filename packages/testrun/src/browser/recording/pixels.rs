//! Bounded JPEG decoding and an even, limited-range NV12 recording canvas.
use crate::error::{Error, Result};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits, imageops};
use std::io::Cursor;

fn valid_size(width: u32, height: u32) -> bool {
    (1..=8192).contains(&width)
        && (1..=8192).contains(&height)
        && u64::from(width) * u64::from(height) <= 8_000_000
}

pub(super) fn nv12(jpeg: &[u8], width: u32, height: u32) -> Result<Vec<u8>> {
    let invalid = || Error::new("INVALID_MEDIA_FRAME", "Invalid JPEG or recording canvas");
    if !valid_size(width, height)
        || width % 2 != 0
        || height % 2 != 0
        || jpeg.len() > 6 * 1024 * 1024
    {
        return Err(invalid());
    }
    let mut reader = ImageReader::with_format(Cursor::new(jpeg), ImageFormat::Jpeg);
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(|_| invalid())?;
    let (source_width, source_height) = decoder.dimensions();
    if !valid_size(source_width, source_height) {
        return Err(invalid());
    }
    let image = DynamicImage::from_decoder(decoder)
        .map_err(|_| invalid())?
        .into_rgb8();
    let (fit_width, fit_height) = if width * source_height <= height * source_width {
        (width, (width * source_height / source_width).max(2) & !1)
    } else {
        ((height * source_width / source_height).max(2) & !1, height)
    };
    let image = if (source_width, source_height) == (fit_width, fit_height) {
        image
    } else {
        imageops::resize(
            &image,
            fit_width,
            fit_height,
            imageops::FilterType::Triangle,
        )
    };
    // NV12 shares chroma across a 2x2 block. Place the fitted image on the same
    // grid so a block never mixes content with the black letterbox.
    let x_offset = (((width - fit_width) / 2) & !1) as usize;
    let y_offset = (((height - fit_height) / 2) & !1) as usize;
    let stride = width as usize;
    let luma_bytes = stride * height as usize;
    let mut output = vec![128; luma_bytes + luma_bytes / 2];
    output[..luma_bytes].fill(16);
    for y in (0..fit_height).step_by(2) {
        for x in (0..fit_width).step_by(2) {
            let mut sum = [0i32; 3];
            let start = (y_offset + y as usize) * stride + x_offset + x as usize;
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let [r, g, b] = image.get_pixel(x + dx, y + dy).0.map(i32::from);
                output[start + dy as usize * stride + dx as usize] =
                    (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16) as u8;
                sum[0] += r;
                sum[1] += g;
                sum[2] += b;
            }
            let [r, g, b] = sum;
            let uv = luma_bytes + (y_offset + y as usize) / 2 * stride + x_offset + x as usize;
            output[uv] = (((-38 * r - 74 * g + 112 * b + 512) >> 10) + 128) as u8;
            output[uv + 1] = (((112 * r - 94 * g - 18 * b + 512) >> 10) + 128) as u8;
        }
    }
    Ok(output)
}

#[cfg(test)]
#[test]
fn nv12_keeps_black_bars_and_top_down_luma() {
    let source = image::RgbImage::from_fn(8, 4, |_, y| {
        image::Rgb(if y < 2 { [255; 3] } else { [96; 3] })
    });
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 100)
        .encode_image(&source)
        .unwrap();
    let output = nv12(&jpeg, 8, 8).unwrap();
    assert_eq!(output.len(), 8 * 8 * 3 / 2);
    assert_eq!(&output[..16], &[16; 16]);
    assert!(output[16..32].iter().all(|y| *y >= 230));
    assert!(output[32..48].iter().all(|y| (95..=103).contains(y)));
    assert_eq!(&output[48..64], &[16; 16]);
    assert_eq!(&output[64..], &[128; 32]);
    assert!(nv12(&jpeg, 7, 8).is_err());
    assert!(nv12(b"not a JPEG", 8, 8).is_err());
}

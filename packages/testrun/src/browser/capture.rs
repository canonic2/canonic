//! Immutable private capture records, decoded publication and bounded evidence comparisons.
use super::{
    contract::Reference,
    page::{Format, Scale, Screenshot},
    session::Error,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read as _,
    path::{Path, PathBuf},
};
use uuid::Uuid;
pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Scale a native viewport image without resizing the browser's surface.
pub(crate) async fn css_viewport(
    bytes: Vec<u8>,
    format: Format,
    quality: Option<u8>,
    width: u32,
    height: u32,
    dpr: f64,
) -> Result<Vec<u8>, Error> {
    tokio::task::spawn_blocking(move || {
        let format = match format {
            Format::Png => image::ImageFormat::Png,
            Format::Jpeg => image::ImageFormat::Jpeg,
        };
        let mut reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(160 * 1024 * 1024);
        limits.max_image_width = Some(32000);
        limits.max_image_height = Some(32000);
        reader.limits(limits);
        let image = reader
            .decode()
            .map_err(|error| Error::caused("INVALID_CAPTURE", error))?;
        if width == 0
            || height == 0
            || u64::from(width) * u64::from(height) > 32_000_000
            || u64::from(image.width()) * u64::from(image.height()) > 32_000_000
        {
            return Err("CAPTURE_BOUNDS".into());
        }
        if !dpr.is_finite()
            || dpr <= 0.0
            || f64::from(image.width()) != (f64::from(width) * dpr).round()
            || f64::from(image.height()) != (f64::from(height) * dpr).round()
        {
            return Err("GEOMETRY_UNAVAILABLE".into());
        }
        let image = image.resize_exact(width, height, image::imageops::FilterType::Lanczos3);
        let mut output = std::io::Cursor::new(Vec::new());
        if format == image::ImageFormat::Jpeg {
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, quality.unwrap_or(80))
                .encode_image(&image)
                .map_err(|error| Error::caused("INVALID_CAPTURE", error))?;
        } else {
            image
                .write_to(&mut output, format)
                .map_err(|error| Error::caused("INVALID_CAPTURE", error))?;
        }
        if output.get_ref().len() > 16 * 1024 * 1024 {
            return Err("CAPTURE_BYTE_LIMIT".into());
        }
        Ok(output.into_inner())
    })
    .await
    .map_err(|_| Error::from("CAPTURE_IO"))?
}

#[derive(Default)]
pub(crate) struct Store {
    records: BTreeMap<Uuid, Value>,
    bytes: usize,
    count: usize,
}
impl Store {
    pub fn reserve(&mut self, bytes: usize) -> Result<(), Error> {
        if bytes > 16 * 1024 * 1024 {
            return Err("CAPTURE_BYTE_LIMIT".into());
        }
        if self.count >= 64 || self.bytes.saturating_add(bytes) > 256 * 1024 * 1024 {
            return Err("CAPTURE_CAPACITY".into());
        }
        self.count += 1;
        self.bytes += bytes;
        Ok(())
    }
    pub fn insert(&mut self, result: &Value) -> Result<(), Error> {
        let id = result["captureId"]
            .as_str()
            .and_then(|id| Uuid::parse_str(id).ok())
            .ok_or("INVALID_CAPTURE")?;
        if self.records.len() >= 64 {
            return Err("CAPTURE_CAPACITY".into());
        }
        self.records.insert(id, result.clone());
        Ok(())
    }
    pub fn release_reservation(&mut self, bytes: usize) {
        self.count -= 1;
        self.bytes -= bytes;
    }
    pub fn release_unpublished_screenshot(&mut self, bytes: usize, error: &Error) {
        if !error
            .partial
            .as_ref()
            .is_some_and(|partial| partial["artifactPublished"] == true)
        {
            self.release_reservation(bytes);
        }
    }
    pub fn pair(&self, baseline: Uuid, actual: Uuid) -> Result<(Value, Value), Error> {
        Ok((
            self.records.get(&baseline).ok_or("STALE_CAPTURE")?.clone(),
            self.records.get(&actual).ok_or("STALE_CAPTURE")?.clone(),
        ))
    }
}

pub(crate) fn output_path(capture: &Screenshot) -> Result<Option<PathBuf>, Error> {
    let Some(path) = capture.output_path.as_deref() else {
        return Ok(None);
    };
    let path = Path::new(path);
    let name = path.file_name().ok_or("CAPTURE_OUTPUT_PATH_INVALID")?;
    let parent = std::fs::canonicalize(path.parent().ok_or("CAPTURE_OUTPUT_PATH_INVALID")?)
        .map_err(|_| Error::from("CAPTURE_OUTPUT_DIRECTORY_UNAVAILABLE"))?;
    crate::artifacts::check_output_directory(&parent)
        .map_err(|_| Error::from("CAPTURE_OUTPUT_DIRECTORY_UNAVAILABLE"))?;
    let output = parent.join(name);
    match std::fs::symlink_metadata(&output) {
        Ok(_) => Err("CAPTURE_OUTPUT_EXISTS".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Some(output)),
        Err(_) => Err("CAPTURE_IO".into()),
    }
}

pub(crate) fn write_screenshot(
    directory: &Path,
    output: Option<PathBuf>,
    format: Format,
    bytes: &[u8],
) -> Result<(Uuid, Value), Error> {
    #[cfg(windows)]
    let external = output.is_some();
    write_screenshot_with_io(
        directory,
        output,
        format,
        bytes,
        |path, bytes| {
            #[cfg(windows)]
            if external {
                return crate::artifacts::write_private(path, bytes);
            }
            crate::artifacts::write(path, bytes)
        },
        crate::artifacts::sync_directory,
    )
}

fn write_screenshot_with_io(
    directory: &Path,
    output: Option<PathBuf>,
    format: Format,
    bytes: &[u8],
    write_artifact: impl FnOnce(&Path, &[u8]) -> crate::error::Result<()>,
    sync_directory: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<(Uuid, Value), Error> {
    let id = Uuid::new_v4();
    let (extension, mime) = match format {
        Format::Png => ("png", "image/png"),
        Format::Jpeg => ("jpg", "image/jpeg"),
    };
    let path = if let Some(output) = output {
        let parent = output.parent().ok_or("CAPTURE_OUTPUT_PATH_INVALID")?;
        crate::artifacts::check_output_directory(parent)
            .map_err(|_| Error::from("CAPTURE_OUTPUT_DIRECTORY_UNAVAILABLE"))?;
        output
    } else {
        let directory = directory.join("captures");
        if !directory.exists() {
            crate::artifacts::directory(&directory)
                .map_err(|error| Error::caused("CAPTURE_IO", error))?;
        }
        directory.join(format!("{id}.{extension}"))
    };
    // The artifact writer only reports CLEANUP_FAILED after publishing.
    let cleanup = match write_artifact(&path, bytes) {
        Ok(()) => None,
        Err(error) if error.code == "CLEANUP_FAILED" => Some(error),
        Err(_) if std::fs::symlink_metadata(&path).is_ok() => {
            return Err("CAPTURE_OUTPUT_EXISTS".into());
        }
        Err(error) => return Err(Error::caused("CAPTURE_IO", error)),
    };
    let attachment = json!({
        "path":path,
        "mimeType":mime,
        "bytes":bytes.len(),
        "sha256":format!("{:x}", Sha256::digest(bytes)),
    });
    let synced = sync_directory(path.parent().expect("Capture output parent"));
    let durability_confirmed = synced.is_ok();
    let cleanup_failed = cleanup.is_some();
    if let Some(mut error) = cleanup
        .map(|cause| Error::caused("CAPTURE_IO", cause))
        .or_else(|| synced.err().map(|cause| Error::caused("CAPTURE_IO", cause)))
    {
        let mut partial = json!({
            "attachment":attachment,
            "artifactPublished":true,
            "durabilityConfirmed":durability_confirmed,
        });
        if cleanup_failed {
            partial["cleanupCode"] = json!("CLEANUP_FAILED");
        }
        error.partial = Some(partial);
        return Err(error);
    }
    Ok((id, attachment))
}

#[expect(
    clippy::too_many_arguments,
    reason = "One immutable publication records the acquisition scope, geometry and source"
)]
pub(crate) async fn publish(
    directory: &Path,
    page: Reference,
    frame: Reference,
    capture: &Screenshot,
    output: Option<PathBuf>,
    bytes: Vec<u8>,
    before: Value,
    after: Value,
    backend: &'static str,
    source: &'static str,
    started: u64,
) -> Result<Value, Error> {
    let directory = directory.to_path_buf();
    let capture = capture.clone();
    tokio::task::spawn_blocking(move||{
        let format=match capture.format{Format::Png=>image::ImageFormat::Png,Format::Jpeg=>image::ImageFormat::Jpeg};
        let mut reader=image::ImageReader::with_format(std::io::Cursor::new(&bytes),format);
        let mut limits=image::Limits::default();limits.max_alloc=Some(160*1024*1024);limits.max_image_width=Some(32000);limits.max_image_height=Some(32000);reader.limits(limits);
        let image=reader.decode().map_err(|error|Error::caused("INVALID_CAPTURE",error))?;
        let (width,height)=(image.width(),image.height());
        if u64::from(width)*u64::from(height)>32_000_000{return Err("CAPTURE_BOUNDS".into());}
        let scale=if matches!(capture.scale,Scale::Css){1.0}else{after["dpr"].as_f64().ok_or("GEOMETRY_UNAVAILABLE")?};
        let (id,attachment)=write_screenshot(&directory,output,capture.format,&bytes)?;
        let stable=before==after;
        let mapped=stable&&capture.is_viewport()&&after["viewport"]["width"].as_f64().is_some_and(|w|w*scale==f64::from(width))&&after["viewport"]["height"].as_f64().is_some_and(|h|h*scale==f64::from(height));
        Ok(json!({"captureId":id,"page":page,"frame":frame,"backend":backend,"attachment":attachment,"width":width,"height":height,"geometryBefore":before,"geometryAfter":after,"geometryStable":stable,"transform":if mapped{Some(json!({"cssToPixels":scale,"documentOrigin":after["scroll"],"padding":{"right":0,"bottom":0}}))}else{None},"coordinateInputAvailable":false,"source":source,"options":capture,"privacy":"local-private","complete":true,"timing":{"requestUnixMs":started,"replyUnixMs":crate::browser::capture::now_ms(),"sourceTimestamp":null,"acquisitionWithinRequestReply":true}}))
    }).await.map_err(|_|Error::from("CAPTURE_IO"))?
}

pub(crate) fn validate_comparison(
    threshold: u8,
    max_changed_ratio: f64,
) -> Result<(), &'static str> {
    let _ = threshold;
    if !max_changed_ratio.is_finite() || !(0.0..=1.0).contains(&max_changed_ratio) {
        return Err("INVALID_COMPARISON_OPTIONS");
    }
    Ok(())
}
pub(crate) async fn compare(
    directory: PathBuf,
    baseline: Value,
    actual: Value,
    threshold: u8,
    max_changed_ratio: f64,
) -> Result<Value, Error> {
    validate_comparison(threshold, max_changed_ratio)?;
    if baseline["kind"] == "snapshot" || actual["kind"] == "snapshot" {
        if baseline["kind"] != "snapshot" || actual["kind"] != "snapshot" {
            return Err("COMPARISON_KIND_MISMATCH".into());
        }
        if threshold != 0 {
            return Err("INVALID_COMPARISON_OPTIONS".into());
        }
        let mut result = super::evidence::snapshot_diff(&baseline, &actual, max_changed_ratio)?;
        result["baseline"] = baseline["captureId"].clone();
        result["actual"] = actual["captureId"].clone();
        result["sourceHashes"] = json!({"baseline":baseline["attachment"]["sha256"],"actual":actual["attachment"]["sha256"]});
        return Ok(result);
    }
    tokio::task::spawn_blocking(move||{
        let read=|record:&Value|->Result<image::RgbaImage,Error>{
            let path=record["attachment"]["path"].as_str().ok_or("INVALID_CAPTURE")?;
            let metadata=std::fs::symlink_metadata(path).map_err(|error|Error::caused("CAPTURE_IO",error))?;
            if !metadata.is_file()||metadata.len()>16*1024*1024{return Err("CAPTURE_CHANGED".into());}
            let mut bytes=Vec::new();
            std::fs::File::open(path).and_then(|file|file.take(16*1024*1024+1).read_to_end(&mut bytes)).map_err(|error|Error::caused("CAPTURE_IO",error))?;
            if bytes.len()>16*1024*1024{return Err("CAPTURE_CHANGED".into());}
            if record["attachment"]["sha256"]!=format!("{:x}",Sha256::digest(&bytes)){return Err("CAPTURE_CHANGED".into());}
            image::load_from_memory(&bytes).map(|image|image.to_rgba8()).map_err(|error|Error::caused("INVALID_CAPTURE",error))
        };
        let a=read(&baseline)?;let b=read(&actual)?;
        if a.dimensions()!=b.dimensions(){return Err("COMPARISON_GEOMETRY_MISMATCH".into());}
        let mut output=image::RgbaImage::new(a.width(),a.height());let mut changed=0u64;
        for ((a,b),out) in a.pixels().zip(b.pixels()).zip(output.pixels_mut()){
            let delta=a.0.iter().zip(b.0).map(|(a,b)|a.abs_diff(b)).max().unwrap_or(0);
            if delta>threshold{changed+=1;*out=image::Rgba([255,0,255,255]);}else{let grey=(u16::from(b[0])+u16::from(b[1])+u16::from(b[2]))/3;*out=image::Rgba([grey as u8,grey as u8,grey as u8,100]);}
        }
        let ratio=changed as f64/(u64::from(a.width())*u64::from(a.height())) as f64;
        let id=Uuid::new_v4();let path=directory.join(format!("comparison-{id}.png"));let mut encoded=std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(output).write_to(&mut encoded,image::ImageFormat::Png).map_err(|error|Error::caused("CAPTURE_IO",error))?;
        if encoded.get_ref().len()>16*1024*1024{return Err("CAPTURE_BYTE_LIMIT".into());}
        crate::artifacts::write(&path,encoded.get_ref()).map_err(|error|Error::caused("CAPTURE_IO",error))?;
        Ok(json!({"comparisonId":id,"baseline":baseline["captureId"],"actual":actual["captureId"],"sourceHashes":{"baseline":baseline["attachment"]["sha256"],"actual":actual["attachment"]["sha256"]},"changedPixels":changed,"changedRatio":ratio,"threshold":threshold,"maxChangedRatio":max_changed_ratio,"matches":ratio<=max_changed_ratio,"attachment":{"path":path,"mimeType":"image/png","bytes":encoded.get_ref().len(),"sha256":format!("{:x}",Sha256::digest(encoded.get_ref()))},"privacy":"local-private"}))
    }).await.map_err(|_|Error::from("CAPTURE_IO"))?
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_screenshot_reports_its_attachment_when_directory_sync_fails() {
        let root = tempfile::tempdir().unwrap();
        let evidence = root.path().join("evidence");
        crate::artifacts::directory(&evidence).unwrap();
        let path = evidence.join("checkout.png");
        let bytes = b"verified screenshot bytes";
        let error = write_screenshot_with_io(
            root.path(),
            Some(path.clone()),
            Format::Png,
            bytes,
            crate::artifacts::write,
            |directory| {
                assert_eq!(directory, evidence);
                assert_eq!(std::fs::read(&path).unwrap(), bytes);
                Err(std::io::Error::other("fixture directory sync failure"))
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "CAPTURE_IO");
        let partial = error.partial.unwrap();
        assert_eq!(partial["artifactPublished"], true);
        assert_eq!(partial["durabilityConfirmed"], false);
        assert_eq!(partial["attachment"]["path"], json!(path));
        assert_eq!(partial["attachment"]["bytes"], bytes.len());
        assert_eq!(
            partial["attachment"]["sha256"],
            format!("{:x}", Sha256::digest(bytes))
        );
        assert_eq!(
            write_screenshot(root.path(), Some(path.clone()), Format::Png, b"replacement")
                .unwrap_err()
                .code,
            "CAPTURE_OUTPUT_EXISTS"
        );
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    #[test]
    fn published_screenshot_reports_its_attachment_when_staging_cleanup_fails() {
        let root = tempfile::tempdir().unwrap();
        let evidence = root.path().join("evidence");
        crate::artifacts::directory(&evidence).unwrap();
        let path = evidence.join("checkout.png");
        let bytes = b"verified screenshot bytes";
        let error = write_screenshot_with_io(
            root.path(),
            Some(path.clone()),
            Format::Png,
            bytes,
            |path, bytes| {
                crate::artifacts::write(path, bytes)?;
                Err(crate::error::Error::new(
                    "CLEANUP_FAILED",
                    "Fixture staging cleanup failure",
                ))
            },
            |directory| {
                assert_eq!(directory, evidence);
                assert_eq!(std::fs::read(&path).unwrap(), bytes);
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "CAPTURE_IO");
        let partial = error.partial.as_ref().unwrap();
        assert_eq!(partial["artifactPublished"], true);
        assert_eq!(partial["durabilityConfirmed"], true);
        assert_eq!(partial["cleanupCode"], "CLEANUP_FAILED");
        assert_eq!(partial["attachment"]["path"], json!(path));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);

        let mut store = Store::default();
        for _ in 0..64 {
            store.reserve(bytes.len()).unwrap();
        }
        store.release_unpublished_screenshot(bytes.len(), &error);
        assert_eq!(
            store.reserve(bytes.len()).unwrap_err().code,
            "CAPTURE_CAPACITY"
        );
        store.release_unpublished_screenshot(bytes.len(), &Error::from("CAPTURE_OUTPUT_EXISTS"));
        store.reserve(bytes.len()).unwrap();
    }
    #[tokio::test]
    async fn css_viewport_resampling_accepts_fractional_dpr_pixel_rounding() {
        for (width, height, dpr, physical_width, physical_height) in
            [(81, 63, 1.25, 101, 79), (7, 5, 0.5, 4, 3)]
        {
            let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                physical_width,
                physical_height,
                image::Rgb([255, 0, 0]),
            ));
            let mut input = std::io::Cursor::new(Vec::new());
            image.write_to(&mut input, image::ImageFormat::Png).unwrap();
            let bytes = input.into_inner();
            let output = css_viewport(bytes.clone(), Format::Png, None, width, height, dpr)
                .await
                .unwrap();
            let decoded = image::load_from_memory(&output).unwrap().to_rgb8();
            assert_eq!(decoded.dimensions(), (width, height));
            assert!(decoded.pixels().all(|pixel| pixel.0 == [255, 0, 0]));
            assert_eq!(
                css_viewport(bytes, Format::Png, None, width, height + 2, dpr)
                    .await
                    .unwrap_err()
                    .code,
                "GEOMETRY_UNAVAILABLE"
            );
        }
    }
    #[tokio::test]
    async fn css_viewport_resampling_preserves_pixels_and_format_and_refuses_invalid_bounds() {
        for format in [Format::Png, Format::Jpeg] {
            let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                8,
                4,
                image::Rgb([255, 0, 0]),
            ));
            let mut input = std::io::Cursor::new(Vec::new());
            image
                .write_to(
                    &mut input,
                    match format {
                        Format::Png => image::ImageFormat::Png,
                        Format::Jpeg => image::ImageFormat::Jpeg,
                    },
                )
                .unwrap();
            let bytes = input.into_inner();
            let output = css_viewport(bytes.clone(), format, Some(90), 4, 2, 2.0)
                .await
                .unwrap();
            assert_eq!(
                image::guess_format(&output).unwrap(),
                match format {
                    Format::Png => image::ImageFormat::Png,
                    Format::Jpeg => image::ImageFormat::Jpeg,
                }
            );
            let decoded = image::load_from_memory(&output).unwrap().to_rgb8();
            assert_eq!(decoded.dimensions(), (4, 2));
            for pixel in decoded.pixels() {
                assert!(pixel[0] > 250 && pixel[1] < 5 && pixel[2] < 5);
            }
            assert_eq!(
                css_viewport(bytes.clone(), format, None, 0, 2, 2.0)
                    .await
                    .unwrap_err()
                    .code,
                "CAPTURE_BOUNDS"
            );
            assert_eq!(
                css_viewport(bytes, format, None, 4, 2, 1.0)
                    .await
                    .unwrap_err()
                    .code,
                "GEOMETRY_UNAVAILABLE"
            );
        }
    }
    #[tokio::test]
    async fn capture_images_compare_pixels_thresholds_and_refuse_tampering() {
        let root = tempfile::tempdir().unwrap();
        let page = Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let encode = |pixels: Vec<u8>| {
            let image = image::RgbaImage::from_raw(2, 1, pixels).unwrap();
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(image)
                .write_to(&mut bytes, image::ImageFormat::Png)
                .unwrap();
            bytes.into_inner()
        };
        let geometry = json!({"viewport":{"width":2,"height":1},"scroll":{"x":0,"y":0},"dpr":1});
        let a = publish(
            root.path(),
            page,
            page,
            &Screenshot::default(),
            None,
            encode(vec![255, 0, 0, 255, 0, 0, 255, 255]),
            geometry.clone(),
            geometry.clone(),
            "fixture",
            "fixture",
            0,
        )
        .await
        .unwrap();
        let evidence = root.path().join("run-evidence");
        crate::artifacts::directory(&evidence).unwrap();
        let destination = evidence.join("checkout.png");
        let capture = Screenshot {
            output_path: Some(destination.to_str().unwrap().into()),
            ..Default::default()
        };
        capture.validate().unwrap();
        let output = output_path(&capture).unwrap();
        let b = publish(
            root.path(),
            page,
            page,
            &capture,
            output.clone(),
            encode(vec![254, 0, 0, 255, 0, 0, 255, 255]),
            geometry.clone(),
            geometry,
            "fixture",
            "fixture",
            0,
        )
        .await
        .unwrap();
        assert_eq!(b["attachment"]["path"], json!(output.as_ref().unwrap()));
        let saved = std::fs::read(&destination).unwrap();
        assert_eq!(
            b["attachment"]["sha256"],
            format!("{:x}", Sha256::digest(&saved))
        );
        assert_eq!(
            output_path(&capture).unwrap_err().code,
            "CAPTURE_OUTPUT_EXISTS"
        );
        assert_eq!(
            write_screenshot(root.path(), output, Format::Png, b"replacement")
                .unwrap_err()
                .code,
            "CAPTURE_OUTPUT_EXISTS"
        );
        assert_eq!(std::fs::read(&destination).unwrap(), saved);
        let changed = compare(root.path().into(), a.clone(), b.clone(), 0, 0.0)
            .await
            .unwrap();
        assert_eq!(changed["changedPixels"], 1);
        assert_eq!(changed["matches"], false);
        let tolerated = compare(root.path().into(), a.clone(), b.clone(), 1, 0.0)
            .await
            .unwrap();
        assert_eq!(tolerated["matches"], true);
        std::fs::write(b["attachment"]["path"].as_str().unwrap(), b"Changed").unwrap();
        assert_eq!(
            compare(root.path().into(), a, b, 0, 0.0)
                .await
                .unwrap_err()
                .code,
            "CAPTURE_CHANGED"
        );
    }
}

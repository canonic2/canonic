//! Complete physical display gestures and keyboard input over an owned HID channel.
mod keyboard;
#[cfg(test)]
mod keyboard_tests;
use crate::{
    error::{Error, Result},
    ui::Point,
};
use idevice::{
    ReadWrite, RemoteXpcClient,
    xpc::{Dictionary, XPCObject as X},
};
use serde_json::{Value, json};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

unsafe extern "C" {
    fn mach_absolute_time() -> u64;
}
pub(crate) trait Reports: Send {
    fn send(
        &mut self,
        service_id: u64,
        report: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
    fn request(
        &mut self,
        _payload: Dictionary,
        _reply: bool,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + '_>> {
        Box::pin(async {
            Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Keyboard registration is unavailable",
            ))
        })
    }
}
struct Channel(RemoteXpcClient<Box<dyn ReadWrite>>);
impl Reports for Channel {
    fn send(
        &mut self,
        service_id: u64,
        report: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            let mut send = Dictionary::new();
            send.insert("_0".into(), X::Data(report));
            send.insert("_1".into(), X::UInt64(service_id));
            let mut payload = Dictionary::new();
            payload.insert("send".into(), X::Dictionary(send));
            let mut request = envelope();
            request.insert("payload".into(), X::Dictionary(payload));
            self.0
                .send_object(request, false)
                .await
                .map_err(transport_error)
        })
    }
    fn request(
        &mut self,
        payload: Dictionary,
        reply: bool,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + Send + '_>> {
        Box::pin(async move {
            let mut request = envelope();
            request.insert("payload".into(), X::Dictionary(payload));
            self.0
                .send_object(request, reply)
                .await
                .map_err(transport_error)?;
            if !reply {
                return Ok(json!({}));
            }
            let reply = serde_json::to_value(self.0.recv().await.map_err(transport_error)?)
                .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid HID reply"))?;
            if reply.get("error").is_some_and(|error| !error.is_null()) {
                return Err(Error::new(
                    "DEVICE_INPUT_UNAVAILABLE",
                    "CoreDevice refused HID configuration",
                ));
            }
            Ok(reply)
        })
    }
}
fn envelope() -> Dictionary {
    let mut request = Dictionary::new();
    request.insert(
        "featureIdentifier".into(),
        X::String("com.apple.coredevice.feature.remote.universalhidservice".into()),
    );
    request.insert("messageType".into(), X::String("Request".into()));
    request
}
fn transport_error(error: idevice::IdeviceError) -> Error {
    Error::new(
        "DEVICE_INPUT_UNAVAILABLE",
        format!("CoreDevice HID transport: {error}"),
    )
}
fn report(contact: bool, point: [u16; 2], timestamp: u64) -> Vec<u8> {
    let mut bytes = vec![9, 1, 5, if contact { 0xc2 } else { 2 }];
    for coordinate in point {
        bytes.extend(coordinate.to_le_bytes());
    }
    bytes.extend([0; 32]);
    bytes.extend([2, 0, 0, 0]);
    bytes.extend(&timestamp.to_le_bytes()[..6]);
    bytes.extend([0; 8]);
    bytes
}
pub(crate) fn points(operation: &str, args: &Value) -> Result<([u16; 2], [u16; 2], u64)> {
    if !matches!(operation, "tap" | "long_press" | "swipe") {
        return Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "HID sessions support display taps, long presses and swipes; use xctest for semantic input",
        ));
    }
    let read = |key: &str| -> Result<[u16; 2]> {
        let point: Point = serde_json::from_value(args[key].clone())
            .map_err(|_| Error::invalid("HID requires normalized points"))?;
        Ok(point.validate()?.map(|v| (v * 65535.0).round() as u16))
    };
    let from = read(if operation == "swipe" {
        "from"
    } else {
        "point"
    })?;
    let to = if operation == "swipe" {
        read("to")?
    } else {
        from
    };
    let duration = if operation == "tap" {
        50
    } else {
        args["durationMs"].as_u64().ok_or_else(|| {
            Error::invalid("Gesture duration must be an integer between 0 and 10000 ms")
        })?
    };
    crate::ui::validate_gesture_duration(duration)?;
    if args["scope"]["kind"] != "display" {
        return Err(Error::invalid(
            "HID input requires explicit display scope; app/system guarded input requires controlPath xctest",
        ));
    }
    Ok((from, to, duration))
}
pub(crate) fn validate(operation: &str, args: &Value) -> Result<()> {
    if args["scope"]["kind"] != "display" {
        return Err(Error::invalid("HID input requires explicit display scope"));
    }
    if matches!(operation, "key" | "type_text") {
        keyboard::strokes(operation, args)?;
    } else {
        points(operation, args)?;
    }
    Ok(())
}
pub(crate) struct Hid {
    reports: tokio::sync::Mutex<Box<dyn Reports>>,
    serial: tokio::sync::Mutex<()>,
    held: Mutex<Option<[u16; 2]>>,
    keyboard: Mutex<Option<u64>>,
    keyboard_unconfirmed: AtomicBool,
    held_keys: AtomicBool,
    uncertain: AtomicBool,
    release_error: Mutex<Option<Error>>,
}
impl Hid {
    fn new(reports: Box<dyn Reports>) -> Arc<Self> {
        Arc::new(Self {
            reports: tokio::sync::Mutex::new(reports),
            serial: tokio::sync::Mutex::new(()),
            held: Mutex::new(None),
            keyboard: Mutex::new(None),
            keyboard_unconfirmed: AtomicBool::new(false),
            held_keys: AtomicBool::new(false),
            uncertain: AtomicBool::new(false),
            release_error: Mutex::new(None),
        })
    }
    pub(crate) async fn open(
        mut service: RemoteXpcClient<Box<dyn ReadWrite>>,
    ) -> Result<Arc<Self>> {
        let mut request = envelope();
        let mut payload = Dictionary::new();
        payload.insert("connectedServices".into(), X::Dictionary(Dictionary::new()));
        request.insert("payload".into(), X::Dictionary(payload));
        tokio::time::timeout(Duration::from_secs(5), async {
            service
                .send_object(request, true)
                .await
                .map_err(transport_error)?;
            let reply = serde_json::to_value(service.recv().await.map_err(transport_error)?)
                .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid HID surface response"))?;
            if !reply["connectedServices"].as_array().is_some_and(|s| {
                s.iter()
                    .any(|s| s["_ServiceID"] == 257 && s["PrimaryUsagePage"] == 13)
            }) {
                return Err(Error::new(
                    "CAPABILITY_UNAVAILABLE",
                    "Main touchscreen HID surface is absent",
                ));
            }
            Ok(Self::new(Box::new(Channel(service))))
        })
        .await
        .map_err(|_| Error::new("TIMEOUT", "HID discovery timed out"))?
    }
    async fn send(&self, contact: bool, point: [u16; 2]) -> Result<()> {
        // SAFETY: read-only monotonic host clock, no pointers or mutable native state.
        let timestamp = unsafe { mach_absolute_time() };
        tokio::time::timeout(Duration::from_secs(2), async {
            self.reports
                .lock()
                .await
                .send(257, report(contact, point, timestamp))
                .await
        })
        .await
        .map_err(|_| Error::new("OUTCOME_UNKNOWN", "HID report deadline expired"))?
    }
    async fn release(&self) -> Result<()> {
        let keys = if self.held_keys.load(Ordering::SeqCst) {
            self.send_keys(&[]).await
        } else {
            Ok(())
        };
        let point = *self.held.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(point) = point {
            if let Err(error) = self.send(false, point).await {
                *self.release_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(error.clone());
                return Err(error);
            }
            *self.held.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
        if let Err(error) = &keys {
            *self.release_error.lock().unwrap_or_else(|e| e.into_inner()) = Some(error.clone());
        }
        keys
    }
    pub(crate) async fn close(&self) -> Result<()> {
        self.uncertain.store(true, Ordering::SeqCst);
        let _serial = self.serial.lock().await;
        let release = self.release().await;
        // Removing the owned keyboard can still release its keys if the final
        // empty report failed. Attempt both operations and retain both causes.
        let removal = self.remove_keyboard().await;
        if release.is_err() || removal.is_err() {
            return Err(
                Error::new("CLEANUP_FAILED", "HID input cleanup failed").details(
                    json!({"releaseError":release.err(),"keyboardRemovalError":removal.err()}),
                ),
            );
        }
        Ok(())
    }
    async fn remove_keyboard(&self) -> Result<()> {
        if self.keyboard_unconfirmed.load(Ordering::SeqCst) {
            return Err(Error::new(
                "CLEANUP_FAILED",
                "HID keyboard registration outcome is unknown; cannot confirm ownership for removal",
            ));
        }
        let service_id = *self.keyboard.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(id) = service_id {
            self.configure(keyboard::remove(id), false)
                .await
                .map_err(|e| {
                    Error::new("CLEANUP_FAILED", "HID keyboard removal failed")
                        .details(json!({"cause":e.code}))
                })?;
            let remaining = self.request(keyboard::list()).await.map_err(|e| {
                Error::new("CLEANUP_FAILED", "Cannot confirm HID keyboard removal")
                    .details(json!({"cause":e.code}))
            })?;
            if !keyboard::absent(&remaining, id) {
                return Err(Error::new(
                    "CLEANUP_FAILED",
                    "HID keyboard removal was not confirmed",
                ));
            }
            *self.keyboard.lock().unwrap_or_else(|e| e.into_inner()) = None;
            self.held_keys.store(false, Ordering::SeqCst);
        }
        Ok(())
    }
    /// `hardwareKeyboardAttached` stays true from the first key action until
    /// removal is confirmed: iOS hides the on-screen keyboard for that whole time.
    pub(crate) fn health(&self) -> Value {
        json!({"uncertain":self.uncertain.load(Ordering::SeqCst),"heldContact":self.held.lock().unwrap_or_else(|e|e.into_inner()).is_some(),"heldKeys":self.held_keys.load(Ordering::SeqCst),"hardwareKeyboardAttached":self.keyboard.lock().unwrap_or_else(|e|e.into_inner()).is_some(),"releaseError":self.release_error.lock().unwrap_or_else(|e|e.into_inner()).clone()})
    }
    async fn request(&self, payload: Dictionary) -> Result<Value> {
        self.configure(payload, true).await
    }
    async fn configure(&self, payload: Dictionary, reply: bool) -> Result<Value> {
        tokio::time::timeout(
            Duration::from_secs(5),
            self.reports.lock().await.request(payload, reply),
        )
        .await
        .map_err(|_| Error::new("OUTCOME_UNKNOWN", "HID configuration deadline expired"))?
    }
    async fn send_keys(&self, usages: &[u32]) -> Result<()> {
        let service_id = self
            .keyboard
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .ok_or_else(|| Error::new("SESSION_UNCERTAIN", "HID keyboard is absent"))?;
        // Track possibly held keys before a send, including uncertain delivery.
        if !usages.is_empty() {
            self.held_keys.store(true, Ordering::SeqCst);
        }
        // SAFETY: read-only monotonic host clock.
        let report = keyboard::report(usages, unsafe { mach_absolute_time() });
        tokio::time::timeout(
            Duration::from_secs(2),
            self.reports.lock().await.send(service_id, report),
        )
        .await
        .map_err(|_| Error::new("OUTCOME_UNKNOWN", "HID keyboard report deadline expired"))??;
        self.held_keys.store(!usages.is_empty(), Ordering::SeqCst);
        Ok(())
    }
    pub(crate) async fn input(self: &Arc<Self>, operation: &str, args: &Value) -> Result<Value> {
        validate(operation, args)?;
        if !matches!(operation, "key" | "type_text") {
            return self.gesture(operation, args).await;
        }
        let strokes = keyboard::strokes(operation, args)?;
        let _serial = self.serial.lock().await;
        if self.uncertain.load(Ordering::SeqCst) {
            return Err(Error::new(
                "SESSION_UNCERTAIN",
                "HID input is uncertain; close and reopen this controller",
            ));
        }
        let start = Instant::now();
        let mut release = Release {
            hid: self.clone(),
            finished: false,
        };
        let registering = self
            .keyboard
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none();
        let outcome = async {
            if registering {
                let id = u64::from(uuid::Uuid::new_v4().as_u128() as u32) | (1 << 32);
                // Retain the requested ID even if registration is interrupted.
                *self.keyboard.lock().unwrap_or_else(|e| e.into_inner()) = Some(id);
                self.keyboard_unconfirmed.store(true, Ordering::SeqCst);
                let reply = self.request(keyboard::create(id)).await?;
                let confirmed = reply["serviceID"]
                    .as_u64()
                    .filter(|id| *id != 0)
                    .ok_or_else(|| {
                        Error::new(
                            "PLATFORM_PROTOCOL",
                            "HID keyboard registration did not confirm its owned service ID",
                        )
                    })?;
                *self.keyboard.lock().unwrap_or_else(|e| e.into_inner()) = Some(confirmed);
                self.keyboard_unconfirmed.store(false, Ordering::SeqCst);
                tokio::time::sleep(keyboard::ATTACH_SETTLE).await;
                let pace = keyboard::pace(true, 0);
                self.send_keys(&[keyboard::PRIME]).await?;
                tokio::time::sleep(pace.hold).await;
                self.send_keys(&[]).await?;
                tokio::time::sleep(pace.gap).await;
            }
            for (index, stroke) in strokes.iter().enumerate() {
                let pace = keyboard::pace(registering, index);
                let mut usages = keyboard::modifiers(*stroke);
                if !usages.is_empty() {
                    self.send_keys(&usages).await?;
                    tokio::time::sleep(pace.hold).await;
                }
                usages.push(stroke.usage);
                self.send_keys(&usages).await?;
                tokio::time::sleep(pace.hold).await;
                self.send_keys(&[]).await?;
                tokio::time::sleep(pace.gap).await;
            }
            Ok::<(), Error>(())
        }
        .await;
        if let Err(error) = outcome {
            self.uncertain.store(true, Ordering::SeqCst);
            return Err(Error::new(
                "OUTCOME_UNKNOWN",
                "HID keyboard action may have reached the device; inspect and reacquire control",
            )
            .details(json!({"cause":error.code})));
        }
        release.finished = true;
        Ok(
            json!({"completed":true,"observed":false,"controlPath":"hid","completion":"reports_sent","deliveryAcknowledged":false,"focusVerified":false,"hardwareKeyboardAttached":true,"hardwareKeyboardJustAttached":registering,"timingsMs":{"dispatch":start.elapsed().as_secs_f64()*1000.0}}),
        )
    }
    pub(crate) async fn gesture(self: &Arc<Self>, operation: &str, args: &Value) -> Result<Value> {
        let (from, to, duration) = points(operation, args)?;
        let _serial = self.serial.lock().await;
        if self.uncertain.load(Ordering::SeqCst) {
            return Err(Error::new(
                "SESSION_UNCERTAIN",
                "HID input is uncertain; close and reopen this controller",
            ));
        }
        let start = Instant::now();
        let mut release = Release {
            hid: self.clone(),
            finished: false,
        };
        *self.held.lock().unwrap_or_else(|e| e.into_inner()) = Some(from);
        let outcome = async {
            self.send(true, from).await?;
            let contact_start = Instant::now();
            let steps = if from == to {
                1
            } else {
                duration.div_ceil(16).max(1)
            };
            for step in 1..=steps {
                let elapsed = Duration::from_millis(duration * step / steps);
                if duration > 0 {
                    tokio::time::sleep_until((contact_start + elapsed).into()).await;
                }
                if from != to {
                    let point = std::array::from_fn(|i| {
                        (f64::from(from[i])
                            + (f64::from(to[i]) - f64::from(from[i])) * step as f64 / steps as f64)
                            .round() as u16
                    });
                    *self.held.lock().unwrap_or_else(|e| e.into_inner()) = Some(point);
                    self.send(true, point).await?;
                }
            }
            self.release().await
        }
        .await;
        if let Err(error) = outcome {
            self.uncertain.store(true, Ordering::SeqCst);
            return Err(Error::new(
                "OUTCOME_UNKNOWN",
                "HID gesture may have reached the device; inspect and reacquire control",
            )
            .details(json!({"cause":error.code})));
        }
        release.finished = true;
        Ok(
            json!({"completed":true,"observed":false,"controlPath":"hid","completion":"reports_sent","deliveryAcknowledged":false,"contactMs":duration,"timingsMs":{"dispatch":start.elapsed().as_secs_f64()*1000.0}}),
        )
    }
}
struct Release {
    hid: Arc<Hid>,
    finished: bool,
}
impl Drop for Release {
    fn drop(&mut self) {
        if !self.finished {
            self.hid.uncertain.store(true, Ordering::SeqCst);
            let hid = self.hid.clone();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    let _serial = hid.serial.lock().await;
                    // release() retains failures for session health and close-time recovery.
                    let _ = hid.release().await;
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake(Arc<Mutex<Vec<Vec<u8>>>>);
    impl Reports for Fake {
        fn send(
            &mut self,
            _service_id: u64,
            report: Vec<u8>,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            Box::pin(async move {
                self.0.lock().unwrap().push(report);
                Ok(())
            })
        }
    }
    fn args() -> Value {
        json!({"point":{"x":0.5,"y":0.5},"scope":{"kind":"display"}})
    }
    #[tokio::test]
    async fn complete_tap_and_cancelled_press_release_the_contact() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let hid = Hid::new(Box::new(Fake(sent.clone())));
        assert_eq!(
            hid.gesture("tap", &args()).await.unwrap()["completion"],
            "reports_sent"
        );
        let reports = sent.lock().unwrap().clone();
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].len(), 58);
        assert_eq!(reports[0][3], 0xc2);
        assert_eq!(reports[1][3], 2);
        let mut input = args();
        input["durationMs"] = json!(1000);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), hid.gesture("long_press", &input))
                .await
                .is_err()
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(sent.lock().unwrap().last().unwrap()[3], 2);
        assert_eq!(
            hid.gesture("tap", &args()).await.unwrap_err().code,
            "SESSION_UNCERTAIN"
        );
    }
    #[test]
    fn scoped_input_and_geometry_are_not_silently_downgraded() {
        let mut input = args();
        input["scope"] = json!({"kind":"app","appId":"com.example.acme"});
        assert!(points("tap", &input).is_err());
        input = args();
        input["point"]["x"] = json!(1.1);
        assert!(points("tap", &input).is_err());
        assert_eq!(&report(false, [1, 65535], 0)[4..8], &[1, 0, 255, 255]);
    }
    struct Failing {
        sent: Arc<Mutex<Vec<Vec<u8>>>>,
        failed: bool,
    }
    impl Reports for Failing {
        fn send(
            &mut self,
            _service_id: u64,
            report: Vec<u8>,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            Box::pin(async move {
                let count = {
                    let mut sent = self.sent.lock().unwrap();
                    sent.push(report);
                    sent.len()
                };
                if count == 2 && !self.failed {
                    self.failed = true;
                    Err(Error::new(
                        "DEVICE_DISCONNECTED",
                        "Fixture interrupted report",
                    ))
                } else {
                    Ok(())
                }
            })
        }
    }
    #[tokio::test]
    async fn swipe_failure_releases_last_contact_and_quarantines_following_input() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let hid = Hid::new(Box::new(Failing {
            sent: sent.clone(),
            failed: false,
        }));
        let input = json!({"from":{"x":0.1,"y":0.2},"to":{"x":0.8,"y":0.9},"durationMs":50,"scope":{"kind":"display"}});
        assert_eq!(
            hid.gesture("swipe", &input).await.unwrap_err().code,
            "OUTCOME_UNKNOWN"
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
        {
            let reports = sent.lock().unwrap();
            assert_eq!(reports.last().unwrap()[3], 2);
            assert_eq!(&reports.last().unwrap()[4..8], &reports[1][4..8]);
        }
        assert_eq!(
            hid.gesture("tap", &args()).await.unwrap_err().code,
            "SESSION_UNCERTAIN"
        );
    }
    #[tokio::test]
    async fn zero_duration_sends_the_endpoint_before_release_without_a_hold() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let hid = Hid::new(Box::new(Fake(sent.clone())));
        let input = json!({"from":{"x":0,"y":0},"to":{"x":1,"y":1},"durationMs":0,"scope":{"kind":"display"}});
        let outcome = hid.gesture("swipe", &input).await.unwrap();
        assert_eq!(outcome["contactMs"], 0);
        {
            let reports = sent.lock().unwrap();
            assert_eq!(reports.len(), 3);
            assert_eq!(&reports[0][3..8], &[0xc2, 0, 0, 0, 0]);
            assert_eq!(&reports[1][3..8], &[0xc2, 255, 255, 255, 255]);
            assert_eq!(&reports[2][3..8], &[2, 255, 255, 255, 255]);
        }
        let mut press = args();
        press["durationMs"] = json!(0);
        hid.gesture("long_press", &press).await.unwrap();
        let reports = sent.lock().unwrap();
        assert_eq!(reports.len(), 5);
        assert_eq!(reports[3][3], 0xc2);
        assert_eq!(reports[4][3], 2);
        assert_eq!(&reports[3][4..8], &reports[4][4..8]);
        assert_eq!(hid.health()["heldContact"], false);
    }
    #[tokio::test]
    async fn swipe_reports_end_at_the_requested_point_before_release() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let hid = Hid::new(Box::new(Fake(sent.clone())));
        let input = json!({"from":{"x":0,"y":0},"to":{"x":1,"y":1},"durationMs":50,"scope":{"kind":"display"}});
        hid.gesture("swipe", &input).await.unwrap();
        let reports = sent.lock().unwrap();
        assert!(reports.len() > 3);
        assert_eq!(reports[0][3], 0xc2);
        assert_eq!(&reports.last().unwrap()[4..8], &[255, 255, 255, 255]);
        assert_eq!(reports.last().unwrap()[3], 2);
    }
}

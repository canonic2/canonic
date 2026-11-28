//! Dedicated Indigo consumer buttons; owns balanced release independently of keyboard/video.
use crate::{
    device_media::ConnectionOwner,
    error::{Error, Result},
};
use idevice::{
    ReadWrite, RemoteXpcClient,
    core_device::hid::{ButtonState, IndigoHidClient},
    rsd::RsdHandshake,
    tcp::handle::AdapterHandle,
};
use serde_json::{Value, json};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::Mutex as AsyncMutex;

type Transport = (AdapterHandle, u16);
trait Sender: Send {
    fn send(
        &mut self,
        usage: u64,
        down: bool,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>>;
}
struct Indigo(IndigoHidClient<Box<dyn ReadWrite>>);
impl Sender for Indigo {
    fn send(
        &mut self,
        usage: u64,
        down: bool,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            self.0
                .send_button(
                    0x0c,
                    usage,
                    if down {
                        ButtonState::Down
                    } else {
                        ButtonState::Up
                    },
                )
                .await
                .map_err(crate::device_media::platform)
        })
    }
}
struct State {
    client: Option<Box<dyn Sender>>,
    owned_tunnel: Option<AdapterHandle>,
}
pub(super) struct Buttons {
    serial: String,
    shared: Option<Arc<crate::device_media::Source>>,
    state: AsyncMutex<State>,
    held: Mutex<Option<u64>>,
    uncertain: AtomicBool,
    closed: AtomicBool,
}
impl Buttons {
    pub(super) fn new(serial: &str, shared: Option<Arc<crate::device_media::Source>>) -> Arc<Self> {
        Arc::new(Self {
            serial: serial.into(),
            shared,
            state: AsyncMutex::new(State {
                client: None,
                owned_tunnel: None,
            }),
            held: Mutex::new(None),
            uncertain: AtomicBool::new(false),
            closed: AtomicBool::new(false),
        })
    }
    pub(super) async fn ready(&self) -> Result<()> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(Error::new("SESSION_CLOSING", "Media buttons are closing"));
        }
        let mut state = self.state.lock().await;
        if self.uncertain.load(Ordering::SeqCst) {
            return Err(Error::new(
                "MEDIA_OUTCOME_UNKNOWN",
                "A media button release is uncertain; disconnect before further controls",
            ));
        }
        if state.client.is_some() {
            return Ok(());
        }
        let transport: Transport = if let Some(source) = &self.shared {
            source.consumer_transport()?
        } else {
            if let Some(tunnel) = &mut state.owned_tunnel {
                tunnel
                    .close()
                    .await
                    .map_err(|e| crate::device_media::platform(e.into()))?;
            }
            state.owned_tunnel.take();
            let (tunnel, port) = ConnectionOwner::usb_tunnel(&self.serial).await?;
            state.owned_tunnel = Some(tunnel.clone());
            let rsd = RsdHandshake::new(
                tunnel
                    .clone()
                    .connect(port)
                    .await
                    .map_err(|e| crate::device_media::platform(e.into()))?,
            )
            .await
            .map_err(crate::device_media::platform)?;
            if rsd
                .properties
                .get("UniqueDeviceID")
                .and_then(plist::Value::as_string)
                != Some(&self.serial)
            {
                return Err(Error::new("STALE_TARGET", "Indigo target identity changed"));
            }
            let port = rsd
                .services
                .get("com.apple.coredevice.hid.indigo")
                .ok_or_else(|| {
                    Error::new(
                        "DEVELOPER_SUPPORT_REQUIRED",
                        "Developer support must expose CoreDevice Indigo",
                    )
                })?
                .port;
            (tunnel, port)
        };
        let (mut tunnel, port) = transport;
        let stream = tunnel
            .connect(port)
            .await
            .map_err(|e| crate::device_media::platform(e.into()))?;
        let mut xpc = RemoteXpcClient::new(Box::new(stream) as Box<dyn ReadWrite>)
            .await
            .map_err(crate::device_media::platform)?;
        xpc.do_handshake()
            .await
            .map_err(crate::device_media::platform)?;
        state.client = Some(Box::new(Indigo(IndigoHidClient::new(xpc))));
        Ok(())
    }
    async fn release(&self, state: &mut State) -> Result<()> {
        let usage = *self.held.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(usage) = usage {
            let client = state.client.as_mut().ok_or_else(|| {
                Error::new(
                    "MEDIA_OUTCOME_UNKNOWN",
                    "Indigo client was lost during release",
                )
            })?;
            tokio::time::timeout(Duration::from_secs(2), client.send(usage, false))
                .await
                .map_err(|_| Error::new("MEDIA_OUTCOME_UNKNOWN", "Indigo release timed out"))??;
            *self.held.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
        Ok(())
    }
    pub(super) async fn press(self: &Arc<Self>, command: &str) -> Result<Value> {
        let usage = usage(command)?;
        self.ready().await?;
        let mut state = self.state.lock().await;
        if self.closed.load(Ordering::SeqCst) || self.uncertain.load(Ordering::SeqCst) {
            return Err(Error::new(
                "MEDIA_OUTCOME_UNKNOWN",
                "Media input is closing or uncertain",
            ));
        }
        let mut guard = Release {
            buttons: self.clone(),
            finished: false,
        };
        *self.held.lock().unwrap_or_else(|e| e.into_inner()) = Some(usage);
        let result = async {
            state.client.as_mut().unwrap().send(usage, true).await?;
            tokio::time::sleep(Duration::from_millis(20)).await;
            self.release(&mut state).await
        }
        .await;
        if let Err(error) = result {
            self.uncertain.store(true, Ordering::SeqCst);
            return Err(Error::new(
                "MEDIA_OUTCOME_UNKNOWN",
                "Media button may have reached iOS; inspect status before further control",
            )
            .details(json!({"cause":error,"command":command})));
        }
        guard.finished = true;
        Ok(
            json!({"source":"coredevice_indigo","dispatch":{"state":"sent","command":command,"destination":{"kind":"system_player"},"completion":"reports_sent","deliveryAcknowledged":false,"holdMs":20},"recipientIdentity":null,"unavailableFields":["recipientIdentity"],"effect":{"state":"not_checked"},"observed":false,"audibility":"unverified"}),
        )
    }
    pub(super) async fn close(&self) -> Result<()> {
        self.closed.store(true, Ordering::SeqCst);
        let mut state = self.state.lock().await;
        self.release(&mut state).await?;
        state.client.take();
        if let Some(tunnel) = &mut state.owned_tunnel {
            tunnel
                .close()
                .await
                .map_err(|e| crate::device_media::platform(e.into()))?;
        }
        state.owned_tunnel.take();
        Ok(())
    }
}
fn usage(command: &str) -> Result<u64> {
    match command {
        "play" => Ok(0xb0),
        "pause" => Ok(0xb1),
        "play_pause" => Ok(0xcd),
        "next" => Ok(0xb5),
        "previous" => Ok(0xb6),
        "volume_up" => Ok(0xe9),
        "volume_down" => Ok(0xea),
        _ => Err(Error::invalid("Unknown media command")),
    }
}
struct Release {
    buttons: Arc<Buttons>,
    finished: bool,
}
impl Drop for Buttons {
    fn drop(&mut self) {
        let state = self.state.get_mut();
        let client = state.client.take();
        let tunnel = state.owned_tunnel.take();
        let held = *self.held.get_mut().unwrap_or_else(|e| e.into_inner());
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                if let (Some(mut client), Some(usage)) = (client, held) {
                    let _ = tokio::time::timeout(Duration::from_secs(2), client.send(usage, false))
                        .await;
                }
                if let Some(mut tunnel) = tunnel {
                    let _ = tunnel.close().await;
                }
            });
        }
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        if !self.finished {
            self.buttons.uncertain.store(true, Ordering::SeqCst);
            let buttons = self.buttons.clone();
            tokio::spawn(async move {
                let mut state = buttons.state.lock().await;
                let _ = buttons.release(&mut state).await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    type Events = Arc<Mutex<Vec<(u64, bool)>>>;
    struct Fixture {
        events: Events,
        fail: Option<usize>,
        block_down: bool,
        released: Option<tokio::sync::oneshot::Sender<()>>,
    }
    impl Sender for Fixture {
        fn send(
            &mut self,
            usage: u64,
            down: bool,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            Box::pin(async move {
                let count = {
                    let mut events = self.events.lock().unwrap();
                    events.push((usage, down));
                    events.len()
                };
                if self.fail == Some(count) {
                    return Err(Error::new("DEVICE_DISCONNECTED", "Injected Indigo failure"));
                }
                if down && self.block_down {
                    std::future::pending::<()>().await;
                }
                if !down && let Some(released) = self.released.take() {
                    let _ = released.send(());
                }
                Ok(())
            })
        }
    }
    async fn fixture(
        fail: Option<usize>,
        block_down: bool,
    ) -> (Arc<Buttons>, Events, tokio::sync::oneshot::Receiver<()>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let (released, release) = tokio::sync::oneshot::channel();
        let buttons = Buttons::new("Acme", None);
        buttons.state.lock().await.client = Some(Box::new(Fixture {
            events: events.clone(),
            fail,
            block_down,
            released: Some(released),
        }));
        (buttons, events, release)
    }
    #[tokio::test]
    async fn every_consumer_command_has_one_balanced_pair_and_no_effect_claim() {
        let (buttons, events, _) = fixture(None, false).await;
        for (command, code) in [
            ("play", 0xb0),
            ("pause", 0xb1),
            ("play_pause", 0xcd),
            ("next", 0xb5),
            ("previous", 0xb6),
            ("volume_up", 0xe9),
            ("volume_down", 0xea),
        ] {
            let result = buttons.press(command).await.unwrap();
            assert_eq!(result["dispatch"]["state"], "sent");
            assert_eq!(result["observed"], false);
            assert_eq!(result["recipientIdentity"], Value::Null);
            {
                let events = events.lock().unwrap();
                assert_eq!(&events[events.len() - 2..], &[(code, true), (code, false)]);
            }
        }
        buttons.close().await.unwrap();
        assert!(buttons.press("play").await.is_err());
    }
    #[tokio::test]
    async fn cancellation_and_send_failures_release_and_quarantine_without_repress() {
        for fail in [None, Some(1), Some(2)] {
            let (buttons, events, released) = fixture(fail, fail.is_none()).await;
            if fail.is_none() {
                let mut press = Box::pin(buttons.press("next"));
                tokio::select! {
                    biased;
                    result = &mut press => panic!("Blocked press completed before cancellation: {result:?}"),
                    () = std::future::ready(()) => {}
                }
                assert_eq!(*events.lock().unwrap(), [(0xb5, true)]);
                drop(press);
            } else {
                assert_eq!(
                    buttons.press("next").await.unwrap_err().code,
                    "MEDIA_OUTCOME_UNKNOWN"
                );
            }
            tokio::time::timeout(Duration::from_secs(2), released)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(events.lock().unwrap().last(), Some(&(0xb5, false)));
            assert!(buttons.press("next").await.is_err());
            assert_eq!(
                events
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(_, down)| *down)
                    .count(),
                1
            );
            buttons.close().await.unwrap();
        }
    }
}

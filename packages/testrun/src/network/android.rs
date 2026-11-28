//! adb connectivity adapter: one fixed readback script, bounded switch commands
//! and the polling that turns commands into an agreed or unsettled outcome.
use super::{Command, Readback, Request};
use crate::{
    android::Android,
    error::{Error, Result},
};
use std::time::{Duration, Instant};

/// Fixed shell script reading every connectivity switch in one adb round trip.
/// Markers separate sections; the parser ignores sections a target lacks.
pub(crate) const READBACK: &str = "echo '#airplane'; cmd connectivity airplane-mode 2>/dev/null; \
echo '#airplane_on'; settings get global airplane_mode_on 2>/dev/null; \
echo '#wifi_on'; settings get global wifi_on 2>/dev/null; \
echo '#mobile_data'; settings get global mobile_data 2>/dev/null; \
echo '#bluetooth_on'; settings get global bluetooth_on 2>/dev/null; \
echo '#wifi'; cmd wifi status 2>/dev/null | grep -m 2 -E '^Wifi is (enabled|disabled|connected to|not connected)'; \
echo '#bluetooth'; dumpsys bluetooth_manager 2>/dev/null | grep -m 1 -E '^ *state:'; \
echo '#default'; dumpsys connectivity 2>/dev/null | grep -m 1 'Active default network'; \
echo '#end'";

const POLL: Duration = Duration::from_millis(500);

impl Android {
    /// Read the target's connectivity switches once.
    pub(crate) async fn network_readback(&self, serial: &str) -> Result<Readback> {
        let output = self
            .script(serial, READBACK, Duration::from_secs(15), 65536)
            .await
            .map_err(|error| {
                Error::new(
                    "NETWORK_READBACK_FAILED",
                    "Cannot read the target's connectivity state",
                )
                .details(serde_json::json!({"cause":error}))
            })?;
        let text = String::from_utf8_lossy(&output);
        if !text.contains("#end") {
            return Err(Error::new(
                "NETWORK_READBACK_FAILED",
                "The connectivity readback ended before its final marker",
            ));
        }
        Ok(Readback::parse(&text))
    }
    /// The target's API level, when its property store answers.
    pub(crate) async fn api_level(&self, serial: &str) -> Option<u32> {
        let output = self
            .shell(
                serial,
                &["getprop", "ro.build.version.sdk"],
                Duration::from_secs(5),
                4096,
            )
            .await
            .ok()?;
        String::from_utf8_lossy(&output).trim().parse().ok()
    }
    async fn switches(&self, serial: &str, commands: &[Command]) -> Result<()> {
        for command in commands {
            self.shell(serial, command, Duration::from_secs(10), 65536)
                .await
                .map_err(|error| {
                    Error::new(
                        "NETWORK_COMMAND_FAILED",
                        format!("adb refused `{}`", command.join(" ")),
                    )
                    .details(serde_json::json!({"cause":error}))
                })?;
        }
        Ok(())
    }
    /// Poll the readback until `agree` accepts it or `budget` elapses. Returns
    /// the last readback and whether it agreed.
    async fn settle(
        &self,
        serial: &str,
        budget: Duration,
        agree: impl Fn(&Readback) -> bool,
    ) -> Result<(Readback, bool, u64)> {
        let start = Instant::now();
        loop {
            let readback = self.network_readback(serial).await?;
            let elapsed = start.elapsed();
            if agree(&readback) {
                return Ok((readback, true, elapsed.as_millis() as u64));
            }
            if elapsed >= budget {
                return Ok((readback, false, elapsed.as_millis() as u64));
            }
            tokio::time::sleep(POLL.min(budget - elapsed)).await;
        }
    }
}

/// One mutation's observed result.
pub(crate) struct Outcome {
    pub(crate) before: Readback,
    pub(crate) after: Readback,
    pub(crate) changed: bool,
    pub(crate) agreed: bool,
    pub(crate) settle_ms: u64,
}

/// Send planned switch commands, then poll until the readback agrees with
/// `request` or the budget ends. An empty plan still polls when connectivity
/// has not agreed.
pub(crate) async fn dispatch(
    adapter: &Android,
    serial: &str,
    commands: &[Command],
    request: Request,
) -> Result<(Readback, bool, u64)> {
    adapter.switches(serial, commands).await?;
    adapter
        .settle(
            serial,
            Duration::from_millis(request.timeout_ms),
            |readback| readback.agrees(request),
        )
        .await
}

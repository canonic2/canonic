//! Clap argument models for public commands and owned native workers.
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Clone, Default, Args)]
pub struct PageArgs {
    /// Continue the same query using its opaque cursor.
    #[arg(long, value_name = "CURSOR")]
    pub cursor: Option<String>,
    /// Maximum units; ordinary lists default to 50 and permit 1 to 100.
    #[arg(long, value_name = "N")]
    pub limit: Option<u64>,
}
impl PageArgs {
    pub(crate) fn arguments(&self) -> serde_json::Value {
        canonic_mcp_kit::page::arguments(self.cursor.as_deref(), self.limit)
    }
    pub(crate) fn add_to(&self, value: &mut serde_json::Value) {
        value.as_object_mut().expect("arguments").extend(
            self.arguments()
                .as_object()
                .expect("page arguments")
                .clone(),
        );
    }
}

/// One device command and the target it names, if any.
pub struct Mobile {
    pub target: Option<String>,
    pub command: Action,
}

#[derive(Args)]
pub struct MediaFixtureArgs {
    /// Chrome only. Play a host WAV as microphone input.
    #[arg(long, value_name = "WAV", requires = "engine")]
    microphone_fixture: Option<String>,
    /// Chrome only. Stream a host Y4M or MJPEG file as camera input.
    #[arg(long, value_name = "VIDEO", requires = "engine")]
    camera_fixture: Option<String>,
}

#[derive(Args)]
pub struct BrowserConnectionArgs {
    /// Select installed Chrome Stable or Edge Stable with --engine chrome.
    #[arg(long, value_parser = ["chrome", "msedge"], requires = "engine")]
    channel: Option<String>,
    /// Use or create a named retained browser profile.
    #[arg(long, requires = "engine", conflicts_with_all = ["borrowed_profile", "disposable_profile"])]
    profile: Option<String>,
    /// Use a closed, compatible caller-owned browser profile directory.
    #[arg(long, requires = "engine", conflicts_with = "profile")]
    borrowed_profile: Option<String>,
    /// Use the regular context of a disposable Chrome for Testing profile.
    #[arg(long, requires = "engine", conflicts_with_all = ["profile", "borrowed_profile", "attach", "channel"])]
    disposable_profile: bool,
    /// Attach to an explicit loopback ws or TLS-verified wss browser endpoint.
    #[arg(long, requires = "engine")]
    attach: Option<String>,
    /// Approved private JSON credential containing the endpoint origin and token.
    #[arg(long, requires = "attach")]
    attachment_credential: Option<String>,
    /// Private PEM CA for this endpoint's TLS connection.
    #[arg(long, requires = "attach")]
    attachment_ca: Option<String>,
    /// Adopt this exact isolated Chrome CDP context.
    #[arg(long, requires = "attach")]
    adopt_context: Option<String>,
}

impl BrowserConnectionArgs {
    pub(crate) fn add_to(&self, args: &mut serde_json::Value) {
        use serde_json::json;
        if let Some(channel) = &self.channel {
            args["channel"] = json!(channel);
        }
        if let Some(name) = &self.profile {
            args["profile"] = json!({"kind":"retained","name":name});
        } else if let Some(path) = &self.borrowed_profile {
            args["profile"] = json!({"kind":"borrowed","path":path});
        } else if self.disposable_profile {
            args["profile"] = json!({"kind":"disposable"});
        }
        let Some(endpoint) = &self.attach else { return };
        let mut attachment = json!({"endpoint":endpoint});
        if let Some(path) = &self.attachment_credential {
            attachment["credentialPath"] = json!(path);
        }
        if let Some(path) = &self.attachment_ca {
            attachment["caPath"] = json!(path);
        }
        if let Some(id) = &self.adopt_context {
            attachment["contextId"] = json!(id);
        }
        args["attachment"] = attachment;
    }
}

impl MediaFixtureArgs {
    pub(crate) fn add_to(&self, args: &mut serde_json::Value) {
        let mut fixtures = serde_json::Map::new();
        for (name, path) in [
            ("microphonePath", &self.microphone_fixture),
            ("cameraPath", &self.camera_fixture),
        ] {
            if let Some(path) = path {
                fixtures.insert(name.into(), serde_json::json!(path));
            }
        }
        if !fixtures.is_empty() {
            args["mediaFixtures"] = fixtures.into();
        }
    }
}

/// Device and browser commands. Each command's words, joined with `_`, name
/// the MCP tool it runs.
#[derive(Subcommand)]
pub enum Action {
    /// Check whether the --target iPhone or Android device is ready, without connecting.
    Doctor,
    /// Connect a device with --target or a browser with --engine.
    Connect {
        /// Launch or attach to a browser with the selected engine.
        #[arg(long, value_parser = ["chrome", "firefox", "webkit"])]
        engine: Option<String>,
        #[arg(long, value_parser = ["headless", "headed"], requires = "engine")]
        mode: Option<String>,
        #[command(flatten)]
        browser_connection: Box<BrowserConnectionArgs>,
        /// Browser context options as JSON; defaults to a 1280x720 viewport.
        #[arg(long, requires = "engine")]
        context: Option<String>,
        /// Browser proxy defaults as JSON; individual context proxy options override them.
        #[arg(long, requires = "engine")]
        proxy: Option<String>,
        #[command(flatten)]
        media_fixtures: MediaFixtureArgs,
        /// Experimental Android capture backend; screenshot is the default.
        #[arg(long, value_parser = ["screenshot", "stream"])]
        android_capture: Option<String>,
        /// Start a shut-down simulator or an android:avd: emulator; disconnect stops it.
        #[arg(long)]
        boot: bool,
        /// Show the window of an emulator started with --boot.
        #[arg(long)]
        window: bool,
        /// Physical iPhone input path: hid (default) or xctest.
        #[arg(long, value_parser = ["hid", "xctest"])]
        control_path: Option<String>,
        /// App inspected over xctest on a physical iPhone.
        #[arg(long)]
        app_id: Option<String>,
        /// Prepare the signed XCTest runner on a physical iPhone.
        #[arg(long)]
        setup: bool,
        /// Install developer support first if the iPhone needs it.
        #[arg(long)]
        prepare_support: bool,
        /// Apple signing team for this connection only; does not change the host default.
        #[arg(long)]
        runner_team_id: Option<String>,
    },
    /// Release the --target device, or the only connected one, for every client.
    Disconnect,
    /// Show connected devices and their last action, or one action with --action-id.
    Status {
        #[command(flatten)]
        page: PageArgs,
        #[arg(long)]
        action_id: Option<String>,
        /// Wait up to 30000 ms for browser preparation or a pending action.
        #[arg(long, value_parser = clap::value_parser!(u64).range(0..=30000))]
        wait_ms: Option<u64>,
        /// Include browser resources and the full operation-summary list.
        #[arg(long)]
        include_details: bool,
        #[arg(long, hide = true)]
        endpoint: Option<PathBuf>,
        #[arg(long, hide = true, requires = "endpoint")]
        command_id: Option<String>,
    },
    /// Screenshot a device or submit a browser screenshot with --target and --page.
    Observe {
        /// Browser page reference as JSON; requires a browser --target.
        #[arg(long)]
        page: Option<String>,
        #[arg(long, value_parser = ["fast", "evidence"])]
        profile: Option<String>,
    },
    /// Perform one JSON action, or up to 8 in order, on the connected device or the --target.
    ///
    /// A browser --target submits typed actions asynchronously without implicit
    /// screenshots.
    ///
    /// On a physical iPhone over HID, the first type_text or key action attaches a
    /// hardware keyboard, and iOS hides the on-screen keyboard until disconnect.
    /// Screens with a focused field then show no keyboard, so keyboard layout,
    /// keyboard types, suggestions and dismissal go untested. To test with the
    /// on-screen keyboard visible, disconnect and connect again with
    /// --control-path xctest; when keyboard behavior matters, cover both.
    Act {
        actions: String,
        /// Browser batch identity for deduplication and status.
        #[arg(long)]
        action_id: Option<String>,
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..=900000))]
        timeout_ms: Option<u64>,
        /// Return only the final screenshot.
        #[arg(long)]
        final_only: bool,
        #[arg(long, value_parser = ["fast", "evidence"])]
        profile: Option<String>,
    },
    /// Request cancellation of a browser action without undoing dispatched work.
    Cancel {
        #[arg(long)]
        action_id: String,
    },
    /// Wait until any of 1..8 JSON conditions holds, without sending input.
    Wait {
        conditions: String,
        #[arg(long, default_value = "10000", value_parser = clap::value_parser!(u64).range(0..=30000))]
        timeout_ms: u64,
        #[arg(long)]
        allow_partial: bool,
    },
    /// List iOS simulators, physical iPhones and Android devices and emulators.
    #[command(subcommand)]
    Target(TargetCommand),
    /// Show the Developer Mode setting on a physical iPhone.
    #[command(subcommand)]
    DeveloperMode(DeveloperModeCommand),
    /// Install the developer support a physical iPhone needs for testing.
    #[command(subcommand)]
    DeveloperSupport(DeveloperSupportCommand),
    /// Read the accessibility tree of the device's screen.
    #[command(subcommand)]
    Ui(UiCommand),
    /// List, install or uninstall apps on the device.
    #[command(subcommand)]
    App(AppCommand),
    /// Show or change whether the agent or a local viewer controls input.
    #[command(subcommand)]
    Control(ControlCommand),
    /// Measure the device's audio output.
    #[command(subcommand)]
    Audio(AudioCommand),
    /// Show media sessions or send media commands.
    #[command(subcommand)]
    Media(MediaCommand),
    /// Show or change the Android media volume.
    #[command(subcommand)]
    Volume(VolumeCommand),
    /// Change the Android device's network.
    #[command(subcommand)]
    Network(NetworkCommand),
    /// Read the iOS simulator's clipboard.
    #[command(subcommand)]
    Clipboard(ClipboardCommand),
    /// Start, inspect or stop a live video preview of an iOS device.
    #[command(subcommand)]
    Preview(PreviewCommand),
    /// Start, inspect or stop a screen recording.
    #[command(subcommand)]
    Recording(RecordingCommand),
    /// Prepare, connect and control isolated Chrome, Firefox and WebKit browsers.
    #[command(subcommand)]
    Browser(BrowserCommand),
    /// The user's manager for one data root; clients start it.
    #[command(hide = true)]
    ManagedOwner {
        #[arg(long)]
        root: PathBuf,
    },
    /// Advanced: retain a foreground controller for explicit socket clients.
    #[command(hide = true)]
    Session {
        #[arg(long, value_parser = ["screenshot", "stream"])]
        android_capture: Option<String>,
        #[arg(long)]
        boot: bool,
        #[arg(long)]
        window: bool,
        #[arg(long)]
        runner_team_id: Option<String>,
        #[arg(long, value_parser = ["hid", "xctest"])]
        control_path: Option<String>,
        #[arg(long)]
        app_id: Option<String>,
        #[arg(long)]
        setup: bool,
        #[arg(long)]
        directory: Option<PathBuf>,
        #[arg(long, default_value = "300000", value_parser = clap::value_parser!(u64).range(1000..=600000))]
        idle_timeout_ms: u64,
        #[arg(long, value_parser = ["evidence", "fast"])]
        observation_profile: Option<String>,
    },
    /// Advanced: execute steps using an explicit retained socket.
    #[command(hide = true)]
    Batch {
        #[arg(long)]
        endpoint: PathBuf,
        #[arg(long)]
        command_id: Option<String>,
        #[arg(long)]
        no_observe: bool,
        #[arg(long, conflicts_with = "no_observe")]
        observe_after_each: bool,
        #[arg(long, value_parser = ["evidence", "fast"])]
        observation_profile: Option<String>,
        steps: String,
    },
    /// Advanced: finalize an explicit retained socket.
    #[command(hide = true)]
    Close {
        #[arg(long)]
        endpoint: PathBuf,
    },
}

#[derive(Subcommand)]
pub enum TargetCommand {
    /// List iOS simulators, physical iPhones and Android devices and emulators.
    List {
        #[command(flatten)]
        page: PageArgs,
        #[arg(long)]
        state: Option<String>,
        #[arg(long, value_parser = ["simulator", "device", "android"])]
        kind: Option<String>,
        /// Also check each physical device's readiness.
        #[arg(long)]
        probe: bool,
        /// Print only identity, state and readiness.
        #[arg(long)]
        compact: bool,
    },
}

#[derive(Subcommand)]
pub enum DeveloperModeCommand {
    /// Show the Developer Mode setting on the --target iPhone; the user turns it on and confirms the restart.
    Reveal,
}

#[derive(Subcommand)]
pub enum DeveloperSupportCommand {
    /// Download and install developer support on the unlocked --target iPhone.
    Install,
}

#[derive(Subcommand)]
pub enum UiCommand {
    /// Print the screen's accessibility tree.
    Snapshot {
        #[command(flatten)]
        page: PageArgs,
        #[command(flatten)]
        scope: Scope,
        #[arg(long)]
        allow_partial: bool,
    },
    /// Find on-screen elements by identifier, role or label substring.
    Find {
        #[arg(long)]
        identifier: Option<String>,
        #[arg(long)]
        role: Option<String>,
        #[arg(long)]
        label: Option<String>,
        #[command(flatten)]
        page: PageArgs,
        #[command(flatten)]
        scope: Scope,
        #[arg(long)]
        allow_partial: bool,
    },
    /// Read one element from a JSON {query} or {snapshotId, elementId}.
    Read {
        element: String,
        #[arg(long)]
        allow_partial: bool,
    },
}

#[derive(Subcommand)]
pub enum AppCommand {
    /// List the apps on the device that can be opened, with their home-screen names.
    List {
        #[command(flatten)]
        page: PageArgs,
        /// Also list apps that cannot be opened, such as Android system packages.
        #[arg(long)]
        all: bool,
        /// Show only apps whose name or app ID contains this text, ignoring case.
        #[arg(long)]
        name: Option<String>,
    },
    /// Install a local .app or .apk; --replace updates an installed app without uninstalling it.
    Install {
        #[arg(
            required_unless_present = "artifact_ref",
            conflicts_with = "artifact_ref"
        )]
        path: Option<PathBuf>,
        /// Retained artifact from a build in this workspace.
        #[arg(long)]
        artifact_ref: Option<String>,
        #[arg(long)]
        replace: bool,
        /// Android: answer a Google Play Protect security-check prompt shown during the install.
        #[arg(long, value_parser = ["fail", "dont_send", "send"], default_value = "fail")]
        play_protect: String,
    },
    /// Uninstall an app and its data.
    Uninstall { app_id: String },
}

#[derive(Subcommand)]
pub enum ControlCommand {
    /// Show whether the agent or a local viewer controls input.
    Status,
    /// Give input control to the agent or a local viewer.
    Acquire {
        /// Browser viewer's exact page reference as JSON; omit for agent control.
        #[arg(long)]
        page: Option<String>,
        #[arg(value_parser = ["agent", "viewer"])]
        owner: String,
        #[arg(long)]
        viewer_id: Option<String>,
        #[arg(long)]
        expected_version: u64,
    },
    /// Release input control while screenshots and recordings continue.
    Release {
        #[arg(long)]
        expected_version: u64,
    },
}

#[derive(Subcommand)]
pub enum AudioCommand {
    /// Measure the device's audio output; Android needs --accept-route-change.
    Probe {
        #[arg(long, default_value = "2000", value_parser = clap::value_parser!(u64).range(100..=10000))]
        duration_ms: u64,
        #[arg(long)]
        retain_clip: bool,
        #[arg(long)]
        accept_route_change: bool,
    },
}

#[derive(Subcommand)]
pub enum MediaCommand {
    /// Show the Android device's media sessions and playback state.
    Status,
    /// Send one media command to an Android --media-session or the iOS --system-player.
    Send {
        #[arg(value_parser = ["play", "pause", "play_pause", "next", "previous", "stop", "volume_up", "volume_down"])]
        command: String,
        #[arg(
            long,
            conflicts_with = "system_player",
            required_unless_present = "system_player"
        )]
        media_session: Option<String>,
        #[arg(long)]
        system_player: bool,
        #[arg(long)]
        observe_for_ms: Option<u64>,
    },
}

#[derive(Subcommand)]
pub enum VolumeCommand {
    /// Show the Android music-stream volume, or a --media-session's.
    Get {
        #[arg(long)]
        media_session: Option<String>,
    },
    /// Set the Android music-stream volume, or a --media-session's.
    Set {
        #[arg(long)]
        media_session: Option<String>,
        #[arg(long, conflicts_with = "adjust", required_unless_present = "adjust")]
        value: Option<u32>,
        #[arg(long, value_parser = ["up", "down"])]
        adjust: Option<String>,
        #[arg(long)]
        observe_for_ms: Option<u64>,
    },
}

#[derive(Subcommand)]
pub enum NetworkCommand {
    /// Turn the Android device's network off or on; it stays that way until you change it.
    Set {
        #[arg(value_parser = ["offline", "online"])]
        mode: String,
        /// airplane (default; also turns Bluetooth off) or radios (Wi-Fi and mobile data).
        #[arg(long, value_parser = ["airplane", "radios"])]
        method: Option<String>,
        /// Milliseconds to wait for the switches to read back (1000..60000).
        #[arg(long, value_parser = clap::value_parser!(u64).range(1000..=60000))]
        timeout_ms: Option<u64>,
    },
}

#[derive(Subcommand)]
pub enum ClipboardCommand {
    /// Print the text on the connected iOS simulator's clipboard.
    Read,
}

#[derive(Subcommand)]
pub enum PreviewCommand {
    /// Start a live video preview of the connected iOS device and print its ID.
    Start {
        #[arg(long, value_parser = ["h264", "jpeg", "hevc"])]
        format: Option<String>,
    },
    /// Show a live preview's health, size and frame age.
    Status { preview_id: String },
    /// Stop a live preview.
    Stop { preview_id: String },
}

#[derive(Subcommand)]
pub enum RecordingCommand {
    /// Start recording the connected device's screen to an MP4 and print its ID.
    Start {
        #[arg(long, value_parser = clap::value_parser!(u64).range(100..=600000))]
        max_duration_ms: Option<u64>,
    },
    /// Show a recording's progress or its finished file.
    Status { recording_id: String },
    /// Stop a recording and save the MP4.
    Stop { recording_id: String },
}

#[derive(Subcommand)]
pub enum BrowserCommand {
    /// List private Chrome, Firefox and WebKit availability without launching.
    List(PageArgs),
    /// Check browser host support and private dependencies.
    Doctor {
        #[arg(long,value_parser=["chrome","firefox","webkit"])]
        engine: String,
    },
    /// Launch a browser or attach to an explicit browser endpoint.
    Connect {
        #[arg(long,value_parser=["chrome","firefox","webkit"])]
        engine: String,
        #[arg(long,value_parser=["headless","headed"],default_value="headless")]
        mode: String,
        #[command(flatten)]
        browser_connection: Box<BrowserConnectionArgs>,
        /// Browser proxy defaults as JSON.
        #[arg(long)]
        proxy: Option<String>,
        /// Allow downloads into the browser's private incoming directory.
        #[arg(long)]
        downloads: bool,
        /// Enable touchscreen emulation for the initial context.
        #[arg(long)]
        touch: bool,
        #[command(flatten)]
        media_fixtures: MediaFixtureArgs,
    },
    /// Snapshot a cleared local fixture and return its registered browser reference.
    FixtureRegister {
        path: PathBuf,
        #[arg(long)]
        mime_type: String,
        #[arg(long,value_parser=["synthetic_generated","blank_template","seed_asset"])]
        provenance: String,
        #[arg(long)]
        cleared_by: String,
        #[arg(long)]
        clearance_method: String,
    },
    /// Show preparation, browser resources or one lifecycle operation.
    Status {
        #[command(flatten)]
        page: PageArgs,
        #[arg(long)]
        operation_id: Option<String>,
        /// Wait up to 30000 ms for browser preparation or a pending operation.
        #[arg(long,default_value="0",value_parser=clap::value_parser!(u64).range(0..=30000))]
        wait_ms: u64,
        /// Include browser resources and the full operation-summary list.
        #[arg(long)]
        include_details: bool,
    },
    /// Send one JSON resource, page, environment or diagnostic request and print its operation ID.
    Send {
        request: String,
        #[command(flatten)]
        page: PageArgs,
        #[arg(long)]
        operation_id: Option<String>,
        #[arg(long,default_value="30000",value_parser=clap::value_parser!(u64).range(1..=900000))]
        timeout_ms: u64,
    },
    /// Request cancellation without replaying dispatched browser work.
    Cancel {
        #[arg(long)]
        operation_id: String,
    },
    /// Close one browser while keeping the other connected targets.
    Disconnect,
}

/// The part of the screen an accessibility read covers: the whole display by
/// default, or one app or the system UI, which physical iPhones over XCTest need.
#[derive(Args)]
pub struct Scope {
    /// Read only this app.
    #[arg(long, conflicts_with = "system")]
    pub app_id: Option<String>,
    /// Read only the system UI.
    #[arg(long)]
    pub system: bool,
}
impl Scope {
    pub(crate) fn value(&self) -> serde_json::Value {
        match (&self.app_id, self.system) {
            (Some(app), _) => serde_json::json!({"kind":"app","appId":app}),
            (None, true) => serde_json::json!({"kind":"system"}),
            (None, false) => serde_json::json!({"kind":"display"}),
        }
    }
}
/// A `ui_find` query from an identifier, a role and a label substring.
pub(crate) fn find_query(
    identifier: Option<String>,
    role: Option<String>,
    label: Option<String>,
) -> serde_json::Value {
    let mut query = serde_json::json!({});
    if let Some(identifier) = identifier {
        query["identifier"] = serde_json::json!(identifier);
    }
    if let Some(role) = role {
        query["role"] = serde_json::json!(role);
    }
    if let Some(label) = label {
        query["label"] = serde_json::json!({"text":label,"mode":"substring"});
    }
    query
}
/// Inspection and recovery through the user's manager.
#[derive(Subcommand)]
pub enum ManagerCommand {
    /// Show the manager's identity and its connected devices and browsers.
    Status,
    /// List what keeps devices or the manager from being released.
    Blockers,
    /// Run unfinished releases now: the --target device's, or every blocker's.
    Retry,
    /// Show a retry operation.
    #[command(subcommand)]
    Operation(ManagerOperationCommand),
    /// Stop the manager if no device or client needs it.
    Stop,
    /// Refuse new connections, release every device and stop.
    Drain,
}
#[derive(Subcommand)]
pub enum ManagerOperationCommand {
    /// Show a retry operation by its ID.
    Show { operation_id: String },
}

#[derive(Parser)]
#[command(
    name = "testrun",
    version,
    about = "QA for mobile devices and isolated browser sessions, from the command line or as an MCP server"
)]
pub struct CommandLine {
    /// Print one JSON result or error, with no prompts or human notices.
    #[arg(long, global = true)]
    pub json: bool,
    /// Target from `target list`, such as simulator:UDID, device:SERIAL or android:SERIAL.
    #[arg(long, global = true)]
    pub(super) target: Option<String>,
    #[command(subcommand)]
    pub(super) command: Command,
}
#[derive(Subcommand)]
pub(super) enum Command {
    /// Read the bundled guides.
    #[command(subcommand)]
    Docs(DocsCommand),
    /// Detect application projects and create reviewed Testrun configuration.
    Init(super::init::Options),
    /// Initialize, validate or inspect optional test-build configuration.
    Config {
        /// Project directory that holds the configuration.
        #[arg(long, alias = "root", global = true, default_value = ".")]
        project: PathBuf,
        /// Configuration file to use instead of testrun.yml or .config/testrun.yml.
        #[arg(long, global = true)]
        config: Option<PathBuf>,
        #[command(subcommand)]
        command: super::builds::ConfigCommand,
    },
    /// Start, inspect or cancel test-app builds.
    #[command(subcommand)]
    Build(BuildCommand),
    /// Inspect project readiness or run a declared development task.
    Project {
        /// Configuration file to use instead of testrun.yml or .config/testrun.yml.
        #[arg(long, global = true)]
        config: Option<PathBuf>,
        #[command(subcommand)]
        command: super::projects::ProjectCommand,
    },
    /// Collect and inspect bounded application diagnostic evidence.
    #[command(subcommand)]
    Diagnostics(super::diagnostics::DiagnosticCommand),
    /// Run Testrun's MCP server or register it with agents.
    #[command(subcommand)]
    Mcp(McpCommand),
    /// Check local development signing and choose the default XCTest signing team.
    #[command(subcommand)]
    Signing(SigningCommand),
    /// End this host task's hold on devices.
    #[command(subcommand)]
    Task(TaskCommand),
    /// Inspect the manager's devices and blockers and recover releases.
    Manager {
        #[command(subcommand)]
        command: ManagerCommand,
    },
    #[command(flatten)]
    Mobile(Action),
    #[command(hide = true)]
    NativeIdentity(Native),
    #[command(hide = true)]
    NativeInput {
        #[command(flatten)]
        native: Native,
        #[arg(long)]
        identity: String,
    },
    #[command(hide = true)]
    NativeSource(Native),
    #[command(hide = true)]
    NativeVerifyVideo {
        #[arg(long)]
        path: PathBuf,
    },
    #[command(hide = true)]
    NativeAccessibility(Native),
}
#[derive(Subcommand)]
pub(super) enum BuildCommand {
    /// Read a bounded private operation log; it may contain project secrets.
    Log {
        operation_id: String,
        #[arg(long, value_name = "CURSOR")]
        cursor: Option<String>,
        #[arg(long,default_value="16384",value_parser=clap::value_parser!(u64).range(1..=65536))]
        limit: u64,
    },
    /// Build an app, optionally install it and launch it on --target.
    Start(Box<super::builds::Options>),
    /// Show a retained test-app build without repeating it.
    Status {
        #[arg(conflicts_with = "submission_id")]
        operation_id: Option<String>,
        #[arg(long)]
        submission_id: Option<String>,
        #[arg(long,default_value="50",value_parser=clap::value_parser!(u64).range(1..=100))]
        limit: u64,
        #[arg(long, value_name = "CURSOR")]
        cursor: Option<String>,
    },
    /// Cancel a build or stop before its next device stage.
    Cancel {
        #[arg(
            required_unless_present = "submission_id",
            conflicts_with = "submission_id"
        )]
        operation_id: Option<String>,
        #[arg(long)]
        submission_id: Option<String>,
    },
}
#[derive(Subcommand)]
pub(super) enum McpCommand {
    /// Serve newline-delimited MCP over stdin/stdout.
    Serve {
        /// Validate server registration and print tools without opening stdio.
        #[arg(long)]
        check: bool,
    },
    /// Install, repair or remove selected agent MCP registrations (project scope by default).
    Setup(crate::agent_setup::Options),
}
#[derive(Subcommand)]
pub(super) enum SigningCommand {
    /// Check local development signing and save the default XCTest signing team.
    Setup {
        /// Save this eligible team as the host default.
        #[arg(long)]
        runner_team_id: Option<String>,
        /// Return structured choices instead of prompting, even in a terminal.
        #[arg(long)]
        non_interactive: bool,
    },
}
#[derive(Subcommand)]
pub(super) enum TaskCommand {
    /// End the host task; devices stay connected for other clients.
    Stop,
}
#[derive(Args)]
pub(super) struct Native {
    #[arg(long)]
    pub(super) developer: String,
    #[arg(long)]
    pub(super) id: String,
}

impl CommandLine {
    /// Whether this invocation owns the MCP stdin reader rather than a device runtime.
    #[must_use]
    pub fn is_mcp_server(&self) -> bool {
        matches!(
            self.command,
            Command::Mcp(McpCommand::Serve { check: false })
        )
    }
    /// Whether results and errors use JSON: `--json`, or a hidden entry point
    /// whose owned clients read a fixed protocol.
    #[must_use]
    pub fn machine_output(&self) -> bool {
        self.json
            || matches!(
                self.command,
                Command::Mobile(
                    Action::Session { .. }
                        | Action::ManagedOwner { .. }
                        | Action::Batch { .. }
                        | Action::Close { .. }
                        | Action::Status {
                            endpoint: Some(_),
                            ..
                        }
                )
            )
    }
}

/// Bundled documentation commands, with the same arguments as MCP.
#[derive(Debug, Clone, clap::Subcommand)]
pub(super) enum DocsCommand {
    /// List bundled page paths and titles.
    List {
        #[arg(long, value_name = "CURSOR")]
        cursor: Option<String>,
        #[arg(long, value_name = "N", value_parser = clap::value_parser!(u64).range(1..=canonic_mcp_kit::docs::MAX_ROWS))]
        limit: Option<u64>,
    },
    /// Search bundled guides for lines matching all terms.
    Search {
        query: String,
        #[arg(long)]
        path: Option<String>,
        #[arg(long, value_name = "CURSOR")]
        cursor: Option<String>,
        #[arg(long, value_name = "N", value_parser = clap::value_parser!(u64).range(1..=canonic_mcp_kit::docs::MAX_ROWS))]
        limit: Option<u64>,
    },
    /// Read lines of a bundled guide.
    Read {
        path: String,
        #[arg(long, value_name = "CURSOR")]
        cursor: Option<String>,
        /// Number of lines to skip before reading.
        #[arg(long, value_name = "N", conflicts_with = "cursor", value_parser = clap::value_parser!(u64).range(0..=canonic_mcp_kit::docs::MAX_PAGE_BYTES as u64))]
        offset: Option<u64>,
        #[arg(long, value_name = "N", value_parser = clap::value_parser!(u64).range(1..=canonic_mcp_kit::docs::MAX_READ_LINES))]
        limit: Option<u64>,
    },
}

impl DocsCommand {
    pub(crate) fn tool(&self) -> (&'static str, serde_json::Value) {
        use canonic_mcp_kit::page::arguments;
        use serde_json::json;
        match self {
            Self::List { cursor, limit } => ("docs_list", arguments(cursor.as_deref(), *limit)),
            Self::Search {
                query,
                path,
                cursor,
                limit,
            } => {
                let mut args = arguments(cursor.as_deref(), *limit);
                args["query"] = json!(query);
                if let Some(path) = path {
                    args["path"] = json!(path);
                }
                ("docs_search", args)
            }
            Self::Read {
                path,
                cursor,
                offset,
                limit,
            } => {
                let mut args = arguments(cursor.as_deref(), *limit);
                args["path"] = json!(path);
                if let Some(offset) = offset {
                    args["offset"] = json!(offset);
                }
                ("docs_read", args)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn named_project_selectors_preserve_legacy_forms() {
        use super::CommandLine;
        use clap::Parser;
        for arguments in [
            vec!["testrun", "project", "list", "--root", "."],
            vec!["testrun", "project", "show", "mobile", "--root", "."],
            vec![
                "testrun",
                "project",
                "status",
                "mobile",
                "--profile",
                "dev",
                "--root",
                ".",
            ],
            vec![
                "testrun",
                "project",
                "start",
                "mobile",
                "--profile",
                "dev",
                "--root",
                ".",
                "--task",
                "build",
            ],
            vec!["testrun", "project", "status", "ios-dev", "--project", "."],
            vec![
                "testrun", "build", "start", "dev", "--app", "mobile", "--root", ".",
            ],
            vec!["testrun", "config", "schema", "--version", "1"],
            vec!["testrun", "config", "check", "--root", "."],
        ] {
            assert!(
                CommandLine::try_parse_from(arguments.clone()).is_ok(),
                "{arguments:?}"
            );
        }
        assert!(
            CommandLine::try_parse_from(["testrun", "project", "status", "mobile", "--root", "."])
                .is_err()
        );
    }
    #[test]
    fn shared_browser_verbs_parse_without_changing_mobile_defaults() {
        use super::{Action, Command, CommandLine};
        use clap::Parser;
        for arguments in [
            vec!["testrun", "connect", "--engine", "firefox"],
            vec![
                "testrun",
                "act",
                "[{\"action\":\"page_list\"}]",
                "--target",
                "browser:Acme",
                "--timeout-ms",
                "5000",
            ],
            vec![
                "testrun",
                "observe",
                "--target",
                "browser:Acme",
                "--page",
                "{}",
            ],
            vec![
                "testrun",
                "status",
                "--target",
                "browser:Acme",
                "--action-id",
                "Acme",
                "--wait-ms",
                "30000",
            ],
            vec![
                "testrun",
                "cancel",
                "--target",
                "browser:Acme",
                "--action-id",
                "Acme",
            ],
        ] {
            CommandLine::try_parse_from(arguments).unwrap();
        }
        let parsed =
            CommandLine::try_parse_from(["testrun", "connect", "--target", "android:Acme"])
                .unwrap();
        assert!(matches!(
            parsed.command,
            Command::Mobile(Action::Connect {
                engine: None,
                mode: None,
                ..
            })
        ));
        assert!(CommandLine::try_parse_from(["testrun", "connect", "--mode", "headed"]).is_err());
    }
    use super::*;
    use clap::CommandFactory;

    fn parse(args: &[&str]) -> CommandLine {
        CommandLine::try_parse_from(std::iter::once("testrun").chain(args.iter().copied())).unwrap()
    }
    #[test]
    fn browser_media_fixture_flags_are_shared_by_both_connect_commands() {
        for mut words in [vec!["connect"], vec!["browser", "connect"]] {
            words.extend([
                "--engine",
                "chrome",
                "--microphone-fixture",
                "Acme speech.wav",
                "--camera-fixture",
                "Acme video.y4m",
            ]);
            let Command::Mobile(action) = parse(&words).command else {
                panic!("Expected connect");
            };
            let options = match action {
                Action::Connect { media_fixtures, .. }
                | Action::Browser(BrowserCommand::Connect { media_fixtures, .. }) => media_fixtures,
                _ => panic!("Expected connect"),
            };
            let mut args = serde_json::json!({});
            options.add_to(&mut args);
            assert_eq!(
                args["mediaFixtures"],
                serde_json::json!({
                    "microphonePath":"Acme speech.wav","cameraPath":"Acme video.y4m"
                })
            );
        }
        assert!(
            CommandLine::try_parse_from([
                "testrun",
                "connect",
                "--target",
                "simulator:acme",
                "--microphone-fixture",
                "audio.wav"
            ])
            .is_err()
        );
    }

    #[test]
    fn browser_attachment_and_profile_flags_are_shared_by_both_connect_commands() {
        for words in [
            vec!["connect", "--engine", "chrome", "--disposable-profile"],
            vec![
                "browser",
                "connect",
                "--engine",
                "chrome",
                "--disposable-profile",
            ],
        ] {
            let Command::Mobile(action) = parse(&words).command else {
                panic!("Expected connect");
            };
            let options = match action {
                Action::Connect {
                    browser_connection, ..
                }
                | Action::Browser(BrowserCommand::Connect {
                    browser_connection, ..
                }) => browser_connection,
                _ => panic!("Expected browser connect"),
            };
            let mut args = serde_json::json!({});
            options.add_to(&mut args);
            assert_eq!(args, serde_json::json!({"profile":{"kind":"disposable"}}));
        }
        for mut words in [vec!["connect"], vec!["browser", "connect"]] {
            words.extend([
                "--engine",
                "chrome",
                "--attach",
                "ws://127.0.0.1:9222/devtools/browser/acme",
                "--adopt-context",
                "acme",
            ]);
            let Command::Mobile(action) = parse(&words).command else {
                panic!("Expected connect");
            };
            let options = match action {
                Action::Connect {
                    browser_connection, ..
                }
                | Action::Browser(BrowserCommand::Connect {
                    browser_connection, ..
                }) => browser_connection,
                _ => panic!("Expected browser connect"),
            };
            let mut args = serde_json::json!({});
            options.add_to(&mut args);
            assert_eq!(
                args,
                serde_json::json!({"attachment":{
                    "endpoint":"ws://127.0.0.1:9222/devtools/browser/acme","contextId":"acme"
                }})
            );
        }
        assert!(
            CommandLine::try_parse_from([
                "testrun",
                "connect",
                "--engine",
                "chrome",
                "--profile",
                "acme",
                "--borrowed-profile",
                "/private/tmp/acme",
            ])
            .is_err()
        );
    }

    #[test]
    fn launch_proxy_arguments_require_browser_scope_and_preserve_json() {
        let proxy =
            "{\"server\":\"http://127.0.0.1:8080\",\"username\":\"Acme\",\"password\":\"example\"}";
        for arguments in [
            vec!["connect", "--engine", "chrome", "--proxy", proxy],
            vec![
                "browser", "connect", "--engine", "firefox", "--proxy", proxy,
            ],
        ] {
            let Command::Mobile(action) = parse(&arguments).command else {
                panic!("Expected browser launch");
            };
            let selected = match action {
                Action::Connect { proxy, .. }
                | Action::Browser(BrowserCommand::Connect { proxy, .. }) => proxy,
                _ => panic!("Expected browser launch"),
            };
            assert_eq!(selected.as_deref(), Some(proxy));
        }
        for arguments in [
            vec!["testrun", "connect", "--proxy", proxy],
            vec![
                "testrun",
                "connect",
                "--target",
                "android:Acme",
                "--proxy",
                proxy,
            ],
            vec![
                "testrun", "browser", "connect", "--engine", "chrome", "--proxy",
            ],
        ] {
            assert!(CommandLine::try_parse_from(arguments).is_err());
        }
        let catalog = crate::catalog::tools();
        for name in ["browser_connect", "connect"] {
            let tool = catalog.iter().find(|tool| tool["name"] == name).unwrap();
            crate::schema::validate(&tool["inputSchema"],&serde_json::json!({"engine":"chrome","mode":"headless","proxy":{"server":"127.0.0.1:8080"}})).unwrap();
            assert!(crate::schema::validate(&tool["inputSchema"],&serde_json::json!({"targetId":"android:Acme","proxy":{"server":"127.0.0.1:8080"}})).is_err());
        }
    }
    #[test]
    fn only_mcp_stdio_uses_nonblocking_runtime_shutdown() {
        assert!(parse(&["mcp", "serve"]).is_mcp_server());
        for args in [
            vec!["mcp", "serve", "--check"],
            vec!["target", "list"],
            vec!["manager", "status"],
            vec!["signing", "setup"],
        ] {
            assert!(!parse(&args).is_mcp_server());
        }
    }
    #[test]
    fn hidden_protocol_entry_points_always_use_machine_output() {
        for args in [
            vec!["close", "--endpoint", "/tmp/acme/control.sock"],
            vec!["batch", "--endpoint", "/tmp/acme/control.sock", "[]"],
            vec!["status", "--endpoint", "/tmp/acme/control.sock"],
            vec!["managed-owner", "--root", "/tmp/acme"],
            vec!["session"],
        ] {
            let input = parse(&args);
            assert!(!input.json && input.machine_output(), "{args:?}");
        }
        for args in [
            vec!["status"],
            vec!["target", "list"],
            vec!["signing", "setup"],
        ] {
            assert!(!parse(&args).machine_output(), "{args:?}");
        }
    }
    #[test]
    fn machine_output_is_explicit_and_global() {
        assert!(!parse(&["target", "list"]).json);
        for args in [
            vec!["--json", "target", "list"],
            vec!["target", "list", "--json"],
            vec!["signing", "setup", "--json"],
            vec!["manager", "blockers", "--json"],
        ] {
            assert!(parse(&args).json);
        }
    }
    #[test]
    fn signing_setup_is_independent_of_target_and_accepts_an_unattended_default_selection() {
        let CommandLine {
            target,
            command:
                Command::Signing(SigningCommand::Setup {
                    runner_team_id,
                    non_interactive,
                }),
            ..
        } = parse(&[
            "signing",
            "setup",
            "--runner-team-id",
            "ACME000001",
            "--non-interactive",
        ])
        else {
            panic!("Expected signing setup");
        };
        assert!(target.is_none());
        assert_eq!(runner_team_id.as_deref(), Some("ACME000001"));
        assert!(non_interactive);
        parse(&["signing", "setup"]);
        for args in [["setup", "ios"], ["signing", "android"]] {
            assert!(CommandLine::try_parse_from(std::iter::once("testrun").chain(args)).is_err());
        }
    }
    #[test]
    fn mcp_setup_defaults_to_project_and_keeps_explicit_selection() {
        let input = parse(&[
            "mcp",
            "setup",
            "--agent",
            "codex",
            "--agent",
            "claude",
            "--dry-run",
            "--json",
        ]);
        assert!(input.json);
        let Command::Mcp(McpCommand::Setup(options)) = input.command else {
            panic!("Expected MCP setup")
        };
        assert_eq!(options.scope, crate::agent_setup::Scope::Project);
        assert_eq!(options.agents, ["codex", "claude"]);
        assert!(options.dry_run);
        assert!(!options.yes);
        let Command::Mcp(McpCommand::Setup(options)) = parse(&[
            "mcp", "setup", "--scope", "user", "--remove", "--agent", "kimi", "--yes",
        ])
        .command
        else {
            panic!("Expected MCP setup")
        };
        assert_eq!(options.scope, crate::agent_setup::Scope::User);
        assert!(options.remove && options.yes);
        for args in [vec!["serve"], vec!["setup", "mcp"]] {
            assert!(CommandLine::try_parse_from(std::iter::once("testrun").chain(args)).is_err());
        }
    }
    #[test]
    fn reset_tool_permissions_is_exposed_by_testrun_setup() {
        let input = parse(&[
            "mcp",
            "setup",
            "--agent",
            "codex",
            "--reset-tool-permissions",
            "--dry-run",
            "--json",
        ]);
        let Command::Mcp(McpCommand::Setup(options)) = input.command else {
            panic!("Expected MCP setup");
        };
        assert!(options.reset_tool_permissions && options.dry_run && input.json);
        assert!(
            CommandLine::try_parse_from([
                "testrun",
                "mcp",
                "setup",
                "--reset-tool-permissions",
                "--remove",
            ])
            .is_err()
        );
    }
    #[test]
    fn browser_commands_use_explicit_engines_and_strict_bounded_request_envelopes() {
        for args in [
            vec!["browser", "list"],
            vec!["browser", "connect", "--engine", "chrome"],
            vec!["browser", "doctor", "--engine", "chrome"],
            vec!["connect", "--engine", "chrome"],
            vec!["browser", "connect", "--engine", "firefox"],
            vec!["browser", "connect", "--engine", "firefox", "--downloads"],
            vec![
                "browser",
                "fixture-register",
                "Acme.csv",
                "--mime-type",
                "text/csv",
                "--provenance",
                "synthetic_generated",
                "--cleared-by",
                "Acme QA",
                "--clearance-method",
                "Generated",
            ],
            vec![
                "browser", "connect", "--engine", "webkit", "--mode", "headed",
            ],
            vec!["browser", "send", "{\"operation\":\"page_list\"}"],
            vec!["browser", "status", "--wait-ms", "30000"],
        ] {
            parse(&args);
        }
        for args in [
            vec!["browser", "connect", "--engine", "chromium"],
            vec!["browser", "connect"],
            vec!["browser", "status", "--wait-ms", "30001"],
            vec!["browser", "send", "{}", "--timeout-ms", "0"],
            vec!["browser", "operation", "{}"],
            vec!["browser-list"],
        ] {
            assert!(CommandLine::try_parse_from(std::iter::once("testrun").chain(args)).is_err());
        }
    }

    /// Published tools have matching commands; onboarding uses the top-level
    /// init workflow instead of spelling its MCP operation names.
    #[test]
    fn every_published_tool_has_the_matching_command() {
        let command = CommandLine::command();
        let path = |name: &str| -> Vec<String> {
            let mut words = Vec::new();
            let mut current = &command;
            let mut rest = match name {
                "project_init" | "project_init_preview" => "init",
                _ => name,
            };
            'words: while !rest.is_empty() {
                let mut subcommands: Vec<_> = current
                    .get_subcommands()
                    .filter(|sub| !sub.is_hide_set())
                    .collect();
                // Prefer the longest command name that matches the remaining words.
                subcommands.sort_by_key(|sub| std::cmp::Reverse(sub.get_name().len()));
                for sub in subcommands {
                    let word = sub.get_name().replace('-', "_");
                    if rest == word || rest.starts_with(&format!("{word}_")) {
                        words.push(sub.get_name().to_owned());
                        rest = rest[word.len()..].trim_start_matches('_');
                        current = sub;
                        continue 'words;
                    }
                }
                return Vec::new();
            }
            assert!(
                !current.has_subcommands(),
                "{name} names a command group, not a command"
            );
            words
        };
        for tool in crate::catalog::tools() {
            let name = tool["name"].as_str().unwrap();
            assert!(!path(name).is_empty(), "{name} has no matching command");
        }
        for (name, words) in [
            ("target_list", vec!["target", "list"]),
            ("browser_send", vec!["browser", "send"]),
            ("developer_mode_reveal", vec!["developer-mode", "reveal"]),
            ("build_start", vec!["build", "start"]),
            ("project_discover", vec!["project", "discover"]),
            ("project_init", vec!["init"]),
            ("project_init_preview", vec!["init"]),
            ("docs_read", vec!["docs", "read"]),
            ("act", vec!["act"]),
        ] {
            assert_eq!(path(name), words, "{name}");
        }
    }

    #[test]
    fn collection_tools_and_commands_publish_both_paging_arguments() {
        let command = CommandLine::command();
        let tools = crate::catalog::tools();
        for (name, path) in [
            ("target_list", vec!["target", "list"]),
            ("app_list", vec!["app", "list"]),
            ("project_list", vec!["project", "list"]),
            ("project_discover", vec!["project", "discover"]),
            ("browser_list", vec!["browser", "list"]),
            ("browser_status", vec!["browser", "status"]),
            ("status", vec!["status"]),
            ("ui_find", vec!["ui", "find"]),
            ("ui_snapshot", vec!["ui", "snapshot"]),
            ("diagnostics_read", vec!["diagnostics", "read"]),
            ("diagnostics_query", vec!["diagnostics", "query"]),
            ("build_log", vec!["build", "log"]),
            ("build_status", vec!["build", "status"]),
            ("docs_list", vec!["docs", "list"]),
            ("docs_search", vec!["docs", "search"]),
            ("docs_read", vec!["docs", "read"]),
        ] {
            let tool = tools
                .iter()
                .find(|tool| tool["name"] == name)
                .expect("published tool");
            canonic_mcp_kit::schema::PreparedSchema::new(&tool["inputSchema"])
                .expect("valid collection schema");
            let mut cli = &command;
            for part in path {
                cli = cli.find_subcommand(part).expect("matching CLI command");
            }
            for field in ["cursor", "limit"] {
                assert!(
                    !tool["inputSchema"]["properties"][field].is_null(),
                    "{name}: {field}"
                );
                assert!(
                    cli.get_arguments().any(|arg| arg.get_long() == Some(field)),
                    "{name}: --{field}"
                );
            }
        }
        for operation in ["events_read", "diagnostics_read"] {
            let request = crate::browser::contract::schemas();
            let branches = request["request"]["$defs"]["browserRequest"]["oneOf"]
                .as_array()
                .expect("browser operations");
            let branch = branches
                .iter()
                .find(|branch| branch["properties"]["operation"]["const"] == operation)
                .expect("read operation");
            let properties = if operation == "diagnostics_read" {
                &branch["properties"]["query"]["properties"]
            } else {
                &branch["properties"]
            };
            for field in ["cursor", "limit"] {
                assert!(!properties[field].is_null(), "browser {operation}: {field}");
            }
        }
    }

    #[test]
    fn commands_are_noun_verb_with_one_global_target() {
        let mut command = CommandLine::command();
        let help = command.render_long_help().to_string();
        for hidden in ["session", "batch", "close", "managed-owner", "native-input"] {
            assert!(
                !help
                    .lines()
                    .any(|line| line.trim_start().starts_with(hidden)),
                "{hidden}"
            );
        }
        for flat in [
            "targets-list",
            "apps-list",
            "app-install",
            "ui-find",
            "media-control",
            "network-set",
            "build-status",
            "task-end",
        ] {
            assert!(
                CommandLine::try_parse_from(["testrun", flat]).is_err(),
                "{flat}"
            );
        }
        let CommandLine {
            target,
            command: Command::Mobile(Action::Act { actions, .. }),
            ..
        } = parse(&["act", "--target", "simulator:Acme", "{\"action\":\"tap\"}"])
        else {
            panic!("Expected act")
        };
        assert_eq!(target.as_deref(), Some("simulator:Acme"));
        assert_eq!(actions, "{\"action\":\"tap\"}");
        // Clients name no session: every client reaches the same devices.
        assert!(CommandLine::try_parse_from(["testrun", "status", "--session", "acme"]).is_err());
        let CommandLine {
            target,
            command:
                Command::Manager {
                    command: ManagerCommand::Retry,
                },
            ..
        } = parse(&["manager", "retry", "--target", "device:Acme"])
        else {
            panic!("Expected manager retry")
        };
        assert_eq!(target.as_deref(), Some("device:Acme"));
        let CommandLine {
            command:
                Command::Manager {
                    command:
                        ManagerCommand::Operation(ManagerOperationCommand::Show { operation_id }),
                },
            ..
        } = parse(&["manager", "operation", "show", "acme-1"])
        else {
            panic!("Expected manager operation show")
        };
        assert_eq!(operation_id, "acme-1");
        parse(&["manager", "stop"]);
        assert!(CommandLine::try_parse_from(["testrun", "manager", "retire"]).is_err());
        let CommandLine {
            command: Command::Config { project, .. },
            ..
        } = parse(&["config", "check", "--project", "/example/acme"])
        else {
            panic!("Expected config check")
        };
        assert_eq!(project, PathBuf::from("/example/acme"));
    }

    #[test]
    fn media_destination_is_one_explicit_choice() {
        let CommandLine {
            command:
                Command::Mobile(Action::Media(MediaCommand::Send {
                    media_session,
                    system_player,
                    ..
                })),
            ..
        } = parse(&["media", "send", "pause", "--media-session", "acme-1"])
        else {
            panic!("Expected media send")
        };
        assert_eq!(media_session.as_deref(), Some("acme-1"));
        assert!(!system_player);
        for args in [
            vec!["testrun", "media", "send", "pause"],
            vec![
                "testrun",
                "media",
                "send",
                "pause",
                "--system-player",
                "--media-session",
                "acme-1",
            ],
        ] {
            assert!(CommandLine::try_parse_from(args).is_err());
        }
    }

    #[test]
    fn commands_start_without_environment_configuration_and_reject_unknown_flags() {
        for args in [
            vec!["testrun", "mcp", "serve"],
            vec!["testrun", "connect", "--target", "device:Acme"],
            vec!["testrun", "observe", "--target", "android:Acme"],
        ] {
            CommandLine::try_parse_from(args).unwrap();
        }
        for args in [
            vec!["testrun", "mcp", "serve", "--config", "unused.json"],
            vec!["testrun", "connect", "--id", "device:Acme"],
            vec!["testrun", "ios", "connect"],
            // A retained command ID means nothing without its endpoint.
            vec!["testrun", "status", "--command-id", "acme-1"],
        ] {
            assert!(CommandLine::try_parse_from(args).is_err());
        }
        CommandLine::try_parse_from([
            "testrun",
            "status",
            "--endpoint",
            "/example/acme.sock",
            "--command-id",
            "acme-1",
        ])
        .unwrap();
        let CommandLine {
            command:
                Command::Mobile(Action::Connect {
                    runner_team_id,
                    setup,
                    ..
                }),
            ..
        } = parse(&[
            "connect",
            "--target",
            "device:Acme",
            "--setup",
            "--runner-team-id",
            "ACMETEAM",
        ])
        else {
            panic!("Expected connect")
        };
        assert!(setup);
        assert_eq!(runner_team_id.as_deref(), Some("ACMETEAM"));
    }

    #[test]
    fn connect_android_capture_is_explicit_and_rejects_unknown_modes() {
        for (argument, expected) in [
            (None, None),
            (Some("screenshot"), Some("screenshot")),
            (Some("stream"), Some("stream")),
        ] {
            let mut arguments = vec!["testrun", "connect", "--target", "android:Acme"];
            if let Some(mode) = argument {
                arguments.extend(["--android-capture", mode]);
            }
            let parsed = CommandLine::try_parse_from(arguments).unwrap();
            let Command::Mobile(Action::Connect {
                android_capture, ..
            }) = parsed.command
            else {
                panic!("Expected connect")
            };
            assert_eq!(android_capture.as_deref(), expected);
        }
        assert!(
            CommandLine::try_parse_from([
                "testrun",
                "connect",
                "--target",
                "android:Acme",
                "--android-capture",
                "auto"
            ])
            .is_err()
        );
    }
}
#[cfg(test)]
mod documentation_tests {
    use super::*;
    use clap::{CommandFactory, Parser};

    #[test]
    fn docs_cli_maps_cursor_and_limit_to_shared_tools() {
        let command = CommandLine::command();
        let docs_group = command.find_subcommand("docs").unwrap();
        for (verb, positional) in [
            ("list", None),
            ("search", Some("needle")),
            ("read", Some("mcp.md")),
        ] {
            let subcommand = docs_group.find_subcommand(verb).unwrap();
            for flag in ["cursor", "limit"] {
                assert!(
                    subcommand
                        .get_arguments()
                        .any(|argument| argument.get_long() == Some(flag))
                );
            }
            let mut words = vec!["testrun", "docs", verb];
            if let Some(value) = positional {
                words.push(value);
            }
            words.extend(["--cursor", "opaque", "--limit", "2"]);
            let input = CommandLine::try_parse_from(words.clone()).unwrap();
            let Command::Docs(command) = input.command else {
                panic!("docs command");
            };
            let (name, arguments) = command.tool();
            assert_eq!(name, format!("docs_{verb}"));
            assert_eq!(arguments["cursor"], "opaque");
            assert_eq!(arguments["limit"], 2);
            for invalid in ["0", if verb == "read" { "401" } else { "101" }] {
                *words.last_mut().unwrap() = invalid;
                assert!(CommandLine::try_parse_from(words.clone()).is_err());
            }
            let mut old = vec!["testrun", "docs", verb];
            if let Some(value) = positional {
                old.push(value);
            }
            old.extend([
                if verb == "read" {
                    "--start-line"
                } else {
                    "--offset"
                },
                "1",
            ]);
            assert!(CommandLine::try_parse_from(old).is_err());
        }
        let offset = ["testrun", "docs", "read", "mcp.md", "--offset", "2"];
        let input = CommandLine::try_parse_from(offset).unwrap();
        let Command::Docs(command) = input.command else {
            panic!("docs command");
        };
        assert_eq!(command.tool().1["offset"], 2);
        assert!(
            CommandLine::try_parse_from(offset.into_iter().chain(["--cursor", "opaque"])).is_err()
        );
        for tool in crate::catalog::tools()
            .into_iter()
            .filter(|tool| tool["name"].as_str().unwrap().starts_with("docs_"))
        {
            assert!(tool["inputSchema"]["properties"].get("cursor").is_some());
            assert!(tool["inputSchema"]["properties"].get("limit").is_some());
        }
    }
}

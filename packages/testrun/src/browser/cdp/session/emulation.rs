//! Native context and page emulation following the pinned SDK's Chromium frame
//! sessions: creation options, mutable environment settings and page media on
//! every owned page, frame and service-worker attachment.

use super::*;
use crate::browser::cdp::resources::WorkerKind;
use crate::browser::settings::{
    self, ContextSetting, MediaOptions, NetworkPreset, Permission, ServiceWorkers,
    ThrottlingOptions,
};

/// Page media and throttling overrides and the per-origin grants the pinned
/// SDK accumulates, because Chrome's `Browser.grantPermissions` replaces an
/// origin's set.
#[derive(Default)]
pub(super) struct State {
    pub(super) page_media: BTreeMap<Reference, MediaOptions>,
    page_throttling: BTreeMap<Reference, ThrottlingOptions>,
    pub(super) granted: BTreeMap<Reference, BTreeMap<String, BTreeSet<Permission>>>,
}

impl State {
    /// Drops overrides whose page or context the index no longer resolves.
    pub(super) fn prune(&mut self, index: &Index) {
        self.page_media
            .retain(|page, _| index.native(*page, Kind::Page).is_ok());
        self.page_throttling
            .retain(|page, _| index.native(*page, Kind::Page).is_ok());
        self.granted
            .retain(|context, _| index.native(*context, Kind::Context).is_ok());
    }
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }
}

/// The private CDP reply message of a definite method failure.
fn remote_message(error: &Error) -> Option<&str> {
    let cause = std::error::Error::source(error)?;
    match cause.downcast_ref::<crate::browser::cdp::Error>()? {
        crate::browser::cdp::Error::Remote(crate::browser::cdp::RemoteError::Command {
            message,
            ..
        }) => Some(message),
        _ => None,
    }
}

/// The version after `marker`, as the pinned regular expressions capture it:
/// up to `groups` digit runs joined by `separator`.
fn version_after(
    agent: &str,
    marker: &str,
    separator: char,
    groups: usize,
    skip_non_digits: bool,
) -> Option<String> {
    let digits = |text: &str| text.bytes().take_while(u8::is_ascii_digit).count();
    for (start, _) in agent.match_indices(marker) {
        let mut rest = &agent[start + marker.len()..];
        if skip_non_digits {
            let trimmed = rest.trim_start_matches(|c: char| !c.is_ascii_digit());
            if trimmed.len() == rest.len() {
                continue;
            }
            rest = trimmed;
        }
        let mut end = digits(rest);
        if end == 0 {
            continue;
        }
        for _ in 1..groups {
            let Some(next) = rest[end..].strip_prefix(separator) else {
                break;
            };
            let count = digits(next);
            if count == 0 {
                break;
            }
            end += separator.len_utf8() + count;
        }
        return Some(rest[..end].to_owned());
    }
    None
}

/// Pinned SDK `calculateUserAgentMetadata`.
pub(super) fn user_agent_metadata(options: &ContextOptions) -> Option<Value> {
    let agent = options.user_agent.as_deref()?;
    let (mut platform, mut version, mut architecture) = ("Windows", String::new(), "x86");
    if let Some(found) = version_after(agent, "Android ", '.', 3, false) {
        (platform, version, architecture) = ("Android", found, "arm");
    } else if let Some(found) = version_after(agent, "iPhone OS ", '_', 2, false)
        .or_else(|| version_after(agent, "iPad; CPU OS ", '_', 2, false))
    {
        (platform, version, architecture) = ("iOS", found, "arm");
    } else if let Some(found) = version_after(agent, "Mac OS X ", '_', 3, false) {
        (platform, version) = ("macOS", found);
        if !agent.contains("Intel") {
            architecture = "arm";
        }
    } else if let Some(found) = version_after(agent, "Windows", '.', 3, true) {
        version = found;
    } else if agent.to_lowercase().contains("linux") {
        platform = "Linux";
    }
    if agent.contains("ARM") {
        architecture = "arm";
    }
    Some(
        json!({"mobile":options.is_mobile == Some(true),"model":"","architecture":architecture,"platform":platform,"platformVersion":version}),
    )
}

fn user_agent_override(options: &ContextOptions) -> Value {
    let mut value = json!({"userAgent":options.user_agent.as_deref().unwrap_or("")});
    if let Some(locale) = &options.locale {
        value["acceptLanguage"] = json!(locale);
    }
    if let Some(metadata) = user_agent_metadata(options) {
        value["userAgentMetadata"] = metadata;
    }
    value
}

/// Pinned `emulatedMedia()` with Chromium's `no-override` as an empty value.
fn emulated_media(options: &ContextOptions, page: Option<&MediaOptions>) -> Value {
    fn name<T: serde::Serialize>(value: T) -> String {
        match serde_json::to_value(value) {
            Ok(Value::String(name)) if name != "no-override" => name,
            _ => String::new(),
        }
    }
    let page = page.cloned().unwrap_or_default();
    let color = page
        .color_scheme
        .or(options.color_scheme)
        .unwrap_or(settings::ColorScheme::Light);
    let motion = page
        .reduced_motion
        .or(options.reduced_motion)
        .unwrap_or(settings::ReducedMotion::NoPreference);
    let forced = page
        .forced_colors
        .or(options.forced_colors)
        .unwrap_or(settings::ForcedColors::None);
    let contrast = page
        .contrast
        .or(options.contrast)
        .unwrap_or(settings::Contrast::NoPreference);
    json!({"media":page.media.map(name).unwrap_or_default(),"features":[
        {"name":"prefers-color-scheme","value":name(color)},
        {"name":"prefers-reduced-motion","value":name(motion)},
        {"name":"forced-colors","value":name(forced)},
        {"name":"prefers-contrast","value":name(contrast)}
    ]})
}

/// Pinned `_setOfflineForSession` (zero latency and -1 throughput clear
/// throttling), combined with a page's network preset.
fn network_conditions(offline: bool, preset: Option<NetworkPreset>) -> Value {
    let offline = offline || preset == Some(NetworkPreset::Offline);
    match preset.and_then(NetworkPreset::conditions) {
        Some((download, upload, latency)) => {
            json!({"offline":offline,"latency":latency,"downloadThroughput":download,"uploadThroughput":upload})
        }
        None => {
            json!({"offline":offline,"latency":0,"downloadThroughput":-1,"uploadThroughput":-1})
        }
    }
}

/// Pinned `FrameSession._initialize` overrides for one page or frame, in order.
fn frame_overrides(
    options: &ContextOptions,
    main_frame: bool,
    media: Option<&MediaOptions>,
    throttling: &ThrottlingOptions,
) -> Vec<(&'static str, Value)> {
    let mut calls = Vec::new();
    // Every owned context gets the SDK's main-frame focus emulation, so each
    // page behaves as focused although only one window has OS focus.
    if main_frame {
        calls.push((
            "Emulation.setFocusEmulationEnabled",
            json!({"enabled":true}),
        ));
    }
    if options.bypass_csp == Some(true) {
        calls.push(("Page.setBypassCSP", json!({"enabled":true})));
    }
    if options.java_script_enabled == Some(false) {
        calls.push((
            "Emulation.setScriptExecutionDisabled",
            json!({"value":true}),
        ));
    }
    if options.user_agent.is_some() || options.locale.is_some() {
        calls.push((
            "Emulation.setUserAgentOverride",
            user_agent_override(options),
        ));
    }
    if let Some(locale) = &options.locale {
        calls.push(("Emulation.setLocaleOverride", json!({"locale":locale})));
    }
    if let Some(timezone) = &options.timezone_id {
        calls.push((
            "Emulation.setTimezoneOverride",
            json!({"timezoneId":timezone}),
        ));
    }
    if let Some(geolocation) = &options.geolocation {
        calls.push(("Emulation.setGeolocationOverride", json!(geolocation)));
    }
    calls.push(("Emulation.setEmulatedMedia", emulated_media(options, media)));
    let preset = throttling
        .network
        .filter(|preset| *preset != NetworkPreset::None);
    if options.offline == Some(true) || preset.is_some() {
        calls.push(("Network.enable", json!({})));
        calls.push((
            "Network.emulateNetworkConditions",
            network_conditions(options.offline == Some(true), preset),
        ));
    }
    if let Some(rate) = throttling.cpu_rate.filter(|rate| *rate > 1.0) {
        calls.push(("Emulation.setCPUThrottlingRate", json!({"rate":rate})));
    }
    if options.service_workers == Some(ServiceWorkers::Block) {
        calls.push((
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source":settings::SERVICE_WORKER_BLOCK,"runImmediately":true}),
        ));
    }
    calls
}

/// Pinned `CRServiceWorker` user agent and offline updates.
fn service_worker_overrides(options: &ContextOptions) -> Vec<(&'static str, Value)> {
    let mut calls = Vec::new();
    if options.user_agent.is_some() || options.locale.is_some() {
        calls.push((
            "Emulation.setUserAgentOverride",
            user_agent_override(options),
        ));
    }
    if options.offline == Some(true) {
        calls.push(("Network.enable", json!({})));
        calls.push((
            "Network.emulateNetworkConditions",
            network_conditions(true, None),
        ));
    }
    calls
}

impl Worker {
    fn emulated_sessions(
        &self,
        context: Reference,
        page: Option<Reference>,
        service_workers: bool,
    ) -> BTreeSet<String> {
        let mut sessions = BTreeSet::new();
        for kind in [Kind::Page, Kind::Frame, Kind::Worker] {
            for (reference, _) in self.index.list(kind) {
                if kind == Kind::Worker
                    && !(service_workers
                        && matches!(
                            self.index.worker_scope(reference),
                            Ok((_, None, WorkerKind::Service))
                        ))
                {
                    continue;
                }
                let Ok(route) = self.index.route(reference, Some(kind)) else {
                    continue;
                };
                if self.index.diagnostic_scope(&route.session_id).is_some_and(
                    |(owner, owner_page, _)| {
                        owner == context && page.is_none_or(|page| owner_page == Some(page))
                    },
                ) {
                    sessions.insert(route.session_id);
                }
            }
        }
        sessions
    }

    /// Pinned `FrameSession._initialize` overrides for pages and frames and
    /// `CRServiceWorker` user agent/offline; dedicated and shared workers get none.
    pub(super) async fn setup_emulation(
        &mut self,
        id: Uuid,
        reference: Reference,
        kind: Kind,
        session: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        let Some((context, page, _)) = self.index.diagnostic_scope(session) else {
            return Ok(());
        };
        let Some(options) = self.context_layout.get(&context).cloned() else {
            return Ok(());
        };
        if kind == Kind::Worker {
            let service = matches!(
                self.index.worker_scope(reference),
                Ok((_, None, WorkerKind::Service))
            );
            if !service || options.service_workers == Some(ServiceWorkers::Block) {
                return Ok(());
            }
            for (method, params) in service_worker_overrides(&options) {
                self.service_worker_call(id, session, method, params, deadline)
                    .await?;
            }
            return Ok(());
        }
        if !matches!(kind, Kind::Page | Kind::Frame) {
            return Ok(());
        }
        let media = page.and_then(|page| self.emulation.page_media.get(&page));
        let throttling = page
            .and_then(|page| self.emulation.page_throttling.get(&page))
            .cloned()
            .unwrap_or_default();
        for (method, params) in frame_overrides(&options, kind == Kind::Page, media, &throttling) {
            let result = self.call(id, Some(session), method, params, deadline).await;
            // Pinned emulateLocale/emulateTimezone: an override already active
            // in a shared renderer is kept; an unknown timezone is refused.
            let kept = match (method, result.as_ref().err().and_then(remote_message)) {
                ("Emulation.setLocaleOverride", Some(message)) => {
                    message.contains("Another locale override is already in effect")
                }
                ("Emulation.setTimezoneOverride", Some(message))
                    if message.contains("Invalid timezone") =>
                {
                    return Err("INVALID_TIMEZONE".into());
                }
                ("Emulation.setTimezoneOverride", Some(message)) => {
                    message.contains("Timezone override is already in effect")
                }
                _ => false,
            };
            if !kept {
                result?;
            }
        }
        Ok(())
    }

    /// A service-worker update whose definite method failure the pinned SDK
    /// ignores; transport and outcome failures still propagate.
    async fn service_worker_call(
        &mut self,
        id: Uuid,
        session: &str,
        method: &str,
        params: Value,
        deadline: Instant,
    ) -> Result<(), Error> {
        match self.call(id, Some(session), method, params, deadline).await {
            Err(error) if error.code == "BROWSER_METHOD_FAILED" => Ok(()),
            result => result.map(|_| ()),
        }
    }

    /// Context-wide creation work after `Target.createBrowserContext`.
    pub(super) async fn initialize_context_emulation(
        &mut self,
        id: Uuid,
        context: Reference,
        options: &ContextOptions,
        deadline: Instant,
    ) -> Result<(), Error> {
        if let Some(permissions) = &options.permissions {
            self.grant_permissions(id, context, None, permissions, deadline)
                .await?;
        }
        Ok(())
    }

    /// Pinned `grantPermissions`: the origin's grants accumulate, Chrome then
    /// receives the whole set, retried once with the older local-network name.
    async fn grant_permissions(
        &mut self,
        id: Uuid,
        context: Reference,
        origin: Option<&str>,
        permissions: &[Permission],
        deadline: Instant,
    ) -> Result<(), Error> {
        settings::validate_permissions(permissions, Some(contract::Engine::Chrome))?;
        let native = self
            .index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        let origin = match origin {
            Some(origin) => Some(
                url::Url::parse(origin)
                    .map_err(|_| "INVALID_PERMISSION_ORIGIN")?
                    .origin()
                    .ascii_serialization(),
            ),
            None => None,
        };
        let key = origin.clone().unwrap_or_else(|| "*".into());
        let mut list = self
            .emulation
            .granted
            .get(&context)
            .and_then(|origins| origins.get(&key))
            .cloned()
            .unwrap_or_default();
        list.extend(permissions.iter().copied());
        let list = list.into_iter().collect::<Vec<_>>();
        self.emulation
            .granted
            .entry(context)
            .or_default()
            .insert(key, list.iter().copied().collect());
        let mut params =
            json!({"browserContextId":native,"permissions":settings::chromium_permissions(&list)});
        if let Some(origin) = &origin {
            params["origin"] = json!(origin);
        }
        match self
            .call(
                id,
                None,
                "Browser.grantPermissions",
                params.clone(),
                deadline,
            )
            .await
        {
            Err(error)
                if error.code == "BROWSER_METHOD_FAILED"
                    && list.contains(&Permission::LocalNetworkAccess) =>
            {
                params["permissions"] = json!(
                    list.iter()
                        .flat_map(|permission| match permission {
                            Permission::LocalNetworkAccess => &["localNetworkAccess"][..],
                            permission => permission.chromium().unwrap_or_default(),
                        })
                        .collect::<Vec<_>>()
                );
                self.call(id, None, "Browser.grantPermissions", params, deadline)
                    .await
                    .map(|_| ())
            }
            result => result.map(|_| ()),
        }
    }

    /// Native `context_configure` for offline, geolocation and permissions.
    pub(super) async fn configure_environment(
        &mut self,
        id: Uuid,
        context: Reference,
        setting: ContextSetting,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let native = self
            .index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        setting.validate()?;
        let requested = setting.summary();
        let failed = |completed: usize, selected: usize, mut error: Error| {
            error.partial = Some(
                json!({"context":context,"requested":requested,"configurationComplete":false,"completedAttachments":completed,"selectedAttachments":selected,"stateMayHaveChanged":true,"effectsObserved":false,"replayed":false,"restoration":"close_owned_context","cause":error.partial}),
            );
            error
        };
        // Each setter stores the context option before native awaits, as the
        // pinned SDK does, so future attachments receive the requested value.
        let (method, params, sessions) = match &setting {
            ContextSetting::Offline { offline } => {
                self.context_layout
                    .get_mut(&context)
                    .ok_or("INVALID_SCOPE")?
                    .offline = Some(*offline);
                (
                    "Network.emulateNetworkConditions",
                    network_conditions(*offline, None),
                    self.emulated_sessions(context, None, true),
                )
            }
            ContextSetting::Geolocation { geolocation } => {
                self.context_layout
                    .get_mut(&context)
                    .ok_or("INVALID_SCOPE")?
                    .geolocation
                    .clone_from(geolocation);
                (
                    "Emulation.setGeolocationOverride",
                    geolocation
                        .as_ref()
                        .map_or_else(|| json!({}), |location| json!(location)),
                    self.emulated_sessions(context, None, false),
                )
            }
            ContextSetting::PermissionsGrant {
                origin,
                permissions,
            } => {
                self.grant_permissions(id, context, Some(origin), permissions, deadline)
                    .await
                    .map_err(|error| failed(0, 0, error))?;
                return self.environment_configured(context, &setting, 0);
            }
            ContextSetting::PermissionsClear {} => {
                self.emulation.granted.remove(&context);
                self.call(
                    id,
                    None,
                    "Browser.resetPermissions",
                    json!({"browserContextId":native}),
                    deadline,
                )
                .await
                .map_err(|error| failed(0, 0, error))?;
                return self.environment_configured(context, &setting, 0);
            }
            ContextSetting::ExtraHttpHeaders { .. } | ContextSetting::HttpCredentials { .. } => {
                return Err("CAPABILITY_UNAVAILABLE".into());
            }
        };
        let mut completed = 0;
        for session in &sessions {
            self.environment_attachment(id, session, &setting, method, &params, deadline)
                .await
                .map_err(|error| failed(completed, sessions.len(), error))?;
            completed += 1;
        }
        self.environment_configured(context, &setting, completed)
    }

    /// One attachment's offline or geolocation update. Context offline keeps
    /// each page's throttling preset; service workers ignore method failures.
    async fn environment_attachment(
        &mut self,
        id: Uuid,
        session: &str,
        setting: &ContextSetting,
        method: &str,
        params: &Value,
        deadline: Instant,
    ) -> Result<(), Error> {
        let scope = self.index.diagnostic_scope(session);
        let service = scope.is_some_and(|(_, page, worker)| page.is_none() && worker.is_some());
        let ContextSetting::Offline { offline } = setting else {
            return self
                .call(id, Some(session), method, params.clone(), deadline)
                .await
                .map(|_| ());
        };
        let params = network_conditions(
            *offline,
            scope
                .and_then(|(_, page, _)| page)
                .and_then(|page| self.emulation.page_throttling.get(&page))
                .and_then(|throttling| throttling.network)
                .filter(|preset| *preset != NetworkPreset::None),
        );
        if service {
            self.service_worker_call(id, session, "Network.enable", json!({}), deadline)
                .await?;
            return self
                .service_worker_call(id, session, method, params, deadline)
                .await;
        }
        self.call(id, Some(session), "Network.enable", json!({}), deadline)
            .await?;
        self.call(id, Some(session), method, params, deadline)
            .await
            .map(|_| ())
    }

    fn environment_configured(
        &self,
        context: Reference,
        setting: &ContextSetting,
        completed: usize,
    ) -> Result<Value, Error> {
        let scope_ended = self.index.native(context, Kind::Context).is_err();
        Ok(
            json!({"context":context,"requested":setting.summary(),"backend":"chrome_cdp","dispatchCompleted":true,"effectsObserved":false,"configuredAttachments":completed,"futureAttachments":true,"scopeEnded":scope_ended,"restoration":"close_owned_context"}),
        )
    }

    /// Native `page_emulate_media`: merged page overrides over the context
    /// baseline, rolled back best effort on failure as the pinned SDK does.
    pub(super) async fn page_emulate_media(
        &mut self,
        id: Uuid,
        page: Reference,
        options: MediaOptions,
        deadline: Instant,
    ) -> Result<Value, Error> {
        options.validate()?;
        let context = self
            .index
            .retained_page_context(page)
            .ok_or("STALE_REFERENCE")?;
        let scope = self.page_storage_scope(id, page, None, deadline).await?;
        let previous = self.emulation.page_media.get(&page).cloned();
        let merged = previous.clone().unwrap_or_default().merged(&options);
        self.emulation.page_media.insert(page, merged);
        self.inputs.invalidate(page);
        let sessions = self.emulated_sessions(context, Some(page), false);
        let mut completed = 0;
        for session in &sessions {
            if let Err(mut error) = self
                .apply_page_media(id, context, page, session, deadline)
                .await
            {
                let restored = self
                    .restore_page_media(id, context, page, previous, &sessions, deadline)
                    .await;
                error.partial = Some(
                    json!({"page":page,"requested":options,"dispatchCompleted":false,"completedAttachments":completed,"selectedAttachments":sessions.len(),"rolledBack":true,"restoredAttachments":restored,"stateMayHaveChanged":restored != sessions.len(),"effectsObserved":false,"replayed":false,"restoration":"reset_media_overrides_or_close_owned_context","cause":error.partial}),
                );
                return Err(error);
            }
            completed += 1;
        }
        match self
            .page_expression(id, &scope, settings::MEDIA_OBSERVATION.into(), deadline)
            .await
        {
            Ok(observed) => Ok(
                json!({"page":page,"frame":scope.frame,"requested":options,"observed":observed,"dispatchCompleted":true,"configuredAttachments":completed,"source":"chrome-cdp-media-emulation","previewRecovery":{"required":false},"restoration":"reset_media_overrides_or_close_owned_context"}),
            ),
            Err(mut error) => {
                error.partial = Some(
                    json!({"page":page,"requested":options,"dispatchCompleted":true,"effectsObserved":false,"previewRecovery":{"required":false}}),
                );
                Err(error)
            }
        }
    }

    /// Pinned `emulateMedia` rollback: restores the previous page media and
    /// reapplies it best effort, returning how many attachments accepted it.
    async fn restore_page_media(
        &mut self,
        id: Uuid,
        context: Reference,
        page: Reference,
        previous: Option<MediaOptions>,
        sessions: &BTreeSet<String>,
        deadline: Instant,
    ) -> usize {
        match previous {
            Some(previous) => self.emulation.page_media.insert(page, previous),
            None => self.emulation.page_media.remove(&page),
        };
        let mut restored = 0;
        for session in sessions {
            if self
                .apply_page_media(id, context, page, session, deadline)
                .await
                .is_ok()
            {
                restored += 1;
            }
        }
        restored
    }

    /// Chrome-only page throttling on the page and its frames, like
    /// chrome-devtools-mcp `emulate`; context offline still applies.
    pub(super) async fn page_emulate_throttling(
        &mut self,
        id: Uuid,
        page: Reference,
        options: ThrottlingOptions,
        deadline: Instant,
    ) -> Result<Value, Error> {
        options.validate()?;
        let context = self
            .index
            .retained_page_context(page)
            .ok_or("STALE_REFERENCE")?;
        self.index
            .native(page, Kind::Page)
            .map_err(resource_error)?;
        let merged = self
            .emulation
            .page_throttling
            .get(&page)
            .cloned()
            .unwrap_or_default()
            .merged(&options);
        // Stored before native awaits so new frames receive the requested value.
        self.emulation.page_throttling.insert(page, merged.clone());
        let offline = self
            .context_layout
            .get(&context)
            .is_some_and(|layout| layout.offline == Some(true));
        let sessions = self.emulated_sessions(context, Some(page), false);
        let mut completed = 0;
        for session in &sessions {
            let applied = async {
                if let Some(preset) = options.network {
                    let preset = Some(preset).filter(|preset| *preset != NetworkPreset::None);
                    self.call(id, Some(session), "Network.enable", json!({}), deadline)
                        .await?;
                    self.call(
                        id,
                        Some(session),
                        "Network.emulateNetworkConditions",
                        network_conditions(offline, preset),
                        deadline,
                    )
                    .await?;
                }
                if let Some(rate) = options.cpu_rate {
                    self.call(
                        id,
                        Some(session),
                        "Emulation.setCPUThrottlingRate",
                        json!({"rate":rate}),
                        deadline,
                    )
                    .await?;
                }
                Ok::<(), Error>(())
            }
            .await;
            if let Err(mut error) = applied {
                error.partial = Some(
                    json!({"page":page,"requested":options,"configurationComplete":false,"completedAttachments":completed,"selectedAttachments":sessions.len(),"stateMayHaveChanged":true,"replayed":false,"restoration":"page_emulate_throttling-with-none-and-cpuRate-1-or-close-owned-page","cause":error.partial}),
                );
                return Err(error);
            }
            completed += 1;
        }
        Ok(
            json!({"page":page,"requested":options,"effective":merged,"contextOffline":offline,"backend":"chrome_cdp","dispatchCompleted":true,"effectsObserved":false,"configuredAttachments":completed,"futureAttachments":true,"restoration":"page_emulate_throttling-with-none-and-cpuRate-1-or-close-owned-page"}),
        )
    }

    async fn apply_page_media(
        &mut self,
        id: Uuid,
        context: Reference,
        page: Reference,
        session: &str,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let options = self
            .context_layout
            .get(&context)
            .cloned()
            .unwrap_or_default();
        let media = emulated_media(&options, self.emulation.page_media.get(&page));
        self.call(
            id,
            Some(session),
            "Emulation.setEmulatedMedia",
            media,
            deadline,
        )
        .await
    }

    /// Pinned `_updateViewport` metrics for an owned page.
    pub(super) fn device_metrics(
        options: &ContextOptions,
        viewport: contract::Viewport,
        screen: contract::Viewport,
    ) -> Value {
        let mobile = options.is_mobile == Some(true);
        let orientation = if mobile && screen.width <= screen.height {
            json!({"angle":0,"type":"portraitPrimary"})
        } else if mobile {
            json!({"angle":90,"type":"landscapePrimary"})
        } else {
            json!({"angle":0,"type":"landscapePrimary"})
        };
        json!({"mobile":mobile,"width":viewport.width,"height":viewport.height,"screenWidth":screen.width,"screenHeight":screen.height,"deviceScaleFactor":options.device_scale_factor,"screenOrientation":orientation})
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_metadata_matches_the_pinned_platform_rules() {
        let metadata = |agent: &str, mobile| {
            user_agent_metadata(&ContextOptions {
                user_agent: Some(agent.into()),
                is_mobile: Some(mobile),
                ..ContextOptions::default()
            })
            .unwrap()
        };
        assert_eq!(
            metadata("Mozilla/5.0 (Linux; Android 14.0.1.9; Pixel) Mobile", true),
            json!({"mobile":true,"model":"","architecture":"arm","platform":"Android","platformVersion":"14.0.1"})
        );
        assert_eq!(
            metadata(
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_4_1 like Mac OS X)",
                true
            )["platformVersion"],
            "17_4"
        );
        assert_eq!(
            metadata("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)", false),
            json!({"mobile":false,"model":"","architecture":"x86","platform":"macOS","platformVersion":"10_15_7"})
        );
        assert_eq!(
            metadata("Mozilla/5.0 (Windows NT 10.0; Win64; x64)", false)["platformVersion"],
            "10.0"
        );
        assert_eq!(
            metadata("Acme/1.0 (X11; Linux ARM)", false)["platform"],
            "Linux"
        );
        assert_eq!(
            metadata("Acme/1.0 (X11; Linux ARM)", false)["architecture"],
            "arm"
        );
        assert_eq!(
            metadata("Acme/1.0", false),
            json!({"mobile":false,"model":"","architecture":"x86","platform":"Windows","platformVersion":""})
        );
        assert!(user_agent_metadata(&ContextOptions::default()).is_none());
    }

    #[test]
    fn emulated_media_uses_the_context_baseline_and_page_overrides() {
        let context = ContextOptions {
            color_scheme: Some(settings::ColorScheme::Dark),
            ..ContextOptions::default()
        };
        assert_eq!(
            emulated_media(&ContextOptions::default(), None),
            json!({"media":"","features":[{"name":"prefers-color-scheme","value":"light"},{"name":"prefers-reduced-motion","value":"no-preference"},{"name":"forced-colors","value":"none"},{"name":"prefers-contrast","value":"no-preference"}]})
        );
        assert_eq!(
            emulated_media(&context, None)["features"][0]["value"],
            "dark"
        );
        let page = MediaOptions {
            media: Some(settings::Media::Print),
            color_scheme: Some(settings::ColorScheme::NoOverride),
            ..MediaOptions::default()
        };
        let media = emulated_media(&context, Some(&page));
        assert_eq!(media["media"], "print");
        assert_eq!(media["features"][0]["value"], "");
        let merged = page.merged(&MediaOptions {
            contrast: Some(settings::Contrast::More),
            ..MediaOptions::default()
        });
        assert_eq!(merged.media, Some(settings::Media::Print));
        assert_eq!(merged.contrast, Some(settings::Contrast::More));
    }

    #[test]
    fn device_metrics_follow_mobile_orientation_and_screen() {
        let options = ContextOptions {
            is_mobile: Some(true),
            device_scale_factor: 3.0,
            ..ContextOptions::default()
        };
        let viewport = contract::Viewport {
            width: 390,
            height: 664,
        };
        let screen = contract::Viewport {
            width: 390,
            height: 844,
        };
        assert_eq!(
            Worker::device_metrics(&options, viewport, screen),
            json!({"mobile":true,"width":390,"height":664,"screenWidth":390,"screenHeight":844,"deviceScaleFactor":3.0,"screenOrientation":{"angle":0,"type":"portraitPrimary"}})
        );
        let landscape = contract::Viewport {
            width: 844,
            height: 390,
        };
        assert_eq!(
            Worker::device_metrics(&options, landscape, landscape)["screenOrientation"]["angle"],
            90
        );
        assert_eq!(
            Worker::device_metrics(&ContextOptions::default(), viewport, viewport)["screenOrientation"]
                ["type"],
            "landscapePrimary"
        );
    }
}

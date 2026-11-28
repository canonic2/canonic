//! Request roles and routing: inspection without admission, use requests
//! admitted to one pooled target's FIFO queue, and `disconnect`. Every device
//! request resolves its target before admission: the connected target it
//! names, or the only connected target. A target that is not connected is
//! refused with `NOT_CONNECTED` unless the request can connect or temporarily
//! claim the target.
use super::{Kind, Service};
use crate::{
    error::{Error, Result},
    managed::{Caller, identity::Identity},
};
use serde_json::{Value, json};
use std::sync::Arc;

/// The intent of a request decides idle renewal and manager startup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    /// Device work and reads a client asked for: renews its target's idle
    /// timeout and starts an absent manager.
    Use,
    /// Status: no renewal, never starts the manager.
    Inspect,
}

pub(crate) fn role(name: &str) -> Role {
    if matches!(
        name,
        "status" | "control_status" | "browser_status" | "browser_control_status"
    ) {
        Role::Inspect
    } else {
        Role::Use
    }
}

/// The parsed parts of a `call` request.
struct Request {
    identity: Identity,
    kind: Kind,
    connection: Option<String>,
    name: String,
    args: Value,
    request_id: Option<String>,
    deadline: Option<tokio::time::Instant>,
    connect_if_absent: bool,
    /// The pooled target the request resolved to.
    target: Option<String>,
}

/// A fresh read can establish the default connection for an exact target.
/// Reads needing an inspection runner or earlier session evidence use a held target.
fn connects_for_read(request: &Request, target: &str) -> bool {
    let name = request.name.as_str();
    if name == "app_list" {
        return true;
    }
    // Existing MCP read approvals do not grant permission to connect.
    if request.kind == Kind::Mcp {
        return false;
    }
    let Some((platform, id)) = target.split_once(':') else {
        return false;
    };
    if id.is_empty() || target.starts_with(crate::android::emulator::AVD_PREFIX) {
        return false;
    }
    let args = &request.args;
    match (platform, name) {
        ("device", "observe") => args.get("profile").is_none_or(|profile| profile == "fast"),
        ("android" | "simulator", "observe" | "wait")
        | ("android", "media_status")
        | ("simulator", "clipboard_read") => true,
        ("android" | "simulator", "ui_snapshot" | "ui_find") => {
            args["scope"]["kind"] == "display" && args["cursor"].is_null()
        }
        ("android" | "simulator", "ui_read") => matches!(
            crate::ui::semantic::parse_target(&args["element"]),
            Ok(crate::ui::semantic::Target::Query { .. })
        ),
        ("android", "volume_get") => args["destination"]["kind"] == "music_stream",
        _ => false,
    }
}

pub(crate) fn not_connected(target: Option<&str>) -> Error {
    Error::new(
        "NOT_CONNECTED",
        match target {
            Some(_) => "That target is not connected; connect it first",
            None => "No device is connected; connect one first",
        },
    )
    .details(json!({"admitted":false,"dispatched":false,"targetId":target,"nextAction":"connect"}))
}

/// How a request without `targetId` resolves against the connected targets:
/// the only one, or a refusal before admission.
fn implicit_target(targets: &[String]) -> Result<String> {
    match targets {
        [target] => Ok(target.clone()),
        [] => Err(not_connected(None)),
        targets => Err(Error::new(
            "TARGET_REQUIRED",
            "Several targets are connected; name one with targetId",
        )
        .details(json!({"admitted":false,"dispatched":false,"targets":targets}))),
    }
}

/// Tools that already name their target through a preview or recording ID.
fn media_reference(name: &str, args: &Value) -> Option<String> {
    match name {
        "preview_status" | "preview_stop" => args["previewId"].as_str(),
        "recording_status" | "recording_stop" => args["recordingId"].as_str(),
        _ => None,
    }
    .map(str::to_owned)
}

fn parse(request: &Value) -> Result<Request> {
    let identity: Identity = serde_json::from_value(request["identity"].clone())
        .map_err(|_| Error::invalid("Invalid client identity"))?;
    let kind = match request["client"]["kind"].as_str() {
        Some("mcp") => Kind::Mcp,
        Some("cli") | None => Kind::Cli,
        Some(_) => return Err(Error::invalid("Unknown client kind")),
    };
    let name = request["name"]
        .as_str()
        .ok_or_else(|| Error::invalid("A call names its tool"))?
        .to_owned();
    Ok(Request {
        identity,
        kind,
        connection: request["client"]["connection"].as_str().map(str::to_owned),
        name,
        args: request["args"].clone(),
        request_id: request["requestId"].as_str().map(str::to_owned),
        deadline: request["deadlineMs"].as_u64().map(|remaining| {
            tokio::time::Instant::now() + std::time::Duration::from_millis(remaining)
        }),
        connect_if_absent: false,
        target: None,
    })
}

/// Pins an admitted request to the pool and releases its pin, operation and
/// temporary claim however the request ends, including abandonment.
struct Admitted {
    service: Arc<Service>,
    pinned: bool,
    operation: Option<String>,
    temporary: Option<String>,
}
impl Drop for Admitted {
    fn drop(&mut self) {
        {
            let mut registry = self.service.registry();
            if self.pinned {
                registry.finish();
            }
            if let Some(operation) = &self.operation {
                registry.end_operation(operation);
            }
            if let Some(target) = &self.temporary {
                registry.settle_temporary(target);
            }
        }
        self.service.prune();
        self.service.notify();
    }
}

impl Service {
    pub(super) async fn call(self: &Arc<Self>, request: Value) -> Result<Value> {
        if crate::diagnostics::handles(request["name"].as_str().unwrap_or_default()) {
            return self.diagnostic_call(&request).await;
        }
        if request["name"] == "project_status" {
            let mut result = crate::project::status(&request["args"]).await?;
            let identity: Identity = serde_json::from_value(request["identity"].clone())
                .map_err(|_| Error::invalid("Invalid client identity"))?;
            result["activeOperations"] = self.builds.active_project(
                &identity.workspace_key(),
                result
                    .get("applicationRoot")
                    .unwrap_or(&result["project"])
                    .as_str()
                    .unwrap(),
            );
            result["recentOperations"] = self.builds.recent_project(
                &identity.workspace_key(),
                result
                    .get("applicationRoot")
                    .unwrap_or(&result["project"])
                    .as_str()
                    .unwrap(),
            );
            return Ok(result);
        }
        if request["name"] == "project_open" {
            let handoff = crate::project::handoff(&request["args"]).await?;
            let mut call = request.clone();
            call["name"] = json!("act");
            call["args"] = json!({"targetId":request["args"]["targetId"],"actions":[{"action":"app_launch","appId":handoff["appId"]},{"action":"open_url","url":handoff["url"]}]});
            return Box::pin(self.call(call)).await;
        }
        if crate::builds::handles(request["name"].as_str().unwrap_or_default()) {
            return self.build_call(request).await;
        }
        let mut request = request;
        if request["name"] == "app_install" && request["args"]["artifactRef"].is_string() {
            let identity: Identity = serde_json::from_value(request["identity"].clone())
                .map_err(|_| Error::invalid("Invalid client identity"))?;
            let artifact = self.builds.artifact(
                &self.root,
                &identity.workspace_key(),
                request["args"]["artifactRef"].as_str().unwrap(),
            )?;
            if request["args"].get("path").is_some() {
                return Err(Error::invalid("Select artifactRef or path, not both"));
            }
            request["args"]
                .as_object_mut()
                .unwrap()
                .remove("artifactRef");
            request["args"]["path"] = artifact["path"].clone();
            request["args"]["expectedSha256"] = artifact["sha256"].clone();
        }
        let mut request = parse(&request)?;
        if role(&request.name) == Role::Inspect {
            return self.inspect(request).await;
        }
        if matches!(request.name.as_str(), "disconnect" | "browser_disconnect") {
            return self.disconnect(request).await;
        }
        if matches!(
            request.name.as_str(),
            "developer_mode_reveal" | "developer_support_install"
        ) {
            let target = request.args["targetId"]
                .as_str()
                .ok_or_else(|| Error::invalid("Name the device with targetId"))?
                .to_owned();
            return self.explicit_target(request, &target).await;
        }
        if crate::browser::surface::handles(&request.name) {
            return self.admit_and_run(request, None).await;
        }
        if request.name == "connect" {
            // A target a pooled device became, such as a started emulator,
            // names that device.
            request.target = request.args["targetId"].as_str().map(|target| {
                self.held_target(target)
                    .unwrap_or_else(|| target.to_owned())
            });
            return self.admit_and_run(request, None).await;
        }
        if let Some(target) = request.args["targetId"].as_str().map(str::to_owned) {
            return self.explicit_target(request, &target).await;
        }
        request.target = Some(self.implicit(&request)?);
        self.admit_and_run(request, None).await
    }

    /// The target of a device request without `targetId`: the device running
    /// the preview or recording it names, or the only connected device. With
    /// none connected, the most recent connection is used only when a manager
    /// loss interrupted it, so its records answer earlier requests; a
    /// disconnected target is not resolved.
    fn implicit(&self, request: &Request) -> Result<String> {
        let devices = {
            let registry = self.registry();
            let pool = registry.pool();
            if let (Some(pool), Some(media)) = (pool, media_reference(&request.name, &request.args))
                && let Some(target) = pool.media_targets.get(&media)
            {
                return Ok(target.clone());
            }
            pool.map(super::registry::Pool::devices).unwrap_or_default()
        };
        if devices.is_empty()
            && let Some(target) = crate::managed::selection::pool_directory(
                &self.root,
                crate::managed::identity::POOL,
            )
            .ok()
            .and_then(|records| crate::managed::selection::interrupted_target(&records))
        {
            return Ok(target);
        }
        implicit_target(&devices)
    }

    /// Admit `request` to the pool, opening it if needed in the same step,
    /// and run it in its target's queue. `temporary` is a claim the request
    /// settles when it ends.
    async fn admit_and_run(
        self: &Arc<Self>,
        request: Request,
        temporary: Option<String>,
    ) -> Result<Value> {
        let host = request.identity.label.clone();
        let operation = super::super::operation(&request.name).then(|| {
            request
                .request_id
                .clone()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
        });
        let kind = request.kind;
        let target = request.target.clone();
        let admitted = self.open_then(|registry| {
            registry.admit(target.as_deref())?;
            if let Some(operation) = &operation {
                registry.begin_operation(
                    operation,
                    &request.name,
                    host.as_deref(),
                    kind,
                    target.as_deref(),
                );
            }
            Ok(())
        });
        let _admitted = Admitted {
            service: self.clone(),
            pinned: admitted.is_ok(),
            operation: admitted.is_ok().then_some(operation).flatten(),
            temporary,
        };
        admitted?;
        self.notify();
        let name = request.name.clone();
        let reservation = self.reserve_media(&name)?;
        let stopping = stopped_media(&name, &request.args);
        let viewer = held_input(&name, &request.args);
        let mut caller = self
            .caller(kind)
            .ok_or_else(|| Error::new("OWNER_EXITING", "The manager has no device pool"))?;
        caller.connection_id = request.connection;
        caller.host.clone_from(&host);
        caller.request_id = request.request_id;
        caller.media_deadline = request.deadline;
        caller.target.clone_from(&request.target);
        caller.device_set = request.identity.device_set;
        let result = if request.connect_if_absent {
            caller.connect_if_absent(request.args).await
        } else {
            caller.call(&name, request.args).await
        };
        let target = caller.target.clone();
        self.account_media(
            &name,
            reservation.as_deref(),
            stopping,
            target.as_deref(),
            &result,
        );
        if let (Some(holding), Some(target), Ok(_)) = (viewer, target.as_deref(), &result) {
            self.viewer(target, holding);
        }
        result
    }

    /// Previews and recordings count against manager-wide limits: a start
    /// reserves its slot before it runs and gets the reservation's ID.
    fn reserve_media(&self, name: &str) -> Result<Option<String>> {
        let media = match name {
            "preview_start" => super::registry::Media::Preview,
            "recording_start" => super::registry::Media::Recording,
            _ => return Ok(None),
        };
        let reservation = format!("starting-{}", uuid::Uuid::new_v4());
        self.registry().reserve_media(media, &reservation)?;
        Ok(Some(reservation))
    }

    /// Record what a media request did to the manager-wide preview and
    /// recording counts: a start replaces its reservation with the new ID; a
    /// stop, or a status showing the recording finished, releases it.
    fn account_media(
        &self,
        name: &str,
        reservation: Option<&str>,
        stopping: Option<String>,
        target: Option<&str>,
        result: &Result<Value>,
    ) {
        let mut registry = self.registry();
        if let Some(reservation) = reservation {
            let started = result.as_ref().ok().and_then(|value| {
                value["previewId"]
                    .as_str()
                    .or_else(|| value["recordingId"].as_str())
            });
            registry.settle_media(reservation, started, target);
        }
        if let (Some(media), Ok(value)) = (stopping, result)
            && (name != "recording_status" || value["status"] != "active")
        {
            registry.release_media(&media);
        }
    }

    /// Status and control status: answered from the pool when the manager
    /// has one, otherwise from durable records, with nothing admitted. With
    /// `targetId` they describe that target; status without one describes
    /// every connected target.
    async fn inspect(self: &Arc<Self>, mut request: Request) -> Result<Value> {
        let _slot = self.admission.inspect()?;
        let mut caller = self
            .caller(request.kind)
            .unwrap_or_else(|| Caller::new(Some(self.clone()), self.root.clone(), request.kind));
        caller.request_id.clone_from(&request.request_id);
        if !crate::browser::surface::handles(&request.name) {
            caller.target = match take_target(&mut request.args) {
                Some(target) => Some(self.inspected_target(&caller, &target)?),
                None if request.name == "control_status" => Some(self.implicit(&request)?),
                None => None,
            };
        }
        caller.call(&request.name, request.args).await
    }

    /// The target an inspection names: in the pool, or recorded as open when
    /// the manager has no pool to describe it.
    fn inspected_target(&self, caller: &Caller, target: &str) -> Result<String> {
        if let Some(held) = self.held_target(target) {
            return Ok(held);
        }
        if caller.pool.is_none() && caller.recorded(target) {
            return Ok(target.to_owned());
        }
        Err(not_connected(Some(target)))
    }

    /// Release one target for every client once the requests ahead of it in
    /// its queue settle: the named target, or the only connected one.
    async fn disconnect(self: &Arc<Self>, mut request: Request) -> Result<Value> {
        let target = take_target(&mut request.args);
        if request.name == "browser_disconnect"
            || target
                .as_deref()
                .is_some_and(|target| target.starts_with(crate::browser::runtime::TARGET_PREFIX))
        {
            return self
                .disconnect_browser(target, request.kind, request.request_id)
                .await;
        }
        let mut caller = self
            .caller(request.kind)
            .unwrap_or_else(|| Caller::new(Some(self.clone()), self.root.clone(), request.kind));
        caller.request_id.clone_from(&request.request_id);
        if caller.pool.is_none() {
            // No pool: settle what durable records show still open.
            caller.target = match target {
                Some(target) if caller.recorded(&target) => Some(target),
                Some(target) => {
                    return Ok(json!({"state":"closed","closed":true,"targetId":target}));
                }
                None => None,
            };
            return caller.call("disconnect", json!({})).await;
        }
        let target = match target {
            Some(target) => match self.held_target(&target) {
                Some(held) => held,
                // A target that is not connected has nothing to release.
                None => return Ok(json!({"state":"closed","closed":true,"targetId":target})),
            },
            None => match implicit_target(&self.devices()) {
                Ok(target) => target,
                Err(error) if error.code == "NOT_CONNECTED" => {
                    return Ok(json!({"state":"closed","closed":true}));
                }
                Err(error) => return Err(error),
            },
        };
        self.release(&target, request.kind, request.request_id)
            .await
    }

    /// Resolve and admit a close for the named browser or the only connected one.
    async fn disconnect_browser(
        self: &Arc<Self>,
        target: Option<String>,
        kind: Kind,
        request_id: Option<String>,
    ) -> Result<Value> {
        let Some(browsers) = self.registry().pool().map(|pool| pool.browsers()) else {
            return Ok(json!({"state":"closed","closed":true}));
        };
        let target = match target {
            Some(target) => target,
            None if browsers.is_empty() => return Ok(json!({"state":"closed","closed":true})),
            None => implicit_target(&browsers)?,
        };
        crate::browser::runtime::connection(&target)?;
        self.release(&target, kind, request_id).await
    }

    /// The manager's release attempt owns both browser cleanup and retirement.
    pub(super) async fn release_browser(&self, target: &str) -> Result<Value> {
        let Some((pool, directory)) = self
            .registry()
            .pool()
            .map(|pool| (pool.id.clone(), pool.directory.clone()))
        else {
            return Ok(json!({"state":"closed","closed":true}));
        };
        let connection = crate::browser::runtime::connection(target)?;
        let receipt = self.browsers.close_connection(&pool, connection).await?;
        self.release_target(target);
        let mut result = json!({"state":"closed","closed":true,"targetId":target});
        if let Some(cleanup) = receipt.get("cleanup") {
            result["cleanup"] = cleanup.clone();
        }
        self.with_browser_record(&pool, &directory, Ok(result))
    }

    /// A call naming a target: it joins that target's queue when the target
    /// is connected. Fresh reads can connect a named target when the default
    /// connection supports the read. App installs and device preparation can
    /// claim a target for one call.
    async fn explicit_target(
        self: &Arc<Self>,
        mut request: Request,
        target: &str,
    ) -> Result<Value> {
        let keeps_target = matches!(
            request.name.as_str(),
            "app_install" | "app_uninstall" | "developer_mode_reveal" | "developer_support_install"
        );
        let name = request.name.clone();
        let mut selected = self.held_target(target);
        if selected.is_none() && connects_for_read(&request, target) {
            if name == "wait" {
                let (platform, _) = target.split_once(':').expect("Named wait target");
                crate::ui::semantic::parse_wait(platform, &request.args)?;
            }
            let connect = Request {
                identity: request.identity.clone(),
                kind: request.kind,
                connection: request.connection.clone(),
                name: "connect".into(),
                args: json!({"targetId":target}),
                request_id: None,
                deadline: request.deadline,
                connect_if_absent: true,
                target: Some(target.to_owned()),
            };
            let ready = self.admit_and_run(connect, None).await?;
            // Reuse the first capture when it satisfies the request. Only
            // a new connection reports the profile used for that capture.
            if name == "observe"
                && ready["observation"].is_object()
                && ready["observation"]["observationError"].is_null()
                && request
                    .args
                    .get("profile")
                    .is_none_or(|profile| ready["reused"] == false && *profile == ready["profile"])
            {
                return Ok(compatible(&name, ready));
            }
            selected = Some(
                self.held_target(target)
                    .unwrap_or_else(|| target.to_owned()),
            );
        }
        let (selected, temporary) = match selected {
            Some(held) => (held, None),
            None if keeps_target => {
                let claimed = self.open_then(|registry| registry.claim_temporary(target))?;
                (claimed.clone(), Some(claimed))
            }
            None => return Err(not_connected(Some(target))),
        };
        if !keeps_target {
            take_target(&mut request.args);
        }
        request.target = Some(selected);
        self.admit_and_run(request, temporary)
            .await
            .map(|value| compatible(&name, value))
    }

    /// Host task end: the manager holds nothing for a host. Requests its
    /// clients abandoned are cancelled when their connections close.
    pub(crate) fn task_end(&self, request: &Value) -> Value {
        let host = request["hostLabel"]
            .as_str()
            .filter(|host| !host.is_empty());
        json!({"hostLabel":host,"released":[],"message":"Devices stay connected for other clients; a host's undispatched requests end with its connections"})
    }
}

/// Remove and return a request's `targetId`: the service resolves it, and
/// the workflows behind it take no such field.
fn take_target(args: &mut Value) -> Option<String> {
    match args.as_object_mut()?.remove("targetId")? {
        Value::String(target) => Some(target),
        _ => None,
    }
}

/// The preview or recording a request may stop.
fn stopped_media(name: &str, args: &Value) -> Option<String> {
    let id = match name {
        "preview_stop" => &args["previewId"],
        "recording_stop" | "recording_status" => &args["recordingId"],
        _ => return None,
    };
    id.as_str().map(str::to_owned)
}

/// Whether a successful request leaves a viewer holding its target's input:
/// a viewer acquisition does, an agent acquisition or a release does not.
fn held_input(name: &str, args: &Value) -> Option<bool> {
    match name {
        "control_acquire" => Some(args["owner"] == "viewer"),
        "control_release" => Some(false),
        _ => None,
    }
}

/// Results of explicit-target commands keep their raw shape: the observation
/// for `observe` and the batch result for `act`.
fn compatible(name: &str, mut value: Value) -> Value {
    match name {
        "observe" if value.get("observation").is_some() => value["observation"].take(),
        "act" if value.get("result").is_some() => value["result"].take(),
        _ => value,
    }
}

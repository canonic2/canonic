//! Same-user viewer IPC, private grants and bounded forwarding to the existing input owner.
use super::{Authority, Credential};
#[cfg(unix)]
use crate::interactive::local::{Endpoint, check_owner, request};
use crate::{
    error::{Error, Result},
    grants::{Grants, consume_grant, issue_grant},
};
use futures_util::{StreamExt, future::BoxFuture, stream::FuturesUnordered};
use serde::Deserialize;
use serde_json::{Value, json};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::{
    future::Future,
    io,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
#[cfg(unix)]
use tokio::net::UnixListener;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc,
    task::JoinHandle,
};

#[cfg(windows)]
mod windows;

#[cfg(unix)]
#[derive(Clone)]
pub(crate) struct Proxy {
    pub(crate) socket: PathBuf,
    pub(crate) epoch: Option<String>,
}
pub(crate) type Handler = Arc<dyn Fn(Value) -> BoxFuture<'static, Result<Value>> + Send + Sync>;

#[derive(Clone)]
enum Route {
    #[cfg(unix)]
    Mobile(Proxy),
    Browser {
        channel_id: String,
        handler: Handler,
    },
}

fn current_grant(
    authority: &Authority,
    credential: &Credential,
    channel_id: &str,
    requested: &str,
    grants: &Grants,
) -> Result<Value> {
    let status = authority.status(Instant::now());
    if requested != channel_id
        || authority.credential(Instant::now()) != *credential
        || status["owner"] != "viewer"
        || !matches!(status["state"].as_str(), Some("viewer_pending" | "active"))
    {
        return Err(Error::new(
            "STALE_CONTROL_LEASE",
            "Viewer grant is no longer current",
        ));
    }
    if status["state"] == "active" {
        return Err(Error::new("TARGET_BUSY", "Viewer is already connected"));
    }
    Ok(
        json!({"token":issue_grant(grants)?,"access":"input_control","expiresInMs":30000,"singleUse":true}),
    )
}

fn pending_refusal(command_id: Value, error: Error) -> Value {
    json!({"kind":"result","commandId":command_id,"result":{"ok":false,"error":error.details(json!({"inputDispatched":false,"nextAction":"control_status"}))}})
}

fn admit_message(
    authority: &Authority,
    credential: &Credential,
    command_id: &str,
    responses: &mpsc::Sender<Value>,
) -> Result<bool> {
    match authority.admit(&credential.id, Instant::now()) {
        Ok(_) => Ok(true),
        Err(error) if error.code == "CONTROL_HANDOFF_PENDING" => {
            responses
                .try_send(pending_refusal(json!(command_id), error))
                .map_err(|_| Error::new("OVERLOADED", "Viewer is not reading replies"))?;
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

pub(crate) struct Channel {
    authority: Arc<Authority>,
    credential: Credential,
    channel_id: String,
    grants: Grants,
    endpoint: PathBuf,
    broker: PathBuf,
    browser: bool,
    worker: Mutex<Option<JoinHandle<()>>>,
}
impl Channel {
    #[cfg(unix)]
    pub(crate) fn start(
        authority: Arc<Authority>,
        credential: Credential,
        proxy: Proxy,
    ) -> Result<Arc<Self>> {
        Self::start_route(authority, credential, Route::Mobile(proxy))
    }
    /// Browser input still uses this shared authenticated channel and authority;
    /// only validation/dispatch behind the transport belongs to its input owner.
    pub(crate) async fn start_browser(
        authority: Arc<Authority>,
        credential: Credential,
        handler: Handler,
    ) -> Result<Arc<Self>> {
        let route = Route::Browser {
            channel_id: uuid::Uuid::new_v4().to_string(),
            handler,
        };
        #[cfg(unix)]
        return Self::start_route(authority, credential, route);
        #[cfg(windows)]
        Self::start_windows(authority, credential, route).await
    }
    #[cfg(unix)]
    fn start_route(
        authority: Arc<Authority>,
        credential: Credential,
        route: Route,
    ) -> Result<Arc<Self>> {
        let directory = PathBuf::from("/tmp")
            .join(format!("testrun-control-{}", uuid::Uuid::new_v4().simple()));
        crate::artifacts::directory(&directory)?;
        let mut endpoint = Endpoint {
            socket: directory.join("viewer.sock"),
            directory,
            identity: None,
        };
        let listener = UnixListener::bind(&endpoint.socket)
            .map_err(|_| Error::new("HOST_IO", "Cannot bind private viewer channel"))?;
        std::fs::set_permissions(&endpoint.socket, std::fs::Permissions::from_mode(0o600))
            .map_err(|_| Error::new("HOST_IO", "Cannot restrict viewer channel"))?;
        let identity = std::fs::symlink_metadata(&endpoint.socket)
            .map_err(|_| Error::new("HOST_IO", "Cannot identify viewer channel"))?;
        endpoint.identity = Some((identity.dev(), identity.ino()));
        let path = endpoint.socket.clone();
        let grants = Grants::default();
        let worker = {
            let authority = authority.clone();
            let credential = credential.clone();
            let grants = grants.clone();
            let route = route.clone();
            tokio::spawn(async move {
                serve(
                    || async {
                        let (socket, _) = listener.accept().await?;
                        Ok(check_owner(&socket).is_ok().then_some(socket))
                    },
                    endpoint.lost(),
                    authority,
                    credential,
                    grants,
                    route,
                )
                .await;
            })
        };
        Ok(Self::owned(
            authority, credential, route, grants, path, worker,
        ))
    }

    #[cfg(windows)]
    async fn start_windows(
        authority: Arc<Authority>,
        credential: Credential,
        route: Route,
    ) -> Result<Arc<Self>> {
        let unavailable = || Error::new("HOST_IO", "Cannot bind private viewer channel");
        let root = crate::windows_ipc::ScratchRoot::create().map_err(|_| unavailable())?;
        let lease = root.acquire().map_err(|_| unavailable())?;
        let path = root.path();
        let grants = Grants::default();
        let (ready, bound) = tokio::sync::oneshot::channel();
        let worker = {
            let authority = authority.clone();
            let credential = credential.clone();
            let grants = grants.clone();
            let route = route.clone();
            tokio::spawn(async move {
                let _root = root;
                let lease = lease;
                let Ok(listener) = lease.bind(false) else {
                    return;
                };
                if ready.send(()).is_ok() {
                    serve(
                        || windows::accept(&listener),
                        windows::lost(&listener),
                        authority,
                        credential,
                        grants,
                        route,
                    )
                    .await;
                }
            })
        };
        let channel = Self::owned(authority, credential, route, grants, path, worker);
        bound.await.map_err(|_| unavailable())?;
        Ok(channel)
    }

    fn owned(
        authority: Arc<Authority>,
        credential: Credential,
        route: Route,
        grants: Grants,
        endpoint: PathBuf,
        worker: JoinHandle<()>,
    ) -> Arc<Self> {
        let (channel_id, broker, browser) = match route {
            #[cfg(unix)]
            Route::Mobile(proxy) => (uuid::Uuid::new_v4().to_string(), proxy.socket, false),
            Route::Browser { channel_id, .. } => (channel_id, endpoint.clone(), true),
        };
        Arc::new(Self {
            authority,
            credential,
            channel_id,
            grants,
            endpoint,
            broker,
            browser,
            worker: Mutex::new(Some(worker)),
        })
    }
    pub(crate) fn metadata(&self) -> Value {
        json!({"protocol":"testrun_viewer_ipc_v1","controlSocketPath":self.endpoint,
            "grantBrokerPath":self.broker,"channelId":self.channel_id,"controlVersion":self.credential.version,
            "maxQueuedActions":8,"maxMessageBytes":1024*1024,"heartbeatTimeoutMs":5000,"heartbeatIntervalMs":1000,
            "authentication":"same_user_private_single_use_grant","inputScope":if self.browser {"browser-page"} else {"mobile-display"}})
    }
    #[cfg(unix)]
    pub(crate) fn grant(&self, id: &str) -> Result<Value> {
        current_grant(
            &self.authority,
            &self.credential,
            &self.channel_id,
            id,
            &self.grants,
        )
    }
    pub(crate) async fn close(&self) {
        self.authority.disconnect(&self.credential);
        self.grants
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        let worker = self.worker.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(worker) = worker {
            worker.abort();
            let _ = worker.await;
        }
    }
}
impl Drop for Channel {
    fn drop(&mut self) {
        self.authority.disconnect(&self.credential);
        self.grants
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        if let Some(worker) = self
            .worker
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            worker.abort();
        }
    }
}

async fn serve<S, F, L>(
    accept: impl Fn() -> F,
    lost: L,
    authority: Arc<Authority>,
    credential: Credential,
    grants: Grants,
    route: Route,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    F: Future<Output = io::Result<Option<S>>>,
    L: Future,
{
    let mut peers: FuturesUnordered<BoxFuture<'static, ()>> = FuturesUnordered::new();
    tokio::pin!(lost);
    loop {
        tokio::select! {
            () = authority.lost(&credential) => break,
            _ = &mut lost => break,
            Some(()) = peers.next(), if !peers.is_empty() => {},
            accepted = accept() => {
                let Ok(socket) = accepted else { break; };
                let Some(socket) = socket else { continue; };
                if peers.len() >= 8 { drop(socket); continue; }
                peers.push(Box::pin(peer(socket, authority.clone(), credential.clone(), grants.clone(), route.clone())));
            }
        }
    }
    authority.disconnect(&credential);
    grants.lock().unwrap_or_else(|e| e.into_inner()).clear();
}
struct Connected {
    authority: Arc<Authority>,
    credential: Credential,
}
impl Drop for Connected {
    fn drop(&mut self) {
        self.authority.disconnect(&self.credential);
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Input {
    Heartbeat,
    #[cfg(unix)]
    Act {
        #[serde(rename = "commandId")]
        command_id: String,
        display: crate::ui::Display,
        actions: Value,
    },
    #[serde(rename = "browser_input")]
    Browser {
        #[serde(rename = "commandId")]
        command_id: String,
        page: Value,
        observation: Option<Value>,
        actions: Value,
    },
}
async fn authenticate(
    socket: &mut (impl AsyncRead + AsyncWrite + Unpin),
    authority: &Authority,
    credential: &Credential,
    grants: &Grants,
    route: &Route,
) -> Result<bool> {
    let auth = tokio::time::timeout(Duration::from_secs(3), read_frame(socket))
        .await
        .map_err(|_| Error::new("UNAUTHORIZED", "Viewer authentication timed out"))??;
    if let Route::Browser { channel_id, .. } = route
        && auth["kind"] == "control_grant"
    {
        if auth.as_object().is_none_or(|fields| fields.len() != 2) {
            return Err(Error::new(
                "STALE_CONTROL_LEASE",
                "Read control status before requesting a private grant",
            ));
        }
        let grant = current_grant(
            authority,
            credential,
            channel_id,
            auth["channelId"].as_str().unwrap_or(""),
            grants,
        )?;
        tokio::time::timeout(Duration::from_secs(2), write_frame(socket, &grant))
            .await
            .map_err(|_| Error::new("TRANSPORT_IO", "Timed out returning the viewer grant"))??;
        return Ok(false);
    }
    let valid = auth.as_object().is_some_and(|object| object.len() == 1)
        && auth["token"]
            .as_str()
            .is_some_and(|token| token.len() == 64 && consume_grant(grants, token));
    if !valid {
        return Err(Error::new(
            "UNAUTHORIZED",
            "Viewer grant is invalid, expired or already used",
        ));
    }
    authority.connect(credential, Instant::now())?;
    Ok(true)
}

async fn peer(
    mut socket: impl AsyncRead + AsyncWrite + Unpin,
    authority: Arc<Authority>,
    credential: Credential,
    grants: Grants,
    route: Route,
) {
    let result = authenticate(&mut socket, &authority, &credential, &grants, &route).await;
    if matches!(result, Ok(false)) {
        return;
    }
    if let Err(error) = result {
        // A failed frame write can leave a partial response on the stream.
        if error.code != "TRANSPORT_IO" {
            let _ = tokio::time::timeout(
                Duration::from_secs(1),
                write_frame(&mut socket, &json!({"ok":false,"error":error})),
            )
            .await;
        }
        return;
    }
    let _connected = Connected {
        authority: authority.clone(),
        credential: credential.clone(),
    };
    if !matches!(tokio::time::timeout(Duration::from_secs(2), write_frame(&mut socket, &json!({"kind":"ready","controlVersion":credential.version,"heartbeatTimeoutMs":5000}))).await,Ok(Ok(()))) { return; }
    let (mut reader, mut writer) = tokio::io::split(socket);
    let (commands, mut pending) = mpsc::channel::<Value>(8);
    let (responses, mut outgoing) = mpsc::channel::<Value>(32);
    let reading = read_commands(
        &mut reader,
        &authority,
        &credential,
        &commands,
        &responses,
        matches!(route, Route::Browser { .. }),
    );
    let forwarding = async {
        while let Some(command) = pending.recv().await {
            let command_id = command["commandId"].clone();
            if let Err(error) = authority.admit(&credential.id, Instant::now()) {
                if error.code != "CONTROL_HANDOFF_PENDING" {
                    return Err(error);
                }
                responses
                    .send(pending_refusal(command_id, error))
                    .await
                    .map_err(|_| Error::new("TRANSPORT_IO", "Viewer replies ended"))?;
                continue;
            }
            let result = match &route {
                #[cfg(unix)]
                Route::Mobile(proxy) => {
                    let mut command = command;
                    if let Some(epoch) = &proxy.epoch {
                        command["ownerEpoch"] = json!(epoch);
                    }
                    request(&proxy.socket, &command).await.map_err(|error|Error::new("OUTCOME_UNKNOWN", "Viewer action reply was interrupted; inspect the command journal without replaying").details(json!({"cause":error.code})))
                }
                Route::Browser { handler, .. } => Ok::<_, Error>(match handler(command).await {
                    Ok(value) => json!({"ok":true,"data":value}),
                    Err(error) => json!({"ok":false,"error":error}),
                }),
            }?;
            responses.send(json!({"kind":"result","commandId":command_id,"result":crate::managed::clean_result(result)})).await
                .map_err(|_|Error::new("TRANSPORT_IO", "Viewer replies ended"))?;
        }
        Ok::<(), Error>(())
    };
    let writing = async {
        while let Some(response) = outgoing.recv().await {
            tokio::time::timeout(Duration::from_secs(2), write_frame(&mut writer, &response))
                .await
                .map_err(|_| Error::new("TIMEOUT", "Viewer stopped reading replies"))??;
        }
        Ok::<(), Error>(())
    };
    tokio::select! { _ = reading => {}, _ = forwarding => {}, _ = writing => {}, () = authority.lost(&credential) => {} }
}
async fn read_commands(
    reader: &mut (impl AsyncRead + Unpin),
    authority: &Authority,
    credential: &Credential,
    commands: &mpsc::Sender<Value>,
    responses: &mpsc::Sender<Value>,
    browser: bool,
) -> Result<()> {
    loop {
        let message = read_frame(reader).await?;
        let message: Input = serde_json::from_value(message)
            .map_err(|_| Error::invalid("Invalid viewer message"))?;
        match message {
            Input::Heartbeat => {
                authority.heartbeat(credential, Instant::now())?;
                responses
                    .try_send(json!({"kind":"heartbeat","ok":true}))
                    .map_err(|_| Error::new("OVERLOADED", "Viewer is not reading replies"))?;
            }
            #[cfg(unix)]
            Input::Act {
                command_id,
                display,
                actions,
            } => {
                if browser {
                    return Err(Error::invalid(
                        "Use browser_input with an explicit page and fresh observation",
                    ));
                }
                if uuid::Uuid::parse_str(&command_id).is_err() {
                    return Err(Error::invalid("Viewer commandId must be a UUID"));
                }
                if !admit_message(authority, credential, &command_id, responses)? {
                    continue;
                }
                let steps = crate::managed::action_steps(&json!({"actions":actions}))?;
                commands.try_send(json!({"kind":"viewer_batch","viewerLeaseId":credential.id,"viewerVersion":credential.version,
                        "commandId":command_id,"display":{"displayId":display.display_id,"geometryVersion":display.geometry_version},"steps":steps})).map_err(|_|Error::new("OVERLOADED", "Viewer input queue is full"))?;
            }
            Input::Browser {
                command_id,
                page,
                observation,
                actions,
            } => {
                if !browser || uuid::Uuid::parse_str(&command_id).is_err() {
                    return Err(Error::invalid(
                        "browser_input requires a browser channel and UUID commandId",
                    ));
                }
                if !admit_message(authority, credential, &command_id, responses)? {
                    continue;
                }
                commands.try_send(json!({"kind":"browser_input","viewerLeaseId":credential.id,"viewerVersion":credential.version,"commandId":command_id,"page":page,"observation":observation,"actions":actions}))
                    .map_err(|_|Error::new("OVERLOADED", "Viewer input queue is full"))?;
            }
        }
    }
}

pub(crate) async fn read_frame(reader: &mut (impl AsyncRead + Unpin)) -> Result<Value> {
    use tokio::io::AsyncReadExt;
    let length = reader
        .read_u32()
        .await
        .map_err(|_| Error::new("TRANSPORT_IO", "Viewer disconnected"))? as usize;
    if length == 0 || length > 1024 * 1024 {
        return Err(Error::invalid("Viewer message exceeds 1 MiB"));
    }
    let mut bytes = vec![0; length];
    reader
        .read_exact(&mut bytes)
        .await
        .map_err(|_| Error::new("TRANSPORT_IO", "Viewer message interrupted"))?;
    serde_json::from_slice(&bytes).map_err(|_| Error::invalid("Invalid viewer JSON"))
}
pub(crate) async fn write_frame(
    writer: &mut (impl AsyncWrite + Unpin),
    value: &Value,
) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let bytes = serde_json::to_vec(value)
        .map_err(|_| Error::new("INTERNAL", "Cannot encode viewer reply"))?;
    if bytes.len() > 1024 * 1024 {
        return Err(Error::new("OUTPUT_LIMIT", "Viewer reply exceeds 1 MiB"));
    }
    writer
        .write_u32(bytes.len() as u32)
        .await
        .map_err(|_| Error::new("TRANSPORT_IO", "Viewer disconnected"))?;
    writer
        .write_all(&bytes)
        .await
        .map_err(|_| Error::new("TRANSPORT_IO", "Viewer reply interrupted"))
}

#[cfg(test)]
pub(crate) async fn connect_test(path: PathBuf) -> io::Result<impl AsyncRead + AsyncWrite + Unpin> {
    #[cfg(unix)]
    return tokio::net::UnixStream::connect(path).await;
    #[cfg(windows)]
    {
        let stream = tokio::task::spawn_blocking(move || {
            canonic_process_kit::endpoint::connect_local(&path, Duration::from_secs(2))
                .map_err(io::Error::other)
        })
        .await??;
        windows::Socket::new(stream)
    }
}

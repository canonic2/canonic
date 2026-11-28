//! Bounded Tokio I/O and Testrun lanes; canonic-mcp-kit owns frame admission and request lifetime.
use super::{Protocol, rpc_error};
use crate::{
    error::{Error, Result},
    qa::Server,
};
use serde_json::{Value, json};
use std::{collections::VecDeque, time::Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub(super) const MAX_REQUEST: usize = crate::browser::transfer::FIXTURE_FRAME_BYTES;
const DISCONNECT_GRACE: std::time::Duration = std::time::Duration::from_millis(250);
pub async fn serve(mut server: Server) -> Result<()> {
    // Refuse invalid catalogs or options before accepting any input.
    let protocol = Protocol::try_new()?;
    // Modern requests need no handshake, so the connection exists from the start.
    server.managed.attach_mcp(protocol.connection_id.clone());
    server.enable_scheduling();
    let (notifications, mut events) = tokio::sync::mpsc::channel(16);
    server.notifications(notifications);
    let (sender, mut requests) = tokio::sync::mpsc::channel(64);
    let reader = tokio::spawn(async move {
        if let Err(error) = read_requests(sender.clone()).await {
            let _ = sender.send(Input::Failure(error)).await;
        }
    });
    let result = tokio::select! {
        result = dispatch(protocol, &mut server, &mut requests, &mut events) => result,
        result = termination() => result,
    };
    reader.abort();
    let cleanup = server.shutdown().await;
    result.and(cleanup)
}
async fn termination() -> Result<()> {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate())
            .map_err(|_| Error::new("HOST_IO", "Cannot observe host termination"))?;
        tokio::select! { result = tokio::signal::ctrl_c() => result.map_err(|_| Error::new("HOST_IO", "Cannot observe host interrupt")), _ = terminate.recv() => Ok(()) }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c()
        .await
        .map_err(|_| Error::new("HOST_IO", "Cannot observe host interrupt"))
}
enum Input {
    Frame(Vec<u8>, Instant),
    Failure(Error),
}
async fn read_requests(sender: tokio::sync::mpsc::Sender<Input>) -> Result<()> {
    let mut input = BufReader::new(tokio::io::stdin());
    loop {
        let mut bytes = Vec::new();
        loop {
            let available = input
                .fill_buf()
                .await
                .map_err(|_| Error::new("TRANSPORT_IO", "Cannot read MCP input"))?;
            if available.is_empty() {
                if bytes.is_empty() {
                    return Ok(());
                }
                break;
            }
            let count = available
                .iter()
                .position(|b| *b == b'\n')
                .map_or(available.len(), |i| i + 1);
            // Buffer at most one byte beyond the payload bound. Session receives
            // that oversized frame and owns the fatal parse reply; do not drain
            // an unbounded unterminated input into memory.
            let room = (MAX_REQUEST + 1).saturating_sub(bytes.len());
            if count > room {
                bytes.extend_from_slice(&available[..room]);
                let _ = sender.send(Input::Frame(bytes, Instant::now())).await;
                return Ok(());
            }
            bytes.extend_from_slice(&available[..count]);
            input.consume(count);
            if bytes.last() == Some(&b'\n') {
                break;
            }
        }
        if sender
            .send(Input::Frame(bytes, Instant::now()))
            .await
            .is_err()
        {
            return Ok(());
        }
    }
}
type Queued = (Value, Instant);
enum Admitted {
    Call(Value, Instant),
    /// A skill file read; it runs in the read lane, never behind device work.
    Bundled(Value, canonic_mcp_kit::BundledRead),
    Reply(Value),
    Completed(Value),
    Fatal(Value),
    Cancel(Value),
    Ignored,
}
fn admit(
    protocol: &mut Protocol,
    client: &mut crate::managed::Client,
    input: Input,
) -> Result<Admitted> {
    let Input::Frame(frame, received) = input else {
        let Input::Failure(error) = input else {
            unreachable!()
        };
        return Err(error);
    };
    use canonic_mcp_kit::Inbound;
    Ok(match protocol.receive(&frame) {
        Inbound::Subscription { id, .. } => Admitted::Completed(rpc_error(
            id,
            -32601,
            "Subscriptions are not enabled for this server",
        )),
        Inbound::Completion { id, .. } => Admitted::Completed(rpc_error(
            id,
            -32601,
            "Completions are not enabled for this server",
        )),
        Inbound::Prompt { id, .. } => Admitted::Completed(rpc_error(
            id,
            -32601,
            "Prompts are not enabled for this server",
        )),
        Inbound::Task { id, .. } => Admitted::Completed(rpc_error(
            id,
            -32601,
            "Tasks are not enabled for this server",
        )),
        Inbound::BundledResource { id, read, .. } => Admitted::Bundled(id, read),
        Inbound::Reply(reply) => Admitted::Reply(reply),
        Inbound::Fatal { reply, .. } => Admitted::Fatal(reply),
        Inbound::Initialize { id, .. } => Admitted::Reply(protocol.initialize(client, id)),
        Inbound::Tool { id, call } => Admitted::Call(super::tool_request(id, call), received),
        Inbound::InvalidArguments { id, call, error } => {
            Admitted::Completed(super::invalid_arguments(id, &call, error))
        }
        Inbound::Method { id, method, params } if method == "resources/read" => {
            Admitted::Completed(protocol.missing_resource(id, &params))
        }
        Inbound::Method { id, method, params } => {
            if protocol.empty_params(&id, &params) {
                Admitted::Call(
                    json!({"jsonrpc":"2.0","id":id,"method":method,"params":{}}),
                    received,
                )
            } else {
                Admitted::Completed(rpc_error(id, -32602, "Task end takes no parameters"))
            }
        }
        Inbound::Notification { method, params } => {
            if params.as_object().is_some_and(serde_json::Map::is_empty) {
                Admitted::Call(
                    json!({"jsonrpc":"2.0","method":method,"params":params}),
                    received,
                )
            } else {
                Admitted::Ignored
            }
        }
        Inbound::Cancel { id } => Admitted::Cancel(id),
        Inbound::Confirmation { .. } | Inbound::Elicitation { .. } | Inbound::Ignored => {
            Admitted::Ignored
        }
    })
}
enum Decision {
    Reply(Option<Value>),
    Cancelled,
    Disconnected,
    Fatal,
    Failed(Error),
}
async fn dispatch(
    mut protocol: Protocol,
    server: &mut Server,
    requests: &mut tokio::sync::mpsc::Receiver<Input>,
    events: &mut tokio::sync::mpsc::Receiver<Value>,
) -> Result<()> {
    let result = dispatch_connection(&mut protocol, server, requests, events).await;
    let closed = match protocol.close() {
        Some(notice) => respond(notice).await,
        None => Ok(()),
    };
    result.and(closed)
}
#[expect(
    clippy::too_many_lines,
    reason = "The select loop preserves Testrun's ordered mutation, read, cancellation and EOF ownership boundary"
)]
async fn dispatch_connection(
    protocol: &mut Protocol,
    server: &mut Server,
    requests: &mut tokio::sync::mpsc::Receiver<Input>,
    events: &mut tokio::sync::mpsc::Receiver<Value>,
) -> Result<()> {
    let mut queue = VecDeque::<Queued>::new();
    let mut disconnected = false;
    let mut reads = super::responsive::Reads::default();
    let mut long_reads = super::responsive::Reads::default();
    let mut jobs = super::scheduled::Jobs::default();
    let targets = server
        .scheduling()
        .expect("Shipping transport enables scheduling");
    let read_server = server.discovery_reader();
    loop {
        let (request, admitted) = if let Some(request) = queue.pop_front() {
            request
        } else {
            let incoming = tokio::select! {
                result = reads.next(), if !reads.is_empty() => { if let Some(response)=result? { reply(protocol, response).await?; } continue; },
                result = long_reads.next(), if !long_reads.is_empty() => { if let Some(response)=result? { reply(protocol, response).await?; } continue; },
                result = jobs.next(), if !jobs.is_empty() => { if let Some(response)=result? { reply(protocol, response).await?; } finalize(&mut jobs, &mut queue); continue; },
                Some(event) = events.recv() => { respond(event).await?; continue; },
                incoming = requests.recv() => incoming,
            };
            let Some(incoming) = incoming else {
                drop(long_reads);
                jobs.finish().await?;
                let until = tokio::time::Instant::now() + DISCONNECT_GRACE;
                while !reads.is_empty() {
                    let Ok(response) = tokio::time::timeout_at(until, reads.next()).await else {
                        break;
                    };
                    if let Some(response) = response? {
                        reply(protocol, response).await?;
                    }
                }
                return Ok(());
            };
            match admit(protocol, &mut server.managed, incoming)? {
                Admitted::Call(request, admitted) => (request, admitted),
                Admitted::Bundled(id, read) => {
                    if let Some(error) = reads.admit_bundled(id, read) {
                        reply(protocol, error).await?;
                    }
                    continue;
                }
                Admitted::Reply(response) => {
                    core_reply(protocol, response).await?;
                    continue;
                }
                Admitted::Completed(response) => {
                    reply(protocol, response).await?;
                    continue;
                }
                Admitted::Fatal(response) => {
                    core_reply(protocol, response).await?;
                    jobs.finish().await?;
                    return Ok(());
                }
                Admitted::Cancel(id) => {
                    reads.cancel(&id);
                    long_reads.cancel(&id);
                    jobs.cancel(&id);
                    queue.retain(|(request, _)| request.get("id") != Some(&id));
                    continue;
                }
                Admitted::Ignored => continue,
            }
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        if disconnected {
            if !id.is_null() {
                reply(
                    protocol,
                    rpc_error(id, -32800, "Connection closed before dispatch"),
                )
                .await?;
            }
            continue;
        }
        if protocol.target_status(&request, &targets).is_some() {
            if let Some(error) = reads.admit_target_status(protocol, &targets, &request) {
                reply(protocol, error).await?;
            }
            continue;
        }
        if protocol
            .responsive_request(&request, &server.managed)
            .is_some()
        {
            if let Some(error) = reads.admit(protocol, &server.managed, &request) {
                reply(protocol, error).await?;
            }
            continue;
        }
        if protocol
            .independent_request(&request, &read_server, &server.managed, admitted)
            .is_some()
        {
            if let Some(error) =
                long_reads.admit_long(protocol, &read_server, &server.managed, &request, admitted)
            {
                reply(protocol, error).await?;
            }
            continue;
        }
        let read_only = read_only(&request);
        if let Some(response) = jobs.admit(protocol, &server.managed, &targets, &request, admitted)
        {
            if let Some(response) = response {
                reply(protocol, response).await?;
            }
            continue;
        }
        let mut worker_protocol = protocol.clone();
        let mut read_client = server.managed.clone();
        let decision = {
            let operation = worker_protocol.admitted_request(server, request, admitted);
            tokio::pin!(operation);
            let mut grace = None;
            loop {
                tokio::select! {
                    result=reads.next(),if !reads.is_empty()=>if let Some(response)=result?{reply(protocol,response).await?;},
                    result=long_reads.next(),if !long_reads.is_empty()=>if let Some(response)=result?{reply(protocol,response).await?;},
                    result=jobs.next(),if !jobs.is_empty()=>{if let Some(response)=result?{reply(protocol,response).await?;}finalize(&mut jobs,&mut queue);},
                    Some(event)=events.recv()=>respond(event).await?,
                    response=&mut operation=>break Decision::Reply(response),
                    _=tokio::time::sleep_until(grace.unwrap_or_else(||tokio::time::Instant::now()+std::time::Duration::from_secs(86400))),if grace.is_some()=>break Decision::Disconnected,
                    incoming=requests.recv(),if !disconnected=>match incoming {
                        None=>{disconnected=true;grace=Some(tokio::time::Instant::now()+DISCONNECT_GRACE);},
                        Some(incoming)=>match admit(protocol, &mut read_client, incoming) {
                            Err(error)=>break Decision::Failed(error),
                            Ok(Admitted::Reply(response))=>core_reply(protocol,response).await?,
                            Ok(Admitted::Completed(response))=>reply(protocol,response).await?,
                            Ok(Admitted::Fatal(response))=>{core_reply(protocol,response).await?;break Decision::Fatal;},
                            Ok(Admitted::Ignored)=>{},
                            Ok(Admitted::Bundled(read_id, read))=>if let Some(error)=reads.admit_bundled(read_id, read){reply(protocol,error).await?;},
                            Ok(Admitted::Cancel(cancelled))=>{
                                reads.cancel(&cancelled); long_reads.cancel(&cancelled); jobs.cancel(&cancelled);
                                queue.retain(|(request,_)|request.get("id")!=Some(&cancelled));
                                if cancelled==id&&!id.is_null(){break Decision::Cancelled;}
                            },
                            Ok(Admitted::Call(next, admitted))=>{
                                if protocol.target_status(&next,&targets).is_some(){
                                    if let Some(error)=reads.admit_target_status(protocol,&targets,&next){reply(protocol,error).await?;}
                                }else if protocol.responsive_request(&next,&read_client).is_some(){
                                    if let Some(error)=reads.admit(protocol,&read_client,&next){reply(protocol,error).await?;}
                                }else if protocol.independent_request(&next,&read_server,&read_client,admitted).is_some(){
                                    if let Some(error)=long_reads.admit_long(protocol,&read_server,&read_client,&next,admitted){reply(protocol,error).await?;}
                                }else if let Some(response)=jobs.admit(protocol,&read_client,&targets,&next,admitted){
                                    if let Some(response)=response{reply(protocol,response).await?;}
                                }else if protocol.task_end(&next){
                                    queue.push_front((next,admitted));
                                }else if queue.len()<64{queue.push_back((next,admitted));}
                                else if let Some(id)=next.get("id"){reply(protocol,rpc_error(id.clone(),-32000,"MCP request queue is full")).await?;}
                            },
                        }
                    }
                }
            }
        };
        match decision {
            Decision::Reply(Some(response)) => reply(protocol, response).await?,
            Decision::Reply(None) => {}
            Decision::Cancelled => {
                server.cancel_dispatched().await?;
            }
            Decision::Disconnected => {
                server.cancel_dispatched().await?;
                if !id.is_null() {
                    let message = if read_only {
                        "Connection closed before the read completed"
                    } else {
                        "Connection closed; dispatched mutations may have an unknown outcome"
                    };
                    reply(protocol, rpc_error(id, -32800, message)).await?;
                }
            }
            Decision::Fatal => {
                server.cancel_dispatched().await?;
                jobs.finish().await?;
                return Ok(());
            }
            Decision::Failed(error) => return Err(error),
        }
    }
}
fn finalize(jobs: &mut super::scheduled::Jobs, queue: &mut VecDeque<Queued>) {
    for request in jobs.take_finalize() {
        queue.push_front((request, Instant::now()));
    }
}
fn read_only(request: &Value) -> bool {
    match request["method"].as_str() {
        Some("tools/call") => super::tools()
            .get(request["params"]["name"].as_str().unwrap_or(""))
            .is_some_and(canonic_mcp_kit::Tool::read_only),
        _ => false,
    }
}
/// Writes an admitted request's reply, encoded for its own revision, unless it was cancelled.
async fn reply(protocol: &Protocol, response: Value) -> Result<()> {
    match protocol.settle(&response) {
        Some(encoded) => write(encoded).await,
        None => Ok(()),
    }
}
/// Core-generated replies are already complete and revision-correct; finish is
/// still called before writing, but these IDs are not tracked as host work.
async fn core_reply(protocol: &Protocol, response: Value) -> Result<()> {
    if let Some(id) = response.get("id") {
        protocol.finish(id);
    }
    respond(response).await
}
async fn respond(response: Value) -> Result<()> {
    write(super::frame(&response)).await
}
async fn write(mut encoded: Vec<u8>) -> Result<()> {
    let mut output = tokio::io::stdout();
    encoded.push(b'\n');
    output
        .write_all(&encoded)
        .await
        .map_err(|_| Error::new("TRANSPORT_IO", "Cannot write MCP output"))?;
    output
        .flush()
        .await
        .map_err(|_| Error::new("TRANSPORT_IO", "Cannot flush MCP output"))
}

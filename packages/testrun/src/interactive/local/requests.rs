//! Bounded retained IPC admission; ordered mutations and independent observations/status.
use super::{
    read,
    reads::Handle,
    transport::{check_owner, fence},
    write,
};
use crate::{
    error::{Error, Result},
    qa::{Server, journal::Archive},
};
use futures_util::{StreamExt, future::LocalBoxFuture, stream::FuturesUnordered};
use serde_json::{Value, json};
use std::{collections::VecDeque, time::Duration};
use tokio::net::{UnixListener, UnixStream};

/// Resolves after `idle` without requests. A device actor never idles out:
/// the manager decides when to release its device.
async fn idle_expired(owner: Option<&crate::managed::Owner>, idle: Duration) {
    if owner.is_some() {
        return std::future::pending().await;
    }
    tokio::time::sleep(idle).await;
}

type Request = (UnixStream, Value);
type Handshake = LocalBoxFuture<'static, Option<Request>>;
type Read = LocalBoxFuture<'static, Option<Value>>;
enum Event {
    Request(Request),
    Display(Value),
}
#[derive(Default)]
struct Admission {
    handshakes: FuturesUnordered<Handshake>,
    reads: FuturesUnordered<Read>,
    status: FuturesUnordered<Read>,
    ordered: VecDeque<Request>,
    closing: bool,
    failure: Option<Value>,
}
impl Admission {
    async fn next(&mut self, listener: &UnixListener, epoch: Option<&str>) -> Result<Event> {
        loop {
            tokio::select! {
                Some(display)=self.reads.next(),if !self.reads.is_empty()=>if let Some(display)=display{return Ok(Event::Display(display));},
                _=self.status.next(),if !self.status.is_empty()=>{},
                Some(request)=self.handshakes.next(),if !self.handshakes.is_empty()=>if let Some(request)=request{return Ok(Event::Request(request));},
                accepted=listener.accept()=>{
                    let (mut socket,_)=accepted.map_err(|_|Error::new("HOST_IO","Controller listener failed"))?;
                    if self.handshakes.len()>=8 {reply(&mut socket,Err(overloaded())).await;continue;}
                    let epoch=epoch.map(String::from);
                    self.handshakes.push(Box::pin(async move {
                        let result=tokio::time::timeout(Duration::from_secs(5),async {
                            check_owner(&socket)?;let mut input=read(&mut socket).await?;
                            if !fence(&mut input,epoch.as_deref()){return Err(Error::new("STALE_OWNER","This request belongs to another connection"));}
                            super::server::validate_request(&input)?;
                            Ok(input)
                        }).await;
                        match result {
                            Ok(Ok(input))=>Some((socket,input)),
                            Ok(Err(error))=>{reply(&mut socket,Err(error)).await;None},
                            Err(_)=>None,
                        }
                    }));
                }
            }
        }
    }
    async fn admit(
        &mut self,
        mut socket: UnixStream,
        mut input: Value,
        handle: Option<&Handle>,
        archive: &Archive,
    ) -> Option<Request> {
        if let Some(failure) = &self.failure {
            if input["kind"] == "close" {
                return Some((socket, input));
            }
            let result = if input["kind"] == "status" {
                if let Some(command) = input["commandId"].as_str() {
                    archive.status(command)
                } else {
                    Ok(failure.clone())
                }
            } else {
                Err(Error::new(
                    "SESSION_CLOSING",
                    "Cleanup is unfinished; retry close",
                ))
            };
            reply(&mut socket, result).await;
            return None;
        }
        if self.closing {
            reply(
                &mut socket,
                if input["kind"] == "status" || input["kind"] == "close" {
                    Ok(json!({"state":"closing","closed":false}))
                } else {
                    Err(Error::new("SESSION_CLOSING", "Owned cleanup is starting"))
                },
            )
            .await;
            return None;
        }
        if let Some(handle) = handle.filter(|handle| handle.supports(&input)) {
            let status = matches!(
                input["kind"].as_str(),
                Some("status" | "control_status" | "control_grant")
            );
            let pool = if status {
                &mut self.status
            } else {
                &mut self.reads
            };
            let limit = if status { 8 } else { 4 };
            if pool.len() >= limit {
                reply(&mut socket, Err(overloaded())).await;
            } else {
                let handle = handle.clone();
                pool.push(Box::pin(handle.serve(socket, input)));
            }
            None
        } else {
            if self.ordered.len() >= 8 {
                reply(&mut socket, Err(overloaded())).await;
                return None;
            }
            if let Some(handle) = handle
                && let Err(error) = handle.prepare(&mut input)
            {
                reply(&mut socket, Err(error)).await;
                return None;
            }
            Some((socket, input))
        }
    }
}
async fn reply(socket: &mut UnixStream, result: Result<Value>) {
    let response = match result {
        Ok(data) => json!({"ok":true,"data":data}),
        Err(error) => json!({"ok":false,"error":error}),
    };
    let _ = tokio::time::timeout(Duration::from_secs(1), write(socket, &response)).await;
}
fn overloaded() -> Error {
    Error::new("OVERLOADED", "Controller request capacity is full")
        .details(json!({"admitted":false,"dispatched":false}))
}

pub(super) async fn serve(
    server: &mut Server,
    listener: &UnixListener,
    mut common: Value,
    idle: Duration,
    owner: Option<&crate::managed::Owner>,
    archive: &Archive,
) -> Result<Value> {
    let mut admission = Admission::default();
    loop {
        // A failed close may already have removed the session or revoked its
        // reads. Recovery needs only the journal, cleanup status and run owner.
        let handle = if admission.failure.is_none() {
            Some(Handle::new(server, &common, archive.clone())?)
        } else {
            None
        };
        let request = if let Some(request) = admission.ordered.pop_front() {
            request
        } else {
            loop {
                let event = tokio::select! {
                    event=admission.next(listener,owner.map(crate::managed::Owner::epoch))=>event?,
                    ()=idle_expired(owner,idle)=>return Ok(json!({"closed":true,"reason":"idle_timeout"})),
                    ended=super::server::owner_ended(owner)=>return ended,
                };
                match event {
                    Event::Display(display) => common["display"] = display,
                    Event::Request((socket, input)) => {
                        if let Some(request) = admission
                            .admit(socket, input, handle.as_ref(), archive)
                            .await
                        {
                            break request;
                        }
                    }
                }
            }
        };
        let (mut socket, input) = request;
        // A queued close fences admission only while it waits. Once it leaves
        // the queue it is handled like an unqueued close, so an abandoned or
        // failed close cannot leave later status and close requests refused.
        if input["kind"] == "close" {
            admission.closing = false;
        }
        // An abandoned queued mutation was never dispatched. It must not become
        // delayed input merely because the earlier owner operation completed.
        let mut probe = [0_u8; 1];
        if !matches!(socket.try_read(&mut probe),Err(error) if error.kind()==std::io::ErrorKind::WouldBlock)
        {
            continue;
        }
        let close = input["kind"] == "close";
        if close && owner.is_some() {
            if let Some(owner) = owner {
                owner.state("closing", json!({}))?;
            }
            reply(&mut socket, Ok(json!({"state":"closing","closed":false}))).await;
            return Ok(json!({"closed":true}));
        }
        let command = input["commandId"].clone();
        let mut display = None;
        let result = {
            let operation = super::server::dispatch(server, &mut common, input);
            tokio::pin!(operation);
            loop {
                tokio::select! {
                    result=&mut operation=>break result,
                    event=admission.next(listener,owner.map(crate::managed::Owner::epoch))=>match event? {
                        Event::Display(latest)=>display=Some(latest),
                        Event::Request((socket,input))=>if let Some((mut socket,input))=admission.admit(socket,input,handle.as_ref(),archive).await {
                            if admission.ordered.len()>=8 {reply(&mut socket,Err(overloaded())).await;}else{if input["kind"]=="close" {admission.closing=true;}admission.ordered.push_back((socket,input));}
                        },
                    }
                }
            }
        };
        if let Some(display) = display {
            common["display"] = display;
        }
        let succeeded = result.is_ok();
        if close && let Err(error) = &result {
            admission.failure = Some(
                json!({"state":"failed","closed":false,"cleanupError":error,"cleanup":server.cleanup_status()}),
            );
        }
        let mut response = match result {
            Ok(data) => json!({"ok":true,"data":data}),
            Err(error) => json!({"ok":false,"error":error}),
        };
        if command.is_string() {
            response["commandId"] = command;
        }
        let _ = tokio::time::timeout(Duration::from_secs(1), write(&mut socket, &response)).await;
        if close && succeeded {
            return Ok(response);
        }
    }
}

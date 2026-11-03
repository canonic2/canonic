//! One connection: the HTTP/1.1 request subset, the exchange and the response.
//!
//! The thread reads and checks one request, hands an admitted request to the
//! dispatcher and writes what its mailbox receives. Every response closes the
//! connection. A client that closes early cancels its request.

use super::{
    Event, Last, MAX_HEADER_BYTES, MAX_HEADER_FIELDS, MAX_REQUEST_LINE_BYTES, Mailbox, Shared,
};
use crate::{
    http::{
        Admitted, Exchange, Form, RequestHead, Response, SSE_KEEP_ALIVE, is_token, sse_event,
        status_for,
    },
    rpc,
};
use std::{
    io::{self, BufRead, BufReader, ErrorKind, Read, Write},
    net::TcpStream,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

/// How often a waiting connection checks whether its client has closed.
const CLOSURE_POLL: Duration = Duration::from_millis(100);
/// Input after the request discarded before the client counts as closed.
const MAX_TRAILING_BYTES: usize = 64 * 1024;

/// A request read off the wire.
struct Request {
    method: String,
    target: String,
    headers: Vec<(String, Vec<u8>)>,
    body: Vec<u8>,
}

/// Why reading stopped.
enum Failure {
    /// Close without a response: timeout, early close or a read error.
    Close,
    /// Write this response, then close.
    Respond(Response),
}

impl Failure {
    fn status(status: u16) -> Self {
        Self::Respond(Response {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        })
    }
}

/// Serves one accepted connection.
pub(super) fn handle(stream: &TcpStream, shared: &Arc<Shared>) {
    // An accepted socket can inherit the listener's mode; reads use timeouts.
    if stream.set_nonblocking(false).is_err() {
        return;
    }
    let request = match read_request(stream, shared) {
        Ok(request) => request,
        Err(Failure::Close) => return,
        Err(Failure::Respond(response)) => {
            refuse(stream, &response, shared.write_timeout);
            return;
        }
    };
    let headers = request.fields();
    let head = request.head(&headers);
    match shared.endpoint.receive(&head, &request.body) {
        Exchange::Respond(response) => {
            let _ = write_response(stream, &response, false, shared.write_timeout);
        }
        Exchange::Notification { method, params } => {
            let _ = write_response(stream, &Response::accepted(), false, shared.write_timeout);
            if shared.notifications.contains(&method) {
                shared.send(Event::Notification(rpc::notification(&method, params)));
            }
        }
        Exchange::Request(admitted) => exchange(stream, shared, admitted),
    }
}

/// Writes a refusal sent before the request was fully read, then discards a
/// bounded amount of the unread input, so closing does not reset the
/// connection before the client reads the refusal.
fn refuse(stream: &TcpStream, response: &Response, timeout: Duration) {
    if write_response(stream, response, false, timeout).is_err()
        || stream.shutdown(std::net::Shutdown::Write).is_err()
    {
        return;
    }
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut discarded = 0;
    let mut buffer = [0; 4096];
    while discarded < MAX_HEADER_BYTES * 2 {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || stream.set_read_timeout(Some(remaining)).is_err() {
            return;
        }
        match (&*stream).read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => discarded += read,
        }
    }
}

/// Writes `503` to a connection above the limit.
pub(super) fn refuse_busy(stream: &TcpStream) {
    let _ = write_response(
        stream,
        &Response {
            status: 503,
            headers: Vec::new(),
            body: Vec::new(),
        },
        false,
        Duration::from_secs(1),
    );
}

impl Request {
    fn fields(&self) -> Vec<(&str, &[u8])> {
        self.headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_slice()))
            .collect()
    }

    fn head<'a>(&'a self, headers: &'a [(&'a str, &'a [u8])]) -> RequestHead<'a> {
        RequestHead {
            method: &self.method,
            target: &self.target,
            headers,
        }
    }
}

fn read_request(stream: &TcpStream, shared: &Shared) -> Result<Request, Failure> {
    let deadline = Instant::now() + shared.read_timeout;
    let mut reader = BufReader::new(stream);
    let line = read_line(&mut reader, MAX_REQUEST_LINE_BYTES, deadline)?;
    let line = std::str::from_utf8(&line).map_err(|_| Failure::status(400))?;
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Failure::status(400));
    };
    if version != "HTTP/1.1" {
        return Err(Failure::status(if version.starts_with("HTTP/") {
            505
        } else {
            400
        }));
    }
    if !is_token(method) || target.is_empty() {
        return Err(Failure::status(400));
    }
    let mut request = Request {
        method: method.to_owned(),
        target: target.to_owned(),
        headers: read_headers(&mut reader, deadline)?,
        body: Vec::new(),
    };
    let headers = request.fields();
    let field = |name: &'static str| {
        headers
            .iter()
            .filter(move |(field, _)| field.eq_ignore_ascii_case(name))
            .map(|(_, value)| *value)
    };
    let length = field("content-length").collect::<Vec<_>>();
    if field("transfer-encoding").next().is_some() {
        return Err(Failure::status(if length.is_empty() { 501 } else { 400 }));
    }
    let head = request.head(&headers);
    shared
        .endpoint
        .check_head(&head)
        .map_err(Failure::Respond)?;
    let length = match length.as_slice() {
        [] => return Err(Failure::status(411)),
        [value] => std::str::from_utf8(value)
            .ok()
            .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| Failure::status(400))?,
        _ => return Err(Failure::status(400)),
    };
    shared
        .endpoint
        .check_length(length)
        .map_err(Failure::Respond)?;
    match field("expect").collect::<Vec<_>>().as_slice() {
        [] => {}
        [value] if value.eq_ignore_ascii_case(b"100-continue") => {
            stream
                .set_write_timeout(Some(shared.write_timeout))
                .and_then(|()| (&*stream).write_all(b"HTTP/1.1 100 Continue\r\n\r\n"))
                .map_err(|_| Failure::Close)?;
        }
        _ => return Err(Failure::status(417)),
    }
    drop(headers);
    request.body = read_body(&mut reader, length, deadline)?;
    Ok(request)
}

/// Reads header fields until the blank line, within the section bounds.
fn read_headers(
    reader: &mut BufReader<&TcpStream>,
    deadline: Instant,
) -> Result<Vec<(String, Vec<u8>)>, Failure> {
    let mut headers = Vec::new();
    let mut budget = MAX_HEADER_BYTES;
    loop {
        let (line, used) = read_line_counted(reader, budget, deadline)?;
        budget -= used;
        if line.is_empty() {
            return Ok(headers);
        }
        if headers.len() == MAX_HEADER_FIELDS {
            return Err(Failure::status(431));
        }
        let colon = line
            .iter()
            .position(|byte| *byte == b':')
            .ok_or_else(|| Failure::status(400))?;
        let name = std::str::from_utf8(&line[..colon])
            .ok()
            .filter(|name| is_token(name))
            .ok_or_else(|| Failure::status(400))?;
        let value = line[colon + 1..].trim_ascii();
        if value
            .iter()
            .any(|byte| byte.is_ascii_control() && *byte != b'\t')
        {
            return Err(Failure::status(400));
        }
        headers.push((name.to_owned(), value.to_vec()));
    }
}

fn read_line(
    reader: &mut BufReader<&TcpStream>,
    limit: usize,
    deadline: Instant,
) -> Result<Vec<u8>, Failure> {
    read_line_counted(reader, limit, deadline).map(|(line, _)| line)
}

/// Reads one line of at most `limit` bytes with its line break, which is
/// removed. Returns the line and the bytes it used.
fn read_line_counted(
    reader: &mut BufReader<&TcpStream>,
    limit: usize,
    deadline: Instant,
) -> Result<(Vec<u8>, usize), Failure> {
    let mut line = Vec::new();
    loop {
        let buffer = fill(reader, deadline)?;
        let (taken, done) = match buffer.iter().position(|byte| *byte == b'\n') {
            Some(index) => (index + 1, true),
            None => (buffer.len(), false),
        };
        line.extend_from_slice(&buffer[..taken]);
        reader.consume(taken);
        if line.len() > limit {
            return Err(Failure::status(431));
        }
        if done {
            let used = line.len();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok((line, used));
        }
    }
}

fn read_body(
    reader: &mut BufReader<&TcpStream>,
    length: usize,
    deadline: Instant,
) -> Result<Vec<u8>, Failure> {
    let mut body = Vec::with_capacity(length);
    while body.len() < length {
        let buffer = fill(reader, deadline)?;
        let taken = buffer.len().min(length - body.len());
        body.extend_from_slice(&buffer[..taken]);
        reader.consume(taken);
    }
    Ok(body)
}

/// Fills the read buffer before `deadline`; an empty read is an early close.
fn fill<'a>(reader: &'a mut BufReader<&TcpStream>, deadline: Instant) -> Result<&'a [u8], Failure> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Failure::Close);
        }
        reader
            .get_ref()
            .set_read_timeout(Some(remaining))
            .map_err(|_| Failure::Close)?;
        match reader.fill_buf() {
            Ok([]) => return Err(Failure::Close),
            // Reborrow after the match so the buffer outlives the loop.
            Ok(_) => break,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => return Err(Failure::Close),
        }
    }
    reader.fill_buf().map_err(|_| Failure::Close)
}

/// Hands `admitted` to the dispatcher and writes its response.
fn exchange(stream: &TcpStream, shared: &Arc<Shared>, admitted: Admitted) {
    let id = shared.next_id.fetch_add(1, Ordering::Relaxed);
    let form = admitted.form;
    let mailbox = Arc::new(Mailbox::default());
    if !shared.send(Event::Submit {
        id,
        admitted,
        mailbox: Arc::clone(&mailbox),
    }) {
        return;
    }
    let mut writer = Writer {
        stream,
        timeout: shared.write_timeout,
        streaming: false,
        last_write: Instant::now(),
    };
    let mut discarded = 0;
    loop {
        let wait = if writer.streaming {
            CLOSURE_POLL.min(
                shared
                    .keep_alive
                    .saturating_sub(writer.last_write.elapsed()),
            )
        } else {
            CLOSURE_POLL
        };
        let (events, last, ended) = mailbox.wait(wait);
        match writer.write(form, events, last, ended, shared.keep_alive) {
            Ok(true) => return,
            Ok(false) if !peer_closed(stream, &mut discarded) => {}
            Ok(false) | Err(_) => {
                shared.send(Event::Closed(id));
                return;
            }
        }
    }
}

struct Writer<'a> {
    stream: &'a TcpStream,
    timeout: Duration,
    streaming: bool,
    last_write: Instant,
}

impl Writer<'_> {
    /// Writes pending output; returns whether the response is complete.
    fn write(
        &mut self,
        form: Form,
        events: Vec<Vec<u8>>,
        last: Option<Last>,
        ended: bool,
        keep_alive: Duration,
    ) -> io::Result<bool> {
        if form == Form::Json {
            return match last {
                Some(last) => {
                    self.respond(&complete(last))?;
                    Ok(true)
                }
                None => Ok(false),
            };
        }
        if !self.streaming && events.is_empty() {
            // A refusal or error before any event keeps its own status.
            match last {
                Some(Last::Response(response)) => {
                    self.respond(&response)?;
                    return Ok(true);
                }
                Some(Last::Reply(bytes, Some(code))) if status_for(code) != 200 => {
                    self.respond(&Response::reply(bytes, Some(code)))?;
                    return Ok(true);
                }
                _ => {}
            }
        }
        if !self.streaming && (!events.is_empty() || last.is_some()) {
            write_response(self.stream, &Response::stream(), true, self.timeout)?;
            self.streaming = true;
        }
        for event in events {
            self.send(&sse_event(&event))?;
        }
        if let Some(last) = last {
            self.send(&sse_event(&complete(last).body))?;
            return Ok(true);
        }
        if ended {
            return Ok(true);
        }
        if self.streaming && self.last_write.elapsed() >= keep_alive {
            self.send(SSE_KEEP_ALIVE)?;
        }
        Ok(false)
    }

    fn respond(&mut self, response: &Response) -> io::Result<()> {
        write_response(self.stream, response, false, self.timeout)
    }

    fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.stream.set_write_timeout(Some(self.timeout))?;
        let mut stream = self.stream;
        stream.write_all(bytes)?;
        stream.flush()?;
        self.last_write = Instant::now();
        Ok(())
    }
}

fn complete(last: Last) -> Response {
    match last {
        Last::Reply(bytes, code) => Response::reply(bytes, code),
        Last::Response(response) => response,
    }
}

/// Whether the client has closed its side, checked without blocking.
///
/// Input after the request is discarded, so it cannot hide a later close;
/// more than [`MAX_TRAILING_BYTES`] of it counts as a closed client.
fn peer_closed(stream: &TcpStream, discarded: &mut usize) -> bool {
    if stream.set_nonblocking(true).is_err() {
        return true;
    }
    let mut buffer = [0; 4096];
    let closed = loop {
        match (&*stream).read(&mut buffer) {
            Ok(0) => break true,
            Ok(read) => {
                *discarded += read;
                if *discarded > MAX_TRAILING_BYTES {
                    break true;
                }
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => break false,
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => break true,
        }
    };
    stream.set_nonblocking(false).is_err() || closed
}

/// Writes a status line, the response's headers and its body. A stream head
/// has no `Content-Length`; every response closes the connection.
fn write_response(
    stream: &TcpStream,
    response: &Response,
    stream_head: bool,
    timeout: Duration,
) -> io::Result<()> {
    let mut out = Vec::with_capacity(256 + response.body.len());
    write!(
        out,
        "HTTP/1.1 {} {}\r\n",
        response.status,
        reason(response.status)
    )?;
    for (name, value) in &response.headers {
        write!(out, "{name}: {value}\r\n")?;
    }
    if !stream_head {
        write!(out, "Content-Length: {}\r\n", response.body.len())?;
    }
    out.extend_from_slice(b"Connection: close\r\n\r\n");
    out.extend_from_slice(&response.body);
    stream.set_write_timeout(Some(timeout))?;
    let mut stream = stream;
    stream.write_all(&out)?;
    stream.flush()
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        411 => "Length Required",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        417 => "Expectation Failed",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        505 => "HTTP Version Not Supported",
        _ => "Unknown",
    }
}

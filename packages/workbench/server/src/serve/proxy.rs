//! Forwarding adapter: every request goes to the Node server as it came, with
//! bodies streamed both ways and protocol upgrades bridged.
use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use hyper::upgrade::OnUpgrade;
use hyper_util::{
    client::legacy::{Client, connect::HttpConnector},
    rt::{TokioExecutor, TokioIo},
};

#[derive(Clone)]
struct Upstream {
    port: u16,
    client: Client<HttpConnector, Body>,
}

pub(crate) fn router(port: u16) -> Router {
    let client = Client::builder(TokioExecutor::new()).build_http();
    Router::new()
        .fallback(forward)
        .with_state(Upstream { port, client })
}

async fn forward(State(upstream): State<Upstream>, mut request: Request) -> Response {
    let upgrade = request
        .headers()
        .contains_key(header::UPGRADE)
        .then(|| hyper::upgrade::on(&mut request));
    let target = request
        .uri()
        .path_and_query()
        .map_or("/", |target| target.as_str());
    match format!("http://127.0.0.1:{}{target}", upstream.port).parse::<Uri>() {
        Ok(uri) => *request.uri_mut() = uri,
        Err(_) => return unavailable("The request target is not a valid path."),
    }
    match upstream.client.request(request).await {
        Ok(mut response) => {
            if response.status() == StatusCode::SWITCHING_PROTOCOLS
                && let Some(client) = upgrade
            {
                let server = hyper::upgrade::on(&mut response);
                tokio::spawn(bridge(client, server));
            }
            response.map(Body::new).into_response()
        }
        Err(error) => unavailable(&format!("The Workbench server did not answer: {error}")),
    }
}

/// Copies both directions of an upgraded connection until either side closes.
async fn bridge(client: OnUpgrade, server: OnUpgrade) {
    if let (Ok(client), Ok(server)) = tokio::join!(client, server) {
        let mut client = TokioIo::new(client);
        let mut server = TokioIo::new(server);
        let _closed = tokio::io::copy_bidirectional(&mut client, &mut server).await;
    }
}

fn unavailable(message: &str) -> Response {
    (StatusCode::BAD_GATEWAY, message.to_owned()).into_response()
}

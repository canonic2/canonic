//! The front door: the space's public loopback port, forwarded to the Node
//! server, with the resident page index answered over standard streams.
//!
//! The process exits when its standard input closes, so it never outlives the
//! Node server that started it.
mod messages;
mod proxy;
mod resident;

use anyhow::{Context, Result, bail};
use std::{net::Ipv4Addr, sync::Arc};
use tokio::net::TcpListener;

/// Arguments of `workbench-server serve`.
pub(crate) struct Options {
    /// The Node server's private loopback port.
    upstream: u16,
    /// Public ports to try in order; 0 takes any free port.
    ports: Vec<u16>,
}

impl Options {
    pub(crate) fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut upstream = None;
        let mut ports = vec![0];
        while let Some(flag) = args.next() {
            let value = args
                .next()
                .with_context(|| format!("{flag} needs a value"))?;
            match flag.as_str() {
                "--upstream" => {
                    upstream = Some(value.parse().context("--upstream must be a port")?);
                }
                "--ports" => {
                    ports = value
                        .split(',')
                        .map(|port| port.trim().parse())
                        .collect::<Result<_, _>>()
                        .context("--ports must be a comma-separated list of ports")?;
                }
                _ => bail!("unknown option {flag}"),
            }
        }
        let upstream = upstream.context("serve needs --upstream <port>")?;
        if ports.is_empty() {
            bail!("--ports must name at least one port");
        }
        Ok(Self { upstream, ports })
    }
}

pub(crate) fn run(options: Options) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(serve(options));
    // An index build may still be running on a blocking thread; the process is
    // ending, so it is not waited for.
    runtime.shutdown_background();
    result
}

async fn serve(options: Options) -> Result<()> {
    let listener = bind(&options.ports).await?;
    let port = listener.local_addr()?.port();
    let output = messages::Output::new();
    output.listening(port).await?;
    let resident = Arc::new(resident::Resident::new(output.clone()));
    let app = proxy::router(options.upstream);
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let read = messages::read(resident, output).await;
    server.abort();
    read
}

/// The first public port that is free, on loopback only.
async fn bind(ports: &[u16]) -> Result<TcpListener> {
    let mut last = None;
    for &port in ports {
        match TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
            Ok(listener) => return Ok(listener),
            Err(error) => last = Some(error),
        }
    }
    Err(last.map_or_else(|| anyhow::anyhow!("no port to listen on"), Into::into))
}

#[cfg(test)]
mod tests {
    use super::Options;

    fn parse(args: &[&str]) -> anyhow::Result<Options> {
        Options::parse(args.iter().map(ToString::to_string))
    }

    #[test]
    fn options_need_an_upstream_and_take_ports_in_order() {
        let options = parse(&["--upstream", "4100", "--ports", "3579, 3580,0"]).unwrap();
        assert_eq!(options.upstream, 4100);
        assert_eq!(options.ports, vec![3579, 3580, 0]);
        assert_eq!(parse(&["--upstream", "4100"]).unwrap().ports, vec![0]);
        assert!(parse(&["--ports", "3579"]).is_err());
        assert!(parse(&["--upstream"]).is_err());
        assert!(parse(&["--upstream", "x"]).is_err());
        assert!(parse(&["--upstream", "4100", "--ports", ""]).is_err());
        assert!(parse(&["--upstream", "4100", "--other", "1"]).is_err());
    }
}

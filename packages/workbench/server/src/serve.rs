//! `workbench-server serve`: one space's server. It owns the space's public
//! loopback port and answers everything on it: Workbench's files, the
//! project's, previews from the native compiler, and the space's routes. It
//! keeps the page index resident and asks its host over standard streams for
//! what is the editor's or the capture helper's.
//!
//! The process exits when its standard input closes, so it never outlives
//! the host that started it.
mod files;
mod messages;
mod previews;
mod resident;
mod router;
mod routes;

pub(crate) use messages::Output;
pub(crate) use resident::Resident;

use crate::catalog::{Catalog, Where};
use crate::diagnostics::{Diagnostics, SESSION_BYTES};
use crate::host::Host;
use crate::space::{self, Space};
use anyhow::{Context, Result, bail};
use serde_json::json;
use std::path::PathBuf;
use std::{net::Ipv4Addr, sync::Arc};
use tokio::net::TcpListener;

/// Arguments of `workbench-server serve`.
#[derive(Debug)]
pub(crate) struct Options {
    /// Public ports to try in order; 0 takes any free port.
    ports: Vec<u16>,
    /// The space's project root and Workbench's product folder.
    root: PathBuf,
    product: PathBuf,
    /// The folder of the space's `workbench.yaml` and the space's key in
    /// it, when they are not the root and its first space.
    space_dir: Option<PathBuf>,
    space_key: Option<String>,
    /// This space's id in the host's list of spaces.
    space_id: String,
    /// Project code runs only in a trusted workspace.
    trusted: bool,
    /// Whether the host receives handoffs.
    handoff: bool,
    /// Lists and builds every docs page at startup.
    eager_previews: bool,
    log_limit: usize,
    export_retention_ms: Option<u64>,
    export_max_archive_bytes: Option<u64>,
    view_stale_ms: Option<f64>,
}

impl Options {
    pub(crate) fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut ports = vec![0];
        let (mut root, mut product, mut space_dir, mut space_key, mut space_id) =
            (None, None, None, None, String::new());
        let (mut trusted, mut handoff, mut eager_previews) = (true, false, false);
        let mut log_limit = SESSION_BYTES;
        let (mut export_retention_ms, mut export_max_archive_bytes, mut view_stale_ms) =
            (None, None, None);
        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--untrusted" => {
                    trusted = false;
                    continue;
                }
                "--handoff" => {
                    handoff = true;
                    continue;
                }
                "--eager-previews" => {
                    eager_previews = true;
                    continue;
                }
                _ => {}
            }
            let value = args
                .next()
                .with_context(|| format!("{flag} needs a value"))?;
            let number = || {
                value
                    .parse::<u64>()
                    .with_context(|| format!("{flag} must be a number"))
            };
            match flag.as_str() {
                "--ports" => {
                    ports = value
                        .split(',')
                        .map(|port| port.trim().parse())
                        .collect::<Result<_, _>>()
                        .context("--ports must be a comma-separated list of ports")?;
                }
                "--root" => root = Some(value.into()),
                "--product" => product = Some(value.into()),
                "--space-dir" => space_dir = Some(value.into()),
                "--space-key" => space_key = Some(value),
                "--space-id" => space_id = value,
                "--log-limit" => log_limit = usize::try_from(number()?)?,
                "--export-retention-ms" => export_retention_ms = Some(number()?),
                "--export-max-archive-bytes" => export_max_archive_bytes = Some(number()?),
                "--view-stale-ms" => {
                    view_stale_ms = Some(
                        value
                            .parse::<f64>()
                            .context("--view-stale-ms must be a number")?,
                    );
                }
                _ => bail!("unknown option {flag}"),
            }
        }
        if ports.is_empty() {
            bail!("--ports must name at least one port");
        }
        Ok(Self {
            ports,
            root: root.context("serve needs --root <project>")?,
            product: product.context("serve needs --product <folder>")?,
            space_dir,
            space_key,
            space_id,
            trusted,
            handoff,
            eager_previews,
            log_limit,
            export_retention_ms,
            export_max_archive_bytes,
            view_stale_ms,
        })
    }
}

pub(crate) fn run(options: Options) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    // Rspack keeps per-compiler state in a task-local context; each compiler
    // gets its own inside its builder task.
    let result = runtime.block_on(rspack_tasks::within_compiler_context(
        Arc::new(rspack_tasks::CompilerContext::new()),
        serve(options),
    ));
    // An index build may still be running on a blocking thread; the process is
    // ending, so it is not waited for.
    runtime.shutdown_background();
    result
}

async fn serve(options: Options) -> Result<()> {
    let root = std::path::absolute(&options.root)?;
    let product = std::path::absolute(&options.product)?;
    let listener = bind(&options.ports).await?;
    let port = listener.local_addr()?.port();
    let output = Output::new();
    let host = Arc::new(Host::new(output.clone()));
    let diagnostics = Diagnostics::new(host.clone(), options.log_limit);
    let storybooks = space::Storybooks::default();
    let proxies = crate::proxies::Proxies::new(space::sockets(storybooks.clone()));
    proxies.set_bridge(port);
    let resident = Arc::new(Resident::new(output.clone()));
    let previews = crate::compiler::Previews::new(&root, &product)?;
    let files = Arc::new(files::Files::new(
        &root,
        &product,
        options.space_dir.as_deref(),
        options.space_key.as_deref(),
    ));
    let place = Where {
        dir: options.space_dir.clone().map_or_else(
            || root.clone(),
            |dir| std::path::absolute(dir).unwrap_or_default(),
        ),
        key: options.space_key.clone(),
    };
    let catalog = Catalog::new(
        &root,
        &product,
        place,
        options.trusted,
        resident.clone(),
        previews.clone(),
        host.clone(),
        output.clone(),
    );
    let settings = space::Settings {
        id: options.space_id.clone(),
        handoff: options.handoff,
        export_retention_ms: options.export_retention_ms,
        export_max_archive_bytes: options.export_max_archive_bytes,
        view_stale_ms: options.view_stale_ms,
    };
    let space = Arc::new(Space::new(
        catalog,
        host,
        diagnostics,
        proxies,
        storybooks,
        settings,
        &product,
        port,
        previews.clone(),
    ));
    // The address for agents is written before the host hears the port, so
    // it is there as soon as the space has started.
    if let Err(error) = space.agent.announce(&format!("http://127.0.0.1:{port}/")) {
        space
            .diagnostics
            .warn(
                "view.announce.failed",
                json!({ "message": format!("{error:#}") }),
            )
            .await;
    }
    output.listening(port).await?;
    let app = router::router(space.clone(), files, output.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    space
        .diagnostics
        .info("session.started", json!({ "port": port }))
        .await;
    if options.eager_previews {
        let warming = space.clone();
        tokio::spawn(async move { warming.warm_docs().await });
    }
    let read = messages::read(resident, space.clone(), output).await;
    space
        .diagnostics
        .info("session.stopping", json!({ "port": port }))
        .await;
    server.abort();
    space.agent.close();
    space.close().await;
    previews.close().await;
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
    fn options_need_a_root_and_product_and_take_ports_in_order() {
        let options = parse(&[
            "--root",
            "/acme",
            "--product",
            "/wb",
            "--ports",
            "3579, 3580,0",
            "--untrusted",
            "--handoff",
        ])
        .unwrap();
        assert_eq!(options.ports, vec![3579, 3580, 0]);
        assert!(!options.trusted && options.handoff && !options.eager_previews);
        let defaults = parse(&["--root", "/acme", "--product", "/wb"]).unwrap();
        assert_eq!(defaults.ports, vec![0]);
        assert!(defaults.trusted);
        assert!(parse(&["--product", "/wb"]).is_err());
        assert!(parse(&["--root", "/acme"]).is_err());
        assert!(parse(&["--root", "/acme", "--product", "/wb", "--ports", ""]).is_err());
        assert!(parse(&["--root", "/acme", "--product", "/wb", "--log-limit", "x"]).is_err());
        assert!(parse(&["--root", "/acme", "--product", "/wb", "--other", "1"]).is_err());
    }
}

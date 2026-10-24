//! Docs pages: a page's Markdown served at its path as the page's docs, in
//! the docs lens `?lens=` names, with the bundle a deferred page mounts, the
//! revision an open page polls, and an example's code for Show code. Any
//! other Markdown file is served as it is.
use super::{json_response, query, respond, text, trouble};
use crate::docs::highlight::{background, highlight, language_of};
use crate::docs::service::{DocsRequest, PageLocations};
use crate::docs::{BUNDLE_PATH, HIGHLIGHT_PATH, REVISION_PATH, SOURCE_PATH};
use crate::refused::refused;
use crate::space::Space;
use anyhow::Result;
use axum::{extract::Request, http::Method, response::Response};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Arc;

/// Names the Markdown and the examples' files in a handoff from a docs lens.
pub(super) async fn handoff(space: &Space, canvas: &mut Value) {
    space.docs_handoff(canvas).await;
}

/// What a docs route’s query names: the page, its lens, and an example.
struct Asked {
    page: String,
    lens: Option<String>,
    example: String,
    scenario: Option<String>,
    plain: bool,
}

impl Asked {
    fn of(request: &Request) -> Self {
        Self {
            page: query(request, "page").unwrap_or_default(),
            lens: query(request, "lens"),
            example: query(request, "example").unwrap_or_default(),
            scenario: query(request, "scenario"),
            plain: query(request, "plain").as_deref() == Some("1"),
        }
    }
}

async fn docs_route(space: &Arc<Space>, path: &str, asked: Asked) -> Result<Value> {
    let view = space.docs_ready().await?.unwrap_or(Value::Null);
    let wanted = asked.page;
    let pages = space.docs_pages(&view);
    let page = pages
        .iter()
        .find(|candidate| candidate.src == wanted)
        .ok_or_else(|| refused(404, format!("Not a docs page: {wanted}")))?;
    let lens = asked.lens;
    if path == HIGHLIGHT_PATH {
        let page = page.clone();
        let space = space.clone();
        let blocks = background(move || space.docs.highlight_page(&page)).await??;
        return Ok(json!({ "blocks": blocks }));
    }
    if path == REVISION_PATH {
        return Ok(json!({ "revision": space.docs.revision(page, lens.as_deref()).await? }));
    }
    if path == BUNDLE_PATH {
        return Ok(serde_json::to_value(
            space.docs.bundle_info(page, lens.as_deref()).await?,
        )?);
    }
    let mut code = space
        .docs
        .example_source(page, lens.as_deref(), &asked.example, true)
        .await?;
    if !asked.plain {
        let text = code.text.clone();
        let language = language_of(&code.file);
        code.html = background(move || highlight(&text, Some(&language))).await?;
    }
    Ok(json!({ "file": code.file, "text": code.text, "html": code.html }))
}

/// A docs page's document, or `None` when the Markdown is no docs page.
async fn page(space: &Space, path: &str, asked: Asked) -> Result<Option<String>> {
    let Some(src) = crate::docs::render::decode(path.trim_start_matches('/')) else {
        return Ok(None);
    };
    // Discovered pages are known once the configuration has been resolved;
    // a broken configuration serves the file as it is.
    let view = space
        .docs_ready()
        .await
        .ok()
        .flatten()
        .unwrap_or(Value::Null);
    let pages = space.docs_pages(&view);
    let Some(entry) = pages.iter().find(|candidate| candidate.src == src) else {
        return Ok(None);
    };
    let sources: HashSet<String> = pages.iter().map(|page| page.src.clone()).collect();
    let ask = DocsRequest {
        lens: asked.lens.as_deref(),
        scenario: asked.scenario.as_deref(),
    };
    let html = space
        .docs
        .page(entry, ask, &sources, &PageLocations::default(), true)
        .await?;
    Ok(Some(crate::serve::files::with_compat(&html)))
}

pub(super) async fn answer(
    space: &Arc<Space>,
    path: &str,
    method: &Method,
    request: Request,
) -> Result<Response, Request> {
    if [SOURCE_PATH, REVISION_PATH, BUNDLE_PATH, HIGHLIGHT_PATH].contains(&path) {
        if method != Method::GET {
            return Ok(text(
                405,
                "GET a docs page’s source, revision, or bundle here.",
            ));
        }
        return Ok(match docs_route(space, path, Asked::of(&request)).await {
            Ok(answer) => json_response(200, &answer),
            Err(error) => trouble(&error, 500),
        });
    }
    let markdown = std::path::Path::new(path)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"));
    if markdown && (method == Method::GET || method == Method::HEAD) {
        return match page(space, path, Asked::of(&request)).await {
            Ok(Some(html)) => Ok(respond(
                200,
                if method == Method::HEAD {
                    String::new()
                } else {
                    html
                },
                "text/html; charset=utf-8",
            )),
            Ok(None) => Err(request),
            Err(error) => Ok(trouble(&error, 500)),
        };
    }
    Err(request)
}

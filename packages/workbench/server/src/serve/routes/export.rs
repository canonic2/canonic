//! `/_workbench/export`: the design export as a job. A canvas POSTs the
//! request and polls `?job=`, then downloads the archive or one of its
//! numbered parts; a plain GET runs the export and answers its download.
use super::{base, body, is_get, json_response, parse, query, same_origin, text, trouble};
use crate::export::{Archive, ExportInput, archive_part, read_request, select_view, start};
use crate::refused::refused;
use crate::space::{Space, SpaceExport};
use anyhow::Result;
use axum::{
    extract::Request,
    http::{HeaderValue, Method, header},
    response::Response,
};
use serde_json::{Value, json};
use std::sync::Arc;

fn archive(archive: &Archive, head: bool) -> Response {
    let mut response = super::respond(
        200,
        if head {
            Vec::new()
        } else {
            archive.body.clone()
        },
        archive.content_type,
    );
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from(archive.body.len()),
    );
    if let Ok(disposition) =
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", archive.filename))
    {
        headers.insert(header::CONTENT_DISPOSITION, disposition);
    }
    response
}

fn job(space: &Space, id: &str, download: bool, part: Option<&str>, head: bool) -> Response {
    let Some(job) = space.exports.get(id) else {
        return json_response(
            404,
            &json!({ "ok": false, "error": "That export job is no longer available." }),
        );
    };
    if !download {
        return json_response(200, &job.status());
    }
    let Some(result) = job.result().filter(|_| job.is_complete()) else {
        return json_response(409, &job.status());
    };
    match archive_part(&result, part) {
        Ok(found) => archive(found, head),
        Err(error) => trouble(&error, 400),
    }
}

async fn begin(
    space: &Arc<Space>,
    method: &Method,
    part: Option<String>,
    request: Request,
) -> Result<Response> {
    let post = method == Method::POST;
    let asked = if post {
        let bytes = body(request, 1024 * 1024)
            .await
            .map_err(|error| refused(400, format!("{error:#}")))?;
        if bytes.is_empty() {
            Value::Null
        } else {
            parse(&bytes).map_err(|error| refused(400, format!("{error:#}")))?
        }
    } else {
        Value::Null
    };
    let export_request = read_request(&asked).map_err(|error| refused(400, error))?;
    let view = space
        .view()
        .await?
        .ok_or_else(|| refused(404, "There is no workbench.yaml at the project root."))?;
    let view = select_view(&view, &export_request).map_err(|error| refused(400, error))?;
    let place = space.catalog.place();
    let input = ExportInput {
        root: space.catalog.root().to_path_buf(),
        view,
        base_url: base(space),
        request: export_request,
        manifest: place.dir.join("workbench.yaml"),
        product: Some(space.product.clone()),
        max_archive_bytes: space
            .settings
            .export_max_archive_bytes
            .and_then(|bytes| usize::try_from(bytes).ok()),
    };
    let job = start(&space.exports, Arc::new(SpaceExport(space.clone())), input)?;
    if post {
        return Ok(json_response(202, &job.status()));
    }
    job.wait().await;
    if let Some(error) = job.error() {
        return Err(anyhow::anyhow!(error));
    }
    let result = job
        .result()
        .ok_or_else(|| anyhow::anyhow!("The export finished without an archive."))?;
    Ok(archive(
        archive_part(&result, part.as_deref())?,
        method == Method::HEAD,
    ))
}

pub(super) async fn answer(
    space: &Arc<Space>,
    path: &str,
    method: &Method,
    request: Request,
) -> Result<Response, Request> {
    if path != "/_workbench/export" {
        return Err(request);
    }
    if !is_get(method) && method != Method::POST {
        return Ok(text(405, "POST to start or GET this design-system export."));
    }
    let part = query(&request, "part");
    if let Some(id) = query(&request, "job").filter(|id| !id.is_empty()) {
        let download = query(&request, "download").as_deref() == Some("1");
        return Ok(job(
            space,
            &id,
            download,
            part.as_deref(),
            method == Method::HEAD,
        ));
    }
    if method == Method::POST && !same_origin(request.headers(), space.port) {
        return Ok(json_response(
            403,
            &json!({ "ok": false, "error": "Only this canvas can start an export." }),
        ));
    }
    Ok(match begin(space, method, part, request).await {
        Ok(response) => response,
        Err(error) => trouble(&error, 500),
    })
}

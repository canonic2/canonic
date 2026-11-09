//! Maps bounded, payload-free operation stages to one MCP request's progress sink.
use crate::review::progress::Progress;
use canonic_mcp_kit::stdio::Context;
use std::sync::Arc;

pub(super) fn for_request(context: &Context) -> Option<Progress> {
    bind(context, None)
}

pub(super) fn resume(
    mut review: Arc<crate::review::Context>,
    context: &Context,
) -> Arc<crate::review::Context> {
    if let Some(progress) = bind(context, Some(&review.progress)) {
        Arc::make_mut(&mut review).progress = progress;
    }
    review
}

fn bind(context: &Context, previous: Option<&Progress>) -> Option<Progress> {
    let reporter = context.progress_reporter()?;
    let observe = move |number, stage: crate::review::progress::Stage| {
        // Dependency closures can contain many artifacts. Progress is advisory;
        // bound emission independently of registry size and queue availability.
        reporter.progress(f64::from(number), None, Some(stage.message()));
    };
    Some(match previous {
        Some(previous) => previous.rebind(observe),
        None => Progress::new(observe),
    })
}

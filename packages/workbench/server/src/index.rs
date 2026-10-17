//! Page-index application workflow: coordinate owned stages and retain no global state.
mod cache;
mod definitions;
mod timings;
use crate::{
    discovery::{self, Discovery, Includes},
    evaluate::Evaluator,
    files::stamp,
    model::Request,
    protocol::{self, Response},
    saved::Saved,
};
use anyhow::Result;
use definitions::Definitions;
use std::time::Instant;
use timings::Timings;

/// Answers one request with its own evaluator child.
pub(crate) fn answer(request: &Request) -> Result<Response> {
    let evaluator = Evaluator::new(
        &request.root,
        &request.node,
        &request.evaluator,
        &request.options,
        std::time::Duration::from_millis(request.deadline_ms),
    );
    index(request, &evaluator)
}

pub(crate) fn index(request: &Request, evaluator: &Evaluator<'_>) -> Result<Response> {
    let mut timings = Timings::new();
    let step = Instant::now();
    let previous = Saved::load(&request.saved, &request.identity);
    timings.elapsed("loadMs", step);
    let step = Instant::now();
    let includes = Includes::read(&request.options)?;
    let mut discovered = discovery::discover(
        &request.root,
        &previous.directories,
        request.walk,
        includes.native(),
    )?;
    // Rules change what a reused folder lists, so a changed `.gitignore`
    // counts as a folder read.
    let ignores: Vec<_> = discovered
        .ignores
        .iter()
        .map(|file| (file.clone(), stamp(&request.root.join(file))))
        .collect();
    if ignores != previous.ignores {
        discovered.reads = discovered.reads.max(1);
    }
    timings.elapsed("walkMs", step);
    timings.count("directories", discovered.directories.len());
    timings.count("directoriesRead", discovered.reads);
    if !select(&mut discovered, &includes, &previous, evaluator)? {
        return Ok(Response::waiting(timings.into_json()));
    }
    let files = &discovered.files;
    let step = Instant::now();
    let reused = definitions::reusable(&request.root, files, &previous.records);
    let reused_count = reused.iter().flatten().count();
    timings.elapsed("checkMs", step);
    let config = cache::Settings::check(&request.root, &request.options, &previous);
    if config.reuse_answer(
        &previous,
        discovered.reads,
        files.len(),
        reused_count,
        &request.cached_generation,
    ) {
        timings.unchanged(files.len(), reused_count);
        return Ok(Response::reused(previous.generation, timings.finish()));
    }
    let step = Instant::now();
    let mut definitions = Definitions::load(&request.root, files, reused);
    timings.elapsed("parseMs", step);
    let step = Instant::now();
    let mut settings = previous.settings.clone();
    if !definitions.pending.is_empty() || config.needs_evaluation {
        if !evaluator.available() {
            timings.counts(files.len(), reused_count, definitions.parsed, 0);
            // A partial native selection preserves literal work. A custom
            // selection must remain complete or the next pass could drop files.
            if !includes.custom {
                cache::store(
                    request,
                    definitions.records,
                    discovered.directories,
                    settings,
                    ignores,
                    String::new(),
                    &mut timings,
                );
            }
            return Ok(Response::waiting(timings.finish()));
        }
        let evaluated = evaluator.evaluate(&definitions.pending)?;
        settings = definitions.apply(&request.root, evaluated);
    }
    timings.elapsed("evaluateMs", step);
    let answer = protocol::listed(files, &definitions.records)?;
    let step = Instant::now();
    let generation = if config.changed(&previous, discovered.reads, files.len(), reused_count) {
        let generation = cache::generation()?;
        cache::store(
            request,
            definitions.records,
            discovered.directories,
            settings,
            ignores,
            generation.clone(),
            &mut timings,
        );
        generation
    } else {
        previous.generation
    };
    timings.elapsed("saveMs", step);
    timings.counts(
        files.len(),
        reused_count,
        definitions.parsed,
        definitions.pending.len(),
    );
    Ok(Response::complete(answer, generation, timings.finish()))
}

fn select(
    discovered: &mut Discovery,
    includes: &Includes,
    previous: &Saved,
    evaluator: &Evaluator<'_>,
) -> Result<bool> {
    if includes.custom {
        if discovered.reads == 0 {
            discovered
                .files
                .retain(|file| previous.records.contains_key(file));
        } else if evaluator.available() {
            discovered.files = evaluator.select(&discovered.files)?;
        } else {
            return Ok(false);
        }
    }
    Ok(true)
}

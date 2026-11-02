//! Suggest contextual prompt arguments and resource-template variables over stdio.
use canonic_mcp_kit::{
    Catalog, Client, CompleteResult, CompletionReference, ModernOptions, Prompt, PromptArgument,
    PromptCatalog, ResourceCatalog, ResourceTemplate, RpcError, ServerInfo, rpc,
    stdio::{self, Context, Handler, Outcome, Request},
};
use std::{io, sync::Arc};

struct Acme;

impl Handler for Acme {
    fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
        Ok(ServerInfo::new("acme", "1"))
    }

    fn call(&self, request: Request, context: &Context) -> Outcome {
        let Request::Completion(request) = request else {
            return Outcome::Method(Err(RpcError::new(rpc::METHOD_NOT_FOUND, "Unknown method")));
        };
        // A real provider checks access before reading or exposing private suggestions.
        if context.check().is_err() {
            return Outcome::Done;
        }
        let candidates: &[&str] = match &request.reference {
            CompletionReference::Prompt { .. } if request.argument.name == "language" => {
                &["python", "rust"]
            }
            CompletionReference::Prompt { .. }
                if request
                    .context
                    .get("language")
                    .is_some_and(|language| language == "python") =>
            {
                &["flask", "fastapi"]
            }
            CompletionReference::Resource { .. } => &["report-1", "report-2"],
            CompletionReference::Prompt { .. } => &[],
        };
        // Filtering and relevance order belong to the provider; the library preserves them.
        let values: Vec<String> = candidates
            .iter()
            .filter(|value| value.starts_with(&request.argument.value))
            .map(|value| (*value).into())
            .collect();
        let total = values.len() as u64;
        match CompleteResult::new(values, Some(total), Some(false)) {
            Ok(result) => Outcome::Completion(result),
            Err(error) => {
                Outcome::Method(Err(RpcError::new(rpc::INTERNAL_ERROR, error.to_string())))
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut prompt = Prompt::new("acme_review");
    prompt.arguments = vec![
        PromptArgument::new("language", true),
        PromptArgument::new("framework", false),
    ];
    let mut options = stdio::Options::default();
    options.session.prompts = Some(Arc::new(PromptCatalog::new(vec![prompt])?));
    options.session.resources = Some(Arc::new(ResourceCatalog::new(
        vec![],
        vec![ResourceTemplate::new("acme://reports/{id}", "Reports")],
    )?));
    options.session.completions = true;
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    stdio::serve(
        Arc::new(Acme),
        Arc::new(Catalog::new(vec![])?),
        io::BufReader::new(io::stdin()),
        io::stdout(),
        &options,
    )?;
    Ok(())
}

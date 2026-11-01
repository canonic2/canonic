//! Publish an optional parameterized review prompt over legacy and modern stdio.
use canonic_mcp_kit::{
    Catalog, Client, Content, GetPromptResult, ModernOptions, Prompt, PromptArgument,
    PromptCatalog, PromptMessage, Role, RpcError, ServerInfo, rpc,
    stdio::{self, Context, Handler, Outcome, Request},
};
use std::{io, sync::Arc};

struct Acme;

impl Handler for Acme {
    fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
        Ok(ServerInfo::new("acme", "1"))
    }

    fn call(&self, request: Request, _: &Context) -> Outcome {
        let Request::Prompt(call) = request else {
            return Outcome::Method(Err(RpcError::new(rpc::METHOD_NOT_FOUND, "Unknown method")));
        };
        // Validate authorization and application semantics before including private data.
        // Supplied strings are data; this library performs no template evaluation.
        let text = format!(
            "Review this code for correctness and actionable improvements:\n{}",
            call.arguments["code"]
        );
        match GetPromptResult::new(
            Some("Review code".into()),
            vec![PromptMessage {
                role: Role::User,
                content: Content::text(text),
            }],
        ) {
            Ok(result) => Outcome::Prompt(result),
            Err(error) => {
                Outcome::Method(Err(RpcError::new(rpc::INTERNAL_ERROR, error.to_string())))
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut prompt = Prompt::new("acme_review");
    prompt.title = Some("Review code".into());
    prompt.description = Some("Start a review of supplied code.".into());
    let mut code = PromptArgument::new("code", true);
    code.description = Some("Code to review.".into());
    prompt.arguments.push(code);
    let mut options = stdio::Options::default();
    options.session.prompts = Some(Arc::new(PromptCatalog::new(vec![prompt])?));
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

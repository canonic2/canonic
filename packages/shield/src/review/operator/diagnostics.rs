//! CLI mapping into shared documentation and immutable policy diagnostics.
use crate::{
    cli::public::{DocsCommand, PolicyCommand},
    review::{Context, Error, mcp},
};
use serde_json::{Value, json};

pub(super) fn docs(command: &DocsCommand) -> Result<Value, Error> {
    let (name, args) = match command {
        DocsCommand::List { offset, limit } => {
            ("docs_list", json!({"offset":offset,"limit":limit}))
        }
        DocsCommand::Search {
            query,
            path,
            offset,
            limit,
        } => {
            let mut args = json!({"query":query,"offset":offset,"limit":limit});
            if let Some(path) = path {
                args["path"] = json!(path);
            }
            ("docs_search", args)
        }
        DocsCommand::Read {
            path,
            start_line,
            limit,
        } => (
            "docs_read",
            json!({"path":path,"start_line":start_line,"limit":limit}),
        ),
    };
    mcp::operator_docs(name, &args)
}

pub(super) fn diagnostic(context: &Context, command: &PolicyCommand) -> Result<Value, Error> {
    mcp::operator_diagnostic(
        context,
        match command {
            PolicyCommand::Evaluate { input, proposal_id } => mcp::Diagnostic::Evaluate {
                input,
                proposal_id: proposal_id.as_deref(),
            },
            PolicyCommand::Compare {
                before,
                after,
                domain,
                input,
            } => mcp::Diagnostic::Compare {
                before,
                after,
                domain,
                input: input.as_deref(),
            },
            _ => return Err(Error::Invalid("expected policy evaluate or compare")),
        },
    )
}

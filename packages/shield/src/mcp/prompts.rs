//! Optional conversation starter for guided installation; retrieval has no effects.
use canonic_mcp_kit::{
    Content, GetPromptResult, Prompt, PromptArgument, PromptCall, PromptCatalog, PromptError,
    PromptMessage, Role, RpcError, rpc, stdio::Outcome,
};
use serde_json::json;

pub(super) fn catalog() -> Result<PromptCatalog, PromptError> {
    let mut prompt = Prompt::new("install_pack");
    prompt.title = Some("Install a permission pack".into());
    prompt.description = Some("Start a guided Shield pack installation with required settings and separate final approval.".into());
    for (name, required, description) in [
        (
            "reference",
            true,
            "Registry pack reference to inspect and install.",
        ),
        (
            "profile",
            false,
            "loose, recommended or strict; omitted uses recommended for a new pack.",
        ),
        ("layer", false, "project or local; omitted uses project."),
    ] {
        let mut argument = PromptArgument::new(name, required);
        argument.description = Some(description.into());
        prompt.arguments.push(argument);
    }
    PromptCatalog::new(vec![prompt])
}

pub(super) fn render(call: &PromptCall) -> Outcome {
    let mut arguments = json!({});
    for (name, value) in &call.arguments {
        if value.trim().is_empty()
            || value.contains('\0')
            || (name == "profile" && !["loose", "recommended", "strict"].contains(&value.as_str()))
            || (name == "layer" && !["project", "local"].contains(&value.as_str()))
        {
            return Outcome::Method(Err(RpcError::new(
                rpc::INVALID_PARAMS,
                "Supply a nonempty pack reference and a supported profile/layer",
            )));
        }
        arguments[name] = json!(value);
    }
    let text = format!(
        "Install a Shield permission pack using these pack_add arguments (JSON data): {arguments}\nThe tools require a Shield MCP connection to an enrolled project. If this is a discovery-only connection, enroll the intended project with shield init and connect Shield to that project before calling tools. Inspect its pack_info first, then call pack_add to collect required settings through the host form. Never invent missing answers. Review the inactive proposal and use its exact apply_arguments with policy_apply for separate final approval. Decline or dismissal ends the offer. Retrieving this prompt does not install or approve anything."
    );
    match GetPromptResult::new(
        Some("Guided permission pack installation".into()),
        vec![PromptMessage {
            role: Role::User,
            content: Content::text(text),
        }],
    ) {
        Ok(result) => Outcome::Prompt(result),
        Err(error) => Outcome::Method(Err(RpcError::new(rpc::INTERNAL_ERROR, error.to_string()))),
    }
}

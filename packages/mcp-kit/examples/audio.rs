//! Return a one-second silent WAV and presentation hints through stdio.

use canonic_mcp_kit::{
    Annotations, Catalog, Client, Content, ContentAnnotations, ContentError, ModernOptions, Role,
    RpcError, ServerInfo, Tool, ToolResult, rpc,
    stdio::{self, Context, Handler, Outcome, Request},
};
use serde_json::json;
use std::{io, sync::Arc};

fn audio_result() -> Result<ToolResult, ContentError> {
    // PCM WAV: mono, 8 kHz, 8-bit unsigned samples; 128 is silence.
    let mut wav = b"RIFF\x64\x1f\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x40\x1f\0\0\x40\x1f\0\0\x01\0\x08\0data\x40\x1f\0\0".to_vec();
    wav.resize(44 + 8000, 128);
    let hints = ContentAnnotations::new(Some(vec![Role::User]), Some(0.5), None)?;
    Ok(ToolResult::success(vec![
        Content::text("One second of silent audio."),
        Content::audio(&wav, "audio/wav")?.with_annotations(hints),
    ]))
}

struct Acme;

impl Handler for Acme {
    fn initialize(&self, _: &Client) -> Result<ServerInfo, RpcError> {
        Ok(ServerInfo::new("acme", "1"))
    }

    fn call(&self, request: Request, _: &Context) -> Outcome {
        if !matches!(request, Request::Tool(_)) {
            return Outcome::Method(Err(RpcError::new(rpc::METHOD_NOT_FOUND, "Unknown method")));
        }
        match audio_result() {
            Ok(result) => Outcome::Tool(result),
            Err(error) => {
                Outcome::Method(Err(RpcError::new(rpc::INTERNAL_ERROR, error.to_string())))
            }
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tool = Tool::new(
        "acme_audio",
        "Return a one-second silent WAV sample.",
        json!({"type":"object","properties":{},"additionalProperties":false}),
    )
    .with_annotations(Annotations {
        read_only_hint: Some(true),
        ..Annotations::default()
    });
    let mut options = stdio::Options::default();
    options.session.modern = Some(ModernOptions::new(ServerInfo::new("acme", "1")));
    stdio::serve(
        Arc::new(Acme),
        Arc::new(Catalog::new(vec![tool])?),
        io::BufReader::new(io::stdin()),
        io::stdout(),
        &options,
    )?;
    Ok(())
}

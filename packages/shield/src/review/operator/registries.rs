//! Terminal review and confirmation for registry enrollment changes.
use super::terminal::Terminal;
use crate::{
    cli::public::{Registry, RegistryCommand},
    review::{
        Context, Error,
        registries::{
            BUILTIN, addable, enrollment, global, global_file, merge, project, registration_error,
        },
        storage::Store,
    },
};
use canonic_registry::{acquisition::Fetcher, registrations::Registrations};
use serde_json::{Value, json};
use std::path::Path;

pub(super) fn mutate(
    context: Option<&Context>,
    fetcher: &Fetcher,
    input: &Registry,
    json_mode: bool,
) -> Result<Value, Error> {
    let mut tty = Terminal::required(json_mode)?;
    let global = global()?;
    let meta = context
        .map(|c| crate::review::project::load(c).map(|s| crate::review::project::metadata(&s)))
        .transpose()?;
    let before = meta.as_ref().map_or_else(|| Ok(global.clone()), project)?;
    let mut after = before.clone();
    let (change, alias, source) = match &input.command {
        RegistryCommand::Add { name, location } => {
            addable(&after, name)?;
            let source = enrollment(fetcher, location)?;
            after
                .add(name, source.clone(), BUILTIN)
                .map_err(registration_error)?;
            ("add", name, Some(source))
        }
        RegistryCommand::Remove { name } => {
            let source = after.remove(name, BUILTIN).map_err(registration_error)?;
            ("remove", name, Some(source))
        }
        RegistryCommand::Default { name } => {
            let available = merge(&global, meta.as_ref().map(|_| &before))?;
            after
                .set_default(name, &available)
                .map_err(registration_error)?;
            ("default", name, None)
        }
        RegistryCommand::List => return Err(Error::Invalid("invalid operator command")),
    };
    let scope = if meta.is_some() { "project" } else { "user" };
    // The default each side resolves to, including one inherited from global scope.
    let resolved = |registrations: &Registrations| {
        merge(&global, meta.as_ref().map(|_| registrations))
            .map(|effective| effective.default_alias().to_owned())
    };
    let receipt = json!({"committed":true,"change":change,"alias":alias,"scope":scope,"source":source,
        "default":{"before":resolved(&before)?,"after":resolved(&after)?},"installed_policy":"preserved"});
    let mut review = receipt.clone();
    review["operation"] = json!("registry configuration");
    review
        .as_object_mut()
        .map(|fields| fields.remove("committed"));
    let Some((context, before_meta)) = context.zip(meta) else {
        tty.confirm(&review)?;
        global_file()?
            .replace(&before, &after)
            .map_err(registration_error)?;
        let mut receipt = receipt;
        receipt["registries"] = json!(after.sources);
        return Ok(receipt);
    };
    let mut changed = before_meta.clone();
    changed["sources"] = json!(after.sources);
    changed["default"] = json!(after.default);
    tty.confirm(&review)?;
    Store::open(Path::new(&context.project_root))?.update(context, &|| Ok(()), |current| {
        if crate::review::project::metadata(current) != before_meta {
            return Err(Error::Invalid(
                "registry configuration changed; review again",
            ));
        }
        current.operator = changed.clone();
        let mut receipt = receipt.clone();
        receipt["registries"] = changed["sources"].clone();
        Ok(receipt)
    })
}

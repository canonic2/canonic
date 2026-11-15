//! Terminal review and confirmation for shared registry trust changes.
use super::terminal::Terminal;
use crate::{
    cli::public::{Registry, RegistryCommand},
    review::{
        Context, Error,
        admin::registries::{Change, Prepared},
    },
};
use canonic_registry_kit::acquisition::Fetcher;
use serde_json::Value;

pub(super) fn mutate(
    context: Option<&Context>,
    fetcher: &Fetcher,
    input: &Registry,
    json_mode: bool,
) -> Result<Value, Error> {
    let mut tty = Terminal::required(json_mode)?;
    if let RegistryCommand::Trust { name } = &input.command {
        let context = context.ok_or(Error::Invalid(
            "select a project to review a registry's user-wide delegated trust",
        ))?;
        let prepared = crate::review::registry_trust::Prepared::new(context, name, &|| Ok(()))?;
        tty.confirm(&prepared.review())?;
        return prepared.commit(context, &|| Ok(()));
    }
    let change = match &input.command {
        RegistryCommand::Add { name, location } => Change::Add { name, location },
        RegistryCommand::Remove { name } => Change::Remove { name },
        RegistryCommand::Default { name } => Change::Default { name },
        RegistryCommand::List { .. } | RegistryCommand::Trust { .. } => {
            return Err(Error::Invalid("invalid operator command"));
        }
    };
    let prepared = Prepared::new(context, fetcher, change)?;
    if let Some(receipt) = prepared.committed_result() {
        return Ok(receipt);
    }
    tty.confirm(&prepared.review())?;
    prepared.commit(context, &|| Ok(()))
}

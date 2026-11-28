//! Per-user signing defaults shared by runner setup, CLI and MCP clients.
use crate::error::{Error, Result};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

mod identities;
mod storage;
#[cfg(test)]
pub(crate) mod tests;

type Teams = BTreeMap<String, String>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Selection {
    pub runner_team_id: String,
    pub source: &'static str,
}

pub(crate) fn validate_team(team: &str) -> Result<()> {
    if team.len() == 10
        && team
            .bytes()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        Ok(())
    } else {
        Err(Error::invalid(
            "A ten-character signing team ID is required",
        ))
    }
}

fn choose(requested: Option<&str>, teams: &Teams) -> Result<String> {
    if let Some(team) = requested {
        validate_team(team)?;
        if teams.contains_key(team) {
            return Ok(team.into());
        }
        return Err(Error::new("SIGNING_IDENTITY_REQUIRED", "The selected team has no valid local Apple Development signing identity; repair signing in Xcode Settings > Accounts or change the default with testrun signing setup")
            .details(json!({"runnerTeamId":team,"availableTeamIds":teams.keys().collect::<Vec<_>>(),"availableTeams":teams,"nextAction":"testrun signing setup"})));
    }
    if teams.len() == 1 {
        return Ok(teams.keys().next().unwrap().clone());
    }
    Err(Error::new("SIGNING_TEAM_REQUIRED", if teams.is_empty() {
        "Configure an Apple Development signing identity in Xcode Settings > Accounts, then run testrun signing setup"
    } else {
        "Choose a default with testrun signing setup --runner-team-id TEAM_ID --non-interactive, or supply runnerTeamId for this connection only"
    }).details(json!({"availableTeamIds":teams.keys().collect::<Vec<_>>(),"availableTeams":teams,"nextAction":"testrun signing setup"})))
}

/// Explicit connection overrides do not read or change host preferences.
pub(crate) async fn select(requested: Option<&str>) -> Result<Selection> {
    if let Some(team) = requested {
        validate_team(team)?;
    }
    let teams = identities::discover().await?;
    let requested = requested.map(str::to_owned);
    tokio::task::spawn_blocking(move || {
        storage::Store::host()?.connection(&teams, requested.as_deref())
    })
    .await
    .map_err(|_| Error::new("HOST_IO", "Signing settings task failed"))?
}

/// Set a host default, or validate/reuse it and select a sole candidate.
pub(crate) async fn setup(requested: Option<&str>) -> Result<Selection> {
    if let Some(team) = requested {
        validate_team(team)?;
    }
    resolve(identities::discover().await?, requested.map(str::to_owned)).await
}

async fn resolve(teams: Teams, requested: Option<String>) -> Result<Selection> {
    tokio::task::spawn_blocking(move || {
        storage::Store::host()?.resolve(&teams, requested.as_deref())
    })
    .await
    .map_err(|_| Error::new("HOST_IO", "Signing settings task failed"))?
}

/// Discovery and saved preference without selecting a different account.
pub(crate) async fn inspect() -> Result<Value> {
    let teams = identities::discover().await?;
    tokio::task::spawn_blocking(move || inspect_store(&teams, &storage::Store::host()?))
        .await
        .map_err(|_| Error::new("HOST_IO", "Signing settings task failed"))?
}

fn inspect_store(teams: &Teams, store: &storage::Store) -> Result<Value> {
    let saved = store.saved()?;
    Ok(
        json!({"savedTeamId":saved,"availableTeams":teams,"availableTeamIds":teams.keys().collect::<Vec<_>>(),"settingsPath":store.path}),
    )
}

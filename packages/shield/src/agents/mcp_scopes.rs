//! Bounded read-only observations for adapter-owned MCP scope checks.
use crate::review::Error;
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

/// Read-only, request-owned configuration evidence. Values never serialize into
/// history or host output. A second bounded read guards the authorization handoff.
pub(crate) struct Observation {
    pub(crate) entries: BTreeMap<String, Observed>,
    files: Vec<PathBuf>,
    agent: &'static str,
    root: PathBuf,
    cwd: PathBuf,
}

#[derive(PartialEq, Eq)]
pub(crate) struct Observed {
    pub(crate) transport: Result<Option<crate::mcp_connections::Transport>, &'static str>,
    pub(crate) source: &'static str,
}

impl Observation {
    pub(super) fn new(agent: &'static str, root: &Path, cwd: &Path) -> Self {
        Self {
            entries: BTreeMap::new(),
            files: Vec::new(),
            agent,
            root: root.into(),
            cwd: cwd.into(),
        }
    }

    pub(super) fn read(&mut self, path: &Path) -> Result<Option<String>, Error> {
        if self.files.len() >= 196 {
            return Err(Error::Invalid("MCP discovery scope limit exceeded"));
        }
        crate::diagnostics::trace::count("mcp.discovery.reads", 1);
        let text = read(path)?;
        self.files.push(path.to_owned());
        Ok(text)
    }

    pub(super) fn add(
        &mut self,
        alias: &str,
        transport: Result<Option<crate::mcp_connections::Transport>, Error>,
        source: &'static str,
    ) -> Result<(), Error> {
        if alias == "shield" {
            return Ok(());
        }
        if self.entries.len() >= 64 && !self.entries.contains_key(alias) {
            return Err(Error::Invalid("MCP discovery server limit exceeded"));
        }
        let transport = transport
            .and_then(|transport| {
                if let Some(transport) = &transport {
                    transport.validate_observed()?;
                }
                Ok(transport)
            })
            .map_err(|_| "unsupported or disabled MCP connection settings");
        match self.entries.entry(alias.into()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Observed { transport, source });
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.get_mut().transport =
                    Err("ambiguous MCP server scopes; resolve duplicate aliases");
            }
        }
        Ok(())
    }

    pub(super) fn disable(&mut self, alias: &str) {
        if let Some(entry) = self.entries.get_mut(alias) {
            entry.transport = Err("MCP server is disabled in host settings");
        }
    }

    pub(crate) fn get(&self, alias: &str) -> Result<Option<&Observed>, Error> {
        super::unambiguous_mcp_alias(self.agent, alias, self.entries.keys().map(String::as_str))?;
        let Some(entry) = self.entries.get(alias) else {
            return Ok(None);
        };
        entry
            .transport
            .as_ref()
            .map_err(|reason| Error::Invalid(reason))?;
        Ok(Some(entry))
    }

    /// Resolve an intercepted namespace to exactly one native configuration key.
    pub(crate) fn resolve(&self, namespace: &str) -> Result<Option<(&str, &Observed)>, Error> {
        let mut matches = self.entries.keys().filter(|alias| {
            super::mcp_namespace(self.agent, alias).is_ok_and(|encoded| encoded == namespace)
        });
        let Some(alias) = matches.next() else {
            // Collision evidence is not a supported mapping. A single related
            // unsupported key receives no policy; competing keys and known
            // duplicate scopes, disabled servers or bad transports still refuse.
            for (alias, entry) in &self.entries {
                if !super::mcp_aliases_collide(self.agent, namespace, alias) {
                    continue;
                }
                entry
                    .transport
                    .as_ref()
                    .map_err(|reason| Error::Invalid(reason))?;
                super::noncolliding_mcp_alias(
                    self.agent,
                    alias,
                    self.entries.keys().map(String::as_str),
                )?;
            }
            return Ok(None);
        };
        if matches.next().is_some() {
            return Err(Error::Invalid(
                "ambiguous MCP server aliases; multiple configured servers encode to the requested host namespace",
            ));
        }
        self.get(alias)
            .map(|entry| entry.map(|entry| (alias.as_str(), entry)))
    }

    pub(crate) fn verify(&self) -> Result<(), Error> {
        crate::diagnostics::trace::count("mcp.discovery.revalidations", self.files.len() as u64);
        // Reuse the adapter's projection so history, tips, unrelated projects and
        // formatting changes do not invalidate a connection observation. Missing
        // files are still read again, so new competing aliases are detected.
        let current = super::find(self.agent)
            .ok_or(Error::Invalid("unknown MCP discovery agent"))?
            .discover_mcp(&self.root, &self.cwd)?
            .ok_or(Error::Invalid("MCP discovery unavailable"))?;
        if self.files != current.files || self.entries != current.entries {
            return Err(Error::Invalid(
                "MCP configuration changed during authorization",
            ));
        }
        Ok(())
    }

    pub(crate) fn connections(
        &self,
        managed: &crate::mcp_connections::Connections,
        agent: &str,
    ) -> crate::mcp_connections::Connections {
        let mut connections = managed.clone();
        for connection in connections.values_mut() {
            connection.enabled &= connection
                .agents
                .get(agent)
                .is_some_and(|alias| !super::reserved_mcp_alias(alias));
        }
        for (alias, entry) in &self.entries {
            if managed.values().any(|c| c.agents.get(agent) == Some(alias))
                || self.get(alias).is_err()
            {
                continue;
            }
            let Ok(Some(transport)) = &entry.transport else {
                continue;
            };
            connections.insert(
                format!("host:{alias}"),
                crate::mcp_connections::Connection {
                    enabled: true,
                    local: false,
                    transport: transport.clone(),
                    agents: BTreeMap::from([(agent.into(), alias.clone())]),
                    packs: BTreeMap::new(),
                    command_fields: BTreeMap::new(),
                    effective_fields: BTreeMap::new(),
                },
            );
        }
        connections
    }
}

/// The host-configured server behind one tool namespace, with the connections
/// automatic pack matching evaluates. Matching itself stays with the caller's policy.
pub(crate) struct HostServer {
    pub(crate) alias: String,
    pub(crate) source: &'static str,
    pub(crate) connections: crate::mcp_connections::Connections,
    pub(crate) observation: Observation,
}

impl HostServer {
    /// The connection ID automatic matching assigns to this server.
    pub(crate) fn server(&self) -> String {
        format!("host:{}", self.alias)
    }
}

/// Observe the agent's MCP settings and find the server a tool namespace names.
pub(crate) fn host_server(
    agent: &str,
    root: &Path,
    cwd: &Path,
    namespace: &str,
    managed: &crate::mcp_connections::Connections,
) -> Result<Option<HostServer>, Error> {
    let Some(observation) = super::find(agent)
        .ok_or(Error::Invalid("unknown MCP agent"))?
        .discover_mcp(root, cwd)?
    else {
        return Ok(None);
    };
    let Some((alias, current)) = observation.resolve(namespace)? else {
        return Ok(None);
    };
    let (alias, source) = (alias.to_owned(), current.source);
    let connections = observation.connections(managed, agent);
    Ok(Some(HostServer {
        alias,
        source,
        connections,
        observation,
    }))
}

pub(super) fn read(path: &Path) -> Result<Option<String>, Error> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Error::Invalid("cannot inspect MCP configuration scope")),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 1024 * 1024 {
        return Err(Error::Invalid(
            "MCP configuration scope must be a bounded regular file",
        ));
    }
    let mut text = String::new();
    fs::File::open(path)
        .and_then(|file| file.take(1024 * 1024 + 1).read_to_string(&mut text))
        .map_err(|_| Error::Invalid("cannot read MCP configuration scope"))?;
    if text.len() > 1024 * 1024 {
        return Err(Error::Invalid("MCP configuration scope exceeds limit"));
    }
    Ok(Some(text))
}

pub(super) fn configuration(override_name: &str, default: &str) -> Result<PathBuf, Error> {
    let path = super::working_files::configuration(override_name, default)
        .filter(|path| path.is_absolute())
        .ok_or(Error::Invalid(
            "cannot resolve absolute MCP configuration scope",
        ))?;
    Ok(path)
}

pub(super) fn project_layers(root: &Path, cwd: &Path) -> Result<Vec<PathBuf>, Error> {
    let root = root.canonicalize()?;
    let cwd = cwd.canonicalize()?;
    let relative = cwd
        .strip_prefix(&root)
        .map_err(|_| Error::Invalid("MCP working directory is outside the bound project"))?;
    let mut layers = vec![root.clone()];
    let mut directory = root;
    for part in relative.components() {
        if layers.len() >= 64 {
            return Err(Error::Invalid(
                "MCP configuration scope depth exceeds limit",
            ));
        }
        directory.push(part);
        layers.push(directory.clone());
    }
    Ok(layers)
}

pub(super) fn no_alias<'a>(
    agent: &str,
    alias: &str,
    aliases: impl Iterator<Item = &'a str>,
) -> Result<(), Error> {
    let aliases = aliases.collect::<Vec<_>>();
    if aliases.contains(&alias) {
        return Err(Error::Invalid(
            "bound MCP alias is shadowed by another host scope; resolve the conflict before setup",
        ));
    }
    super::unambiguous_mcp_alias(agent, alias, aliases.into_iter())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transport() -> crate::mcp_connections::Transport {
        serde_json::from_value(
            serde_json::json!({"type":"stdio","command":"acme-browser","args":[],"env":{}}),
        )
        .unwrap()
    }

    #[test]
    fn encoded_aliases_resolve_uniquely_and_doctor_agrees() {
        let temp = assert_fs::TempDir::new().unwrap();
        for (agent, namespace) in [("codex", "acme_browser"), ("claude", "acme-browser")] {
            let mut observation = Observation::new(agent, temp.path(), temp.path());
            observation
                .add("acme-browser", Ok(Some(transport())), "host-project")
                .unwrap();
            assert!(observation.get("acme-browser").unwrap().is_some());
            assert_eq!(
                observation.resolve(namespace).unwrap().unwrap().0,
                "acme-browser"
            );
            observation
                .add("acme_browser", Ok(Some(transport())), "host-user")
                .unwrap();
            if agent == "codex" {
                assert!(observation.resolve(namespace).is_err());
                assert!(observation.get("acme-browser").is_err());
                assert!(observation.get("acme_browser").is_err());
            } else {
                assert_eq!(
                    observation.resolve(namespace).unwrap().unwrap().0,
                    "acme-browser"
                );
            }
        }
    }

    #[test]
    fn unsupported_aliases_and_duplicate_scopes_are_not_reinterpreted() {
        let temp = assert_fs::TempDir::new().unwrap();
        let mut observation = Observation::new("codex", temp.path(), temp.path());
        for alias in ["acme--browser", "Acme", "plugin_browser"] {
            observation
                .add(alias, Ok(Some(transport())), "host-project")
                .unwrap();
            assert!(observation.get(alias).is_err());
        }
        let mut unique = Observation::new("codex", temp.path(), temp.path());
        unique
            .add("Acme", Ok(Some(transport())), "host-project")
            .unwrap();
        assert!(unique.resolve("Acme").unwrap().is_none());
        assert!(unique.resolve("acme").unwrap().is_none());
        unique
            .add("Acme", Ok(Some(transport())), "host-user")
            .unwrap();
        assert!(
            unique.resolve("Acme").is_err(),
            "unsupported spelling cannot hide duplicate scopes"
        );
        observation
            .add("acme_browser", Ok(Some(transport())), "host-project")
            .unwrap();
        observation
            .add("acme_browser", Ok(Some(transport())), "host-user")
            .unwrap();
        assert!(observation.resolve("acme_browser").is_err());
    }
}

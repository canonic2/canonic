//! Registry-indexed integration selection shared by interfaces.
use crate::review::integrations::host_definition::{self, Host};

/// The selection bits of the Box and Hub integrations, above the adapter indexes.
const BOX_BIT: u64 = 1 << 63;
const HUB_BIT: u64 = 1 << 62;
/// Adapter indexes stay below the host bits.
const ADAPTER_LIMIT: usize = 62;

fn host_bit(host: &Host) -> u64 {
    if host.id == host_definition::HUB.id {
        HUB_BIT
    } else {
        BOX_BIT
    }
}

/// Plan-engine selection; no selection authorizes tool execution.
#[derive(Clone, Copy)]
pub(crate) enum IntegrationChoice {
    /// Preserve existing registration, resuming any confirmed repair plan.
    Keep,
    /// Install the agents in this set, indexed by the immutable adapter
    /// registry, with Box and Hub as the highest bits.
    Agents(u64),
    /// Remove selected adapters while preserving the others; zero opens selection.
    RemoveAgents(u64),
}

#[cfg(test)]
impl IntegrationChoice {
    /// Install one registered agent.
    pub(crate) fn agent(id: &str) -> Self {
        Self::Agents(Self::bits(&[id]).expect("registered agent"))
    }
    /// Install the mutually compatible baseline used by shared fixtures.
    /// Copilot remains explicit because it imports incompatible Claude hooks.
    pub(crate) fn all() -> Self {
        let ids: Vec<_> = crate::agents::ADAPTERS
            .iter()
            .map(|a| a.spec().id)
            .filter(|id| *id != "copilot")
            .collect();
        Self::Agents(Self::bits(&ids).expect("registered compatible agents"))
    }
    /// Remove every Shield-owned agent registration.
    pub(crate) fn remove_all() -> Self {
        Self::RemoveAgents(Self::every())
    }
    fn every() -> u64 {
        let ids: Vec<_> = crate::agents::ADAPTERS
            .iter()
            .map(|a| a.spec().id)
            .collect();
        Self::bits(&ids).expect("registered agents")
    }
}

impl IntegrationChoice {
    pub(crate) fn contains(self, agent: &str) -> bool {
        match self {
            Self::Agents(bits) | Self::RemoveAgents(bits) => {
                if let Some(host) = host_definition::find(agent) {
                    return bits & host_bit(host) != 0;
                }
                crate::agents::ADAPTERS
                    .iter()
                    .position(|a| a.spec().id == agent)
                    .is_some_and(|i| i < ADAPTER_LIMIT && bits & (1 << i) != 0)
            }
            Self::Keep => false,
        }
    }

    /// The selected agent manager, Hub or Box; a selection names at most one.
    pub(crate) fn host(self) -> Option<&'static Host> {
        host_definition::HOSTS
            .into_iter()
            .find(|host| self.contains(host.id))
    }

    /// Refuse an installation that combines Hub with Box or with native
    /// agents: those routes would publish competing Shield entries to the
    /// same agents. Box keeps its earlier hand-over alongside native agents.
    pub(crate) fn check_routes(self) -> Result<(), crate::review::Error> {
        use crate::review::Error;
        let Self::Agents(bits) = self else {
            return Ok(());
        };
        if bits & (HUB_BIT | BOX_BIT) == HUB_BIT | BOX_BIT {
            return Err(Error::Invalid(
                "choose hub or box, not both; each publishes its own Shield entries to the same agents",
            ));
        }
        if bits & HUB_BIT != 0 && bits & !(HUB_BIT | BOX_BIT) != 0 {
            return Err(Error::Invalid(
                "choose only --agent hub, or native agents without Hub; both routes would publish competing Shield entries",
            ));
        }
        Ok(())
    }

    /// The registry-indexed set of `agents`.
    pub(crate) fn bits(agents: &[impl AsRef<str>]) -> Result<u64, crate::review::Error> {
        use crate::review::Error;
        let mut bits = 0u64;
        for id in agents {
            if let Some(host) = host_definition::find(id.as_ref()) {
                bits |= host_bit(host);
                continue;
            }
            let index = crate::agents::ADAPTERS
                .iter()
                .position(|a| a.spec().id == id.as_ref())
                .filter(|index| *index < ADAPTER_LIMIT)
                .ok_or(Error::Invalid(
                    "unknown Shield integration; select a registered agent, Hub or Box",
                ))?;
            bits |= 1u64
                .checked_shl(
                    u32::try_from(index).map_err(|_| Error::Invalid("too many agent adapters"))?,
                )
                .ok_or(Error::Invalid("too many agent adapters"))?;
        }
        Ok(bits)
    }
}

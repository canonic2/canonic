//! Registry-indexed integration selection shared by interfaces.
/// The selection bit of the Box integration, apart from the adapter indexes.
const BOX_BIT: u64 = 1 << 63;

/// Plan-engine selection; no selection authorizes tool execution.
#[derive(Clone, Copy)]
pub(crate) enum IntegrationChoice {
    /// Preserve existing registration, resuming any confirmed repair plan.
    Keep,
    /// Install the agents in this set, indexed by the immutable adapter
    /// registry, with Box as the highest bit.
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
    /// Install every registered agent.
    pub(crate) fn all() -> Self {
        Self::Agents(Self::every())
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
            Self::Agents(bits) | Self::RemoveAgents(bits)
                if agent == crate::review::integrations::box_definition::ID =>
            {
                bits & BOX_BIT != 0
            }
            Self::Agents(bits) | Self::RemoveAgents(bits) => crate::agents::ADAPTERS
                .iter()
                .position(|a| a.spec().id == agent)
                .is_some_and(|i| i < 63 && bits & (1 << i) != 0),
            Self::Keep => false,
        }
    }

    /// The registry-indexed set of `agents`.
    pub(crate) fn bits(agents: &[impl AsRef<str>]) -> Result<u64, crate::review::Error> {
        use crate::review::Error;
        let mut bits = 0u64;
        for id in agents {
            if id.as_ref() == crate::review::integrations::box_definition::ID {
                bits |= BOX_BIT;
                continue;
            }
            let index = crate::agents::ADAPTERS
                .iter()
                .position(|a| a.spec().id == id.as_ref())
                .filter(|index| *index < 63)
                .ok_or(Error::Invalid(
                    "unknown Shield integration; select a registered agent or Box",
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

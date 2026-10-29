//! Compact invocation dispatch over persisted body postings and indexed member data.
use super::{Plan, query};
use crate::policy::{Error, Pack, Profile, Request, invocations as shared, rules::Rule};
use std::collections::BTreeSet;

/// Posting kinds of shared invocation bodies: exact argv, argv prefix, and
/// broad. Each key leads with the executable, then owner, group and set, so a
/// request finds its dispatch bindings from the body postings themselves.
const KINDS: [u8; 3] = [2, 3, 4];

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Binding {
    group: String,
    set: Option<String>,
    executable: String,
}
impl Binding {
    pub(super) fn from_rule(rule: &Rule) -> Option<Self> {
        Some(Self {
            group: rule.invocation.clone()?,
            set: rule.command_set.clone(),
            executable: rule.executable()?.into(),
        })
    }
    /// Key prefix of this binding's bodies of one posting kind.
    fn query(&self, kind: u8, owner: usize) -> Vec<u8> {
        let mut key = query(kind, &self.executable);
        key.extend(owner.to_string().as_bytes());
        key.push(0);
        key.extend(self.group.as_bytes());
        key.push(0);
        key.extend(self.set.as_deref().unwrap_or_default().as_bytes());
        key.push(0);
        key
    }
    /// The owner and binding that lead one body posting key.
    fn from_key(key: &[u8]) -> Result<(usize, Self), Error> {
        let invalid = || Error::Contract("invalid compiled invocation binding");
        let mut fields = key
            .get(1..)
            .ok_or_else(invalid)?
            .splitn(5, |byte| *byte == 0);
        let mut field = || {
            fields
                .next()
                .and_then(|field| std::str::from_utf8(field).ok())
                .ok_or_else(invalid)
        };
        let (executable, owner, group, set) = (field()?, field()?, field()?, field()?);
        if fields.next().is_none() || executable.is_empty() || group.is_empty() {
            return Err(invalid());
        }
        let binding = Self {
            group: group.into(),
            set: (!set.is_empty()).then(|| set.into()),
            executable: executable.into(),
        };
        // Only the canonical spelling sorts where `query` places it.
        let index: usize = owner.parse().map_err(|_| invalid())?;
        if index.to_string() != owner {
            return Err(invalid());
        }
        Ok((index, binding))
    }
}

pub(super) fn key(rule: &Rule, plain: Vec<u8>, owner: usize) -> Vec<u8> {
    let Some(binding) = Binding::from_rule(rule) else {
        return plain;
    };
    let kind = match plain[0] {
        0 => 2,
        1 => 3,
        _ => 4,
    };
    let mut key = binding.query(kind, owner);
    if kind != 4 {
        // The namespace already contains the executable; retain only argv.
        if let Some(offset) = plain[1..].iter().position(|b| *b == 0) {
            key.extend(&plain[offset + 2..]);
        }
    }
    if key.len() > 65_536 {
        binding.query(4, owner)
    } else {
        key
    }
}

impl Plan {
    /// Distinct owner bindings of one executable, read from its body postings.
    /// Each binding costs one bounded search per kind, not a scan of its bodies.
    fn bindings(
        &self,
        executable: &str,
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<BTreeSet<(usize, Binding)>, Error> {
        let mut found = BTreeSet::new();
        for kind in KINDS {
            let prefix = query(kind, executable);
            let mut end = prefix.clone();
            *end.last_mut().ok_or(Error::Syntax)? = 1;
            let (mut position, last) = (self.bound(&prefix, false)?, self.bound(&end, false)?);
            while position < last {
                checkpoint()?;
                let entry = self.entry(position)?;
                let (owner, binding) = Binding::from_key(&self.bytes[entry.start..entry.key_end])?;
                if owner != entry.owner || binding.executable != executable {
                    return Err(Error::Contract("invalid compiled invocation binding"));
                }
                let mut next = binding.query(kind, owner);
                *next.last_mut().ok_or(Error::Syntax)? = 1;
                let after = self.bound(&next, false)?;
                if after <= position {
                    return Err(Error::Contract("invalid compiled invocation binding"));
                }
                position = after;
                found.insert((owner, binding));
            }
        }
        Ok(found)
    }
    pub(super) fn collect_invocations(
        &self,
        request: &Request,
        packs: &mut [(Pack, Profile)],
        checkpoint: &dyn Fn() -> Result<(), Error>,
    ) -> Result<(), Error> {
        let mut budget = shared::Budget::default();
        // Only this executable's bindings; dispatch keeps grammar, owner and
        // binding order, and parses each distinct grammar once.
        let mut bound = Vec::new();
        for (owner, binding) in self.bindings(&request.executable, checkpoint)? {
            let document = &self
                .metadata
                .packs
                .get(owner)
                .ok_or(Error::Contract("invalid compiled posting owner"))?
                .document;
            let segments = &document
                .invocations
                .get(&binding.group)
                .ok_or(Error::Contract("unknown compiled invocation"))?
                .segments;
            if binding
                .set
                .as_ref()
                .is_some_and(|set| !document.command_sets.contains_key(set))
            {
                return Err(Error::Contract("unknown compiled command set"));
            }
            bound.push((segments, owner, binding));
        }
        bound.sort();
        let mut parsed: Option<(&shared::Segments, Vec<shared::Route>)> = None;
        for (segments, owner, binding) in &bound {
            if parsed.as_ref().is_none_or(|(seen, _)| seen != segments) {
                parsed = Some((
                    segments,
                    shared::routes(segments, &request.arguments, checkpoint)?,
                ));
            }
            let routes = parsed.as_ref().map_or(&[][..], |(_, r)| r.as_slice());
            for route in routes {
                let (offset, member) = if let Some(set) = &binding.set {
                    let Some(command) = request.arguments.get(route.offset) else {
                        continue;
                    };
                    let Some(id) =
                        self.metadata.packs[*owner].document.command_sets[set].get(command)
                    else {
                        continue;
                    };
                    (route.offset + 1, Some(id.as_str()))
                } else {
                    (route.offset, None)
                };
                let mut key = binding.query(2, *owner);
                for length in offset..=request.arguments.len() {
                    self.collect_invocation(
                        &key,
                        *owner,
                        *route,
                        &request.arguments[..offset],
                        member,
                        packs,
                        checkpoint,
                        &mut budget,
                    )?;
                    if let Some(argument) = request.arguments.get(length) {
                        key.extend(argument.as_bytes());
                        key.push(0);
                    }
                }
                key[0] = 3;
                self.collect_invocation(
                    &key,
                    *owner,
                    *route,
                    &request.arguments[..offset],
                    member,
                    packs,
                    checkpoint,
                    &mut budget,
                )?;
                self.collect_invocation(
                    &binding.query(4, *owner),
                    *owner,
                    *route,
                    &request.arguments[..offset],
                    member,
                    packs,
                    checkpoint,
                    &mut budget,
                )?;
            }
        }
        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "dispatch carries one checked owner, route and original argv prefix to the shared materializer"
    )]
    fn collect_invocation(
        &self,
        key: &[u8],
        owner: usize,
        route: shared::Route,
        prefix: &[String],
        member: Option<&str>,
        packs: &mut [(Pack, Profile)],
        checkpoint: &dyn Fn() -> Result<(), Error>,
        budget: &mut shared::Budget,
    ) -> Result<(), Error> {
        for position in self.bound(key, false)?..self.bound(key, true)? {
            checkpoint()?;
            let entry = self.entry(position)?;
            if entry.owner != owner {
                continue;
            }
            budget.consume((owner, position), entry.end - entry.key_end, prefix)?;
            let rule: Rule = serde_json::from_slice(&self.bytes[entry.key_end..entry.end])
                .map_err(|_| Error::Syntax)?;
            let group = rule
                .invocation
                .as_ref()
                .and_then(|name| self.metadata.packs[owner].document.invocations.get(name))
                .ok_or(Error::Contract("missing invocation binding"))?;
            packs[owner]
                .0
                .document
                .rules
                .push(shared::materialize(&rule, group, route, prefix, member));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Binding;

    #[test]
    fn body_keys_accept_only_the_canonical_owner_spelling() {
        let (owner, binding) = Binding::from_key(b"\x02acme\x0012\0globals\0\0").unwrap();
        assert_eq!(owner, 12);
        assert_eq!(binding.query(2, owner), b"\x02acme\x0012\0globals\0\0");
        // These parse to an owner, but `query` would rebuild a different
        // prefix, so a dispatch scan could not move past them.
        for owner in ["00", "012", "+1"] {
            let mut key = b"\x02acme\0".to_vec();
            key.extend(owner.as_bytes());
            key.extend(b"\0globals\0\0");
            assert!(Binding::from_key(&key).is_err(), "{owner}");
        }
    }
}

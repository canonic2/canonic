//! Resolves ordered host-loaded remaps and retains visible conflict evidence.

use super::{
    Command, Error,
    bindings::{Binding, Dispatcher},
};
use std::{collections::HashMap, time::Duration};

/// Source evidence supplied by the host; never read or opened here.
#[derive(Clone, Debug)]
pub struct Origin {
    /// Displayable source path.
    pub path: String,
    /// One-based line.
    pub line: usize,
}
/// One ordered override. Later entries win within the supplied scope map.
#[derive(Clone, Debug)]
pub struct Remap {
    /// Command identity.
    pub id: String,
    /// Replacement shortcuts; empty means explicitly unbound.
    pub bindings: Vec<Binding>,
    /// Source evidence.
    pub origin: Origin,
}
/// Keyboard-help detail retained beside effective descriptors.
#[derive(Clone, Debug)]
pub struct KeyInfo {
    /// Bindings before remapping.
    pub defaults: Vec<Binding>,
    /// Latest remap's source.
    pub origin: Origin,
    /// Conflict resolution, including ignored reserved keys.
    pub conflicts: Vec<String>,
    /// Losing or rejected shortcuts shown in help only; never used for dispatch.
    pub ignored: Vec<Binding>,
    /// Whether an override actually changed the effective key.
    pub custom: bool,
}
/// Atomic resolution output shared by dispatch and presentation.
pub struct EffectiveMap {
    /// Effective rows, including unbound conflict losers.
    pub commands: Vec<Command>,
    /// Visible per-command source and conflict details.
    pub metadata: HashMap<String, KeyInfo>,
}
/// Resolve overrides in host-defined precedence order without filesystem I/O.
///
/// # Errors
/// Rejects invalid base maps and unknown command identities.
pub fn resolve(mut commands: Vec<Command>, changes: &[Remap]) -> Result<EffectiveMap, Error> {
    Dispatcher::new(&commands, Duration::from_secs(3))?;
    let mut metadata: HashMap<String, KeyInfo> = HashMap::new();
    for change in changes {
        let at = commands
            .iter()
            .position(|c| c.id == change.id)
            .ok_or_else(|| Error::Binding(format!("unknown command {}", change.id)))?;
        let info = metadata
            .entry(change.id.clone())
            .or_insert_with(|| KeyInfo {
                defaults: commands[at].bindings.clone(),
                origin: change.origin.clone(),
                conflicts: vec![],
                ignored: vec![],
                custom: false,
            });
        info.origin = change.origin.clone();
        let mut candidate = commands[at].clone();
        candidate.bindings.clone_from(&change.bindings);
        if let Err(error) = Dispatcher::new(&[candidate], Duration::from_secs(3)) {
            info.conflicts.push(format!("ignored: {error}"));
            info.ignored.extend(change.bindings.clone());
            continue;
        }
        for (i, command) in commands.iter_mut().enumerate() {
            if i == at {
                continue;
            }
            let collisions: Vec<_> = command
                .bindings
                .iter()
                .filter(|b| {
                    change
                        .bindings
                        .iter()
                        .any(|n| b.0.starts_with(&n.0) || n.0.starts_with(&b.0))
                })
                .cloned()
                .collect();
            if collisions.is_empty() {
                continue;
            }
            let prior = metadata.get(&command.id).map_or("default".into(), |m| {
                format!("{}:{}", m.origin.path, m.origin.line)
            });
            let note = format!(
                "{} wins over {} · {}:{} over {prior}",
                change.id, command.id, change.origin.path, change.origin.line
            );
            let loser = metadata
                .entry(command.id.clone())
                .or_insert_with(|| KeyInfo {
                    defaults: command.bindings.clone(),
                    origin: change.origin.clone(),
                    conflicts: vec![],
                    ignored: vec![],
                    custom: false,
                });
            loser.conflicts.push(format!("overridden · {note}"));
            loser.ignored.extend(collisions.iter().cloned());
            command.bindings.retain(|b| !collisions.contains(b));
            if let Some(info) = metadata.get_mut(&change.id) {
                info.conflicts.push(note);
            }
        }
        commands[at].bindings.clone_from(&change.bindings);
        if let Some(info) = metadata.get_mut(&change.id) {
            info.custom = true;
        }
    }
    Ok(EffectiveMap { commands, metadata })
}

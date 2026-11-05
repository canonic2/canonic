//! Captured pack installation and required settings, independent of presentation.
use crate::{
    policy,
    review::{Context, Error, changes, inspection},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::review) struct Selection {
    pub(in crate::review) reference: String,
    pub(in crate::review) profile: Option<policy::Profile>,
    #[serde(default)]
    pub(in crate::review) variables: Vec<String>,
}

pub(in crate::review) struct Setting {
    pub(in crate::review) key: String,
    pub(in crate::review) name: String,
    pub(in crate::review) pack: String,
    pub(in crate::review) declaration: Value,
    index: usize,
}

pub(in crate::review) struct Installation {
    pub(in crate::review) settings: Vec<Setting>,
    pub(in crate::review) revision: String,
    pub(in crate::review) changes: Vec<Value>,
    binding: String,
    candidate: Value,
    metadata: Value,
    local: bool,
}

impl Installation {
    pub(in crate::review) fn prepare(
        context: &Context,
        selections: &[Selection],
        local: bool,
    ) -> Result<Self, Error> {
        if !(1..=16).contains(&selections.len()) {
            return Err(Error::Invalid("select 1-16 packs"));
        }
        let mut before = crate::review::project::load(context)?;
        let revision = before.policy_revision.clone();
        let binding = changes::prepared_binding(&before);
        let original = before.candidates["project"].clone();
        let original_meta = before.operator.clone();
        let mut requested = BTreeMap::new();
        let mut changes = Vec::new();
        for selection in selections {
            let (candidate, metadata) = super::prepare_add(
                context,
                &before,
                &selection.reference,
                selection.profile,
                false,
                |_, _| Ok(policy::Profile::Recommended),
            )?;
            let rows = metadata["installed"]
                .as_array()
                .ok_or(Error::Invalid("installed packs missing"))?;
            let index = super::addition_selection(&metadata, &selection.reference)?;
            let selector = rows[index]["selector"]
                .as_str()
                .ok_or(Error::Invalid("pack selector missing"))?
                .to_owned();
            if local
                && selection.profile.is_some()
                && original_meta["installed"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|row| row["selector"] == selector)
            {
                return Err(Error::Invalid(
                    "the pack is already installed; a local installation cannot change its profile",
                ));
            }
            let mut names = BTreeSet::new();
            for name in &selection.variables {
                if !names.insert(name.clone()) {
                    return Err(Error::Invalid("select each variable only once"));
                }
            }
            if requested.insert(selector, names).is_some() {
                return Err(Error::Invalid("select each pack only once"));
            }
            let mut change = json!({"kind":"pack_add","reference":selection.reference});
            if let Some(profile) = selection.profile {
                change["profile"] = json!(profile);
            }
            if local {
                change["layer"] = json!("local");
            }
            changes.push(change);
            before.candidates.insert("project".into(), candidate);
            before.operator = metadata;
        }
        let candidate = before
            .candidates
            .insert("project".into(), original)
            .ok_or(Error::Invalid("project candidate missing"))?;
        let metadata = std::mem::replace(&mut before.operator, original_meta);
        let (candidate, metadata) = super::add_layer(&before, candidate, metadata, local)?;
        let settings = collect_settings(&candidate, &metadata, &requested)?;
        Ok(Self {
            settings,
            revision,
            changes,
            binding,
            candidate,
            metadata,
            local,
        })
    }

    pub(in crate::review) fn complete(&self, values: &Value) -> Result<changes::Prepared, Error> {
        let values = values
            .as_object()
            .ok_or(Error::Invalid("invalid installation settings"))?;
        if values.len() != self.settings.len()
            || self.settings.iter().any(|s| !values.contains_key(&s.key))
        {
            return Err(Error::Invalid(
                "answer every installation setting exactly once",
            ));
        }
        let mut metadata = self.metadata.clone();
        for (index, row) in metadata["installed"]
            .as_array_mut()
            .ok_or(Error::Invalid("installed packs missing"))?
            .iter_mut()
            .enumerate()
        {
            let selected: BTreeMap<_, _> = self
                .settings
                .iter()
                .filter(|s| s.index == index)
                .map(|s| (s.name.clone(), values[&s.key].clone()))
                .collect();
            if selected.is_empty() {
                continue;
            }
            let artifact = self.candidate["packs"][index]
                .as_str()
                .ok_or(Error::Invalid("pack bytes missing"))?;
            let mut settings = inspection::selection_settings(&self.candidate, index)?;
            settings.edit(self.local, |binding| {
                super::settings::assign(artifact, binding, &selected, &[])
            })?;
            super::bind(row, &settings);
        }
        let artifacts: Vec<String> = serde_json::from_value(self.candidate["packs"].clone())
            .map_err(|_| Error::Invalid("pack artifacts missing"))?;
        let rows = metadata["installed"]
            .as_array_mut()
            .ok_or(Error::Invalid("installed packs missing"))?;
        let candidate = super::rebuild(&self.candidate, rows, &artifacts, None)?;
        super::unbind(rows);
        Ok(changes::Prepared {
            binding: self.binding.clone(),
            candidate,
            metadata,
        })
    }
}

fn collect_settings(
    candidate: &Value,
    metadata: &Value,
    requested: &BTreeMap<String, BTreeSet<String>>,
) -> Result<Vec<Setting>, Error> {
    // Pending interactions retain source bytes in memory, never an unbounded bundle.
    let bytes = candidate
        .as_object()
        .into_iter()
        .flatten()
        .map(|(_, v)| {
            v.as_str().map_or_else(
                || {
                    v.as_array().map_or(0, |items| {
                        items.iter().filter_map(Value::as_str).map(str::len).sum()
                    })
                },
                str::len,
            )
        })
        .sum::<usize>();
    if bytes > 4 * 1024 * 1024 {
        return Err(Error::Invalid(
            "guided installation retains at most 4 MiB of source; use shield pack add in the terminal",
        ));
    }
    let mut settings = Vec::new();
    for (index, row) in metadata["installed"]
        .as_array()
        .ok_or(Error::Invalid("installed packs missing"))?
        .iter()
        .enumerate()
    {
        let artifact = candidate["packs"][index]
            .as_str()
            .ok_or(Error::Invalid("pack bytes missing"))?;
        let binding = inspection::selection_settings(candidate, index)?.effective();
        let declarations = policy::ordered_declarations(artifact)?;
        let requested = row["selector"]
            .as_str()
            .and_then(|selector| requested.get(selector));
        if requested.is_some_and(|names| {
            names
                .iter()
                .any(|name| !declarations.iter().any(|(declared, _)| declared == name))
        }) {
            return Err(Error::Invalid(
                "unknown variable; inspect pack_info settings before installation",
            ));
        }
        for (name, declaration) in declarations {
            if (declaration.required() && !binding.variables.contains_key(&name))
                || requested.is_some_and(|names| names.contains(&name))
            {
                settings.push(Setting {
                    key: format!("setting_{}", settings.len()),
                    name,
                    pack: row["id"]
                        .as_str()
                        .ok_or(Error::Invalid("pack identity missing"))?
                        .into(),
                    declaration: json!(declaration),
                    index,
                });
            }
        }
    }
    Ok(settings)
}

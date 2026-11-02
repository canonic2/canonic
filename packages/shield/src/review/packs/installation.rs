//! Captured pack installation and required settings, independent of presentation.
use crate::{
    policy,
    review::{Context, Error, changes, inspection},
};
use serde_json::{Value, json};

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
    binding: String,
    candidate: Value,
    metadata: Value,
    local: bool,
}

impl Installation {
    pub(in crate::review) fn prepare(
        context: &Context,
        reference: &str,
        profile: Option<policy::Profile>,
        local: bool,
    ) -> Result<Self, Error> {
        let before = crate::review::project::load(context)?;
        let (candidate, metadata) =
            super::prepare_add(context, &before, reference, profile, local, |_, _| {
                Ok(policy::Profile::Recommended)
            })?;
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
            let binding = inspection::selection_settings(&candidate, index)?.effective();
            for (name, declaration) in policy::ordered_declarations(artifact)? {
                if declaration.required() && !binding.variables.contains_key(&name) {
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
        Ok(Self {
            settings,
            revision: before.policy_revision.clone(),
            binding: changes::prepared_binding(&before),
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
            let selected: std::collections::BTreeMap<_, _> = self
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

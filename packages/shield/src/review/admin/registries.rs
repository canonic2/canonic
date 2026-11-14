//! Reviewed registry trust changes, independent of presentation and transport.
use crate::review::{
    Checkpoint, Context, Error, project,
    registries::{BUILTIN, addable, enrollment, global_file, merge, registration_error},
    storage::Store,
};
use canonic_registry_kit::{
    acquisition::Fetcher,
    registrations::{GlobalFile, Publication, Registrations, Snapshot},
};
use serde_json::{Value, json};
use std::path::Path;

#[derive(Clone, Copy)]
pub(in crate::review) enum Change<'a> {
    Add { name: &'a str, location: &'a str },
    Remove { name: &'a str },
    Default { name: &'a str },
}

pub(in crate::review) struct Prepared {
    root: Option<String>,
    global: Registrations,
    before: Registrations,
    after: Registrations,
    metadata: Option<Value>,
    receipt: Value,
    snapshot: Option<Snapshot>,
    reconciled: bool,
    file: Option<GlobalFile>,
}

impl Prepared {
    pub(in crate::review) fn new(
        context: Option<&Context>,
        fetcher: &Fetcher,
        change: Change<'_>,
    ) -> Result<Self, Error> {
        Self::at(context, fetcher, change, global_file()?)
    }

    fn at(
        context: Option<&Context>,
        fetcher: &Fetcher,
        change: Change<'_>,
        file: GlobalFile,
    ) -> Result<Self, Error> {
        let recovered = file.recover().map_err(registration_error)?;
        let snapshot = file.snapshot().map_err(registration_error)?;
        let global = snapshot.registrations().clone();
        let intent = intent(change)?;
        if context.is_none() {
            if let Some(publication) = &recovered
                && publication.receipt["intent"] == intent
                && (publication.resumed || !matches!(change, Change::Add { .. }))
            {
                return Ok(Self::replayed(snapshot, publication, file));
            }
            if !snapshot.projection_ready() {
                return Err(Error::Invalid(
                    "global registry projection has conflicting manual edits; preserve them and reconcile ~/.shield/registries.json with the committed registrations before retrying",
                ));
            }
        }
        // A project's registrations are the `registries` key of its files.
        let metadata = context
            .map(|c| project::checkpoint(c).map(|state| json!(state.settings.registries)))
            .transpose()?;
        let mut plan = Self::captured(
            context.map(|c| c.project_root.clone()),
            global,
            metadata,
            fetcher,
            change,
        )?;
        if context.is_none()
            && let Some(publication) = &recovered
            && publication.operation == operation(&plan.receipt)
        {
            return Ok(Self::replayed(snapshot, publication, file));
        }
        plan.snapshot = Some(snapshot);
        plan.file = Some(file);
        Ok(plan)
    }

    fn captured(
        root: Option<String>,
        global: Registrations,
        metadata: Option<Value>,
        fetcher: &Fetcher,
        change: Change<'_>,
    ) -> Result<Self, Error> {
        let before = metadata.as_ref().map_or_else(
            || Ok(global.clone()),
            |registries| {
                serde_json::from_value(registries.clone())
                    .map_err(|_| Error::Invalid("invalid project sources"))
            },
        )?;
        let mut after = before.clone();
        let intent = intent(change)?;
        let (kind, alias, source) = match change {
            Change::Add { name, location } => {
                addable(&after, name)?;
                let source = enrollment(fetcher, location)?;
                let replacing = after.sources.remove(name).is_some();
                after
                    .add(name, source.clone(), BUILTIN)
                    .map_err(registration_error)?;
                (
                    if replacing { "replace" } else { "add" },
                    name,
                    Some(source),
                )
            }
            Change::Remove { name } => {
                let source = after.remove(name, BUILTIN).map_err(registration_error)?;
                ("remove", name, Some(source))
            }
            Change::Default { name } => {
                after
                    .set_default(name, &merge(&global, metadata.as_ref().map(|_| &before))?)
                    .map_err(registration_error)?;
                ("default", name, None)
            }
        };
        let resolved = |registrations: &Registrations| {
            if metadata.is_some() {
                merge(&global, Some(registrations))
            } else {
                merge(registrations, None)
            }
        };
        let before_effective = resolved(&before)?;
        let after_effective = resolved(&after)?;
        let effective_source = |effective: &canonic_registry_kit::registrations::Effective| {
            effective.get(alias).map(|source| {
                json!({
                    "source":source,"scope":effective.scope(alias),"trust":trust_review(source)
                })
            })
        };
        let previous = if kind == "replace" {
            before.sources.get(alias)
        } else {
            None
        };
        let receipt = json!({"committed":true,"change":kind,"alias":alias,"scope":if metadata.is_some() {"project"} else {"user"},"source":source,
            "trust":source.as_ref().map(trust_review),
            "intent":intent,
            "previous_source":previous,"previous_trust":previous.map(trust_review),
            "effective_source":{"before":effective_source(&before_effective),"after":effective_source(&after_effective)},
            "scope_effect":if metadata.is_some() {"Only this project's registry configuration changes."} else {"User-wide registry configuration changes; other projects using this user's sources are affected."},
            "default":{"before":before_effective.default_alias(),"after":after_effective.default_alias()},"installed_policy":"preserved"});
        Ok(Self {
            root,
            global,
            before,
            after,
            metadata,
            receipt,
            snapshot: None,
            reconciled: false,
            file: None,
        })
    }

    fn replayed(snapshot: Snapshot, publication: &Publication, file: GlobalFile) -> Self {
        let registrations = snapshot.registrations().clone();
        let mut receipt = publication_receipt(publication);
        receipt["registries"] = json!(registrations.sources);
        Self {
            root: None,
            global: registrations.clone(),
            before: registrations.clone(),
            after: registrations,
            metadata: None,
            receipt,
            snapshot: Some(snapshot),
            reconciled: true,
            file: Some(file),
        }
    }

    pub(in crate::review) fn committed_result(&self) -> Option<Value> {
        self.reconciled.then(|| self.receipt.clone())
    }

    pub(in crate::review) fn review(&self) -> Value {
        let mut value = self.receipt.clone();
        value["operation"] = json!("registry configuration");
        value
            .as_object_mut()
            .map(|fields| fields.remove("committed"));
        value
    }

    pub(in crate::review) fn commit(
        self,
        context: Option<&Context>,
        checkpoint: Checkpoint<'_>,
    ) -> Result<Value, Error> {
        checkpoint()?;
        if context.map(|c| &c.project_root) != self.root.as_ref() {
            return Err(Error::StaleSnapshot);
        }
        if self.reconciled {
            return Ok(self.receipt);
        }
        let file = self.file.clone().map_or_else(global_file, Ok)?;
        let Some((context, before_meta)) = context.zip(self.metadata) else {
            let receipt = self.receipt;
            let snapshot = self.snapshot.as_ref().ok_or(Error::StaleSnapshot)?;
            if snapshot.registrations() != &self.before {
                return Err(Error::StaleSnapshot);
            }
            let publication = file
                .publish(snapshot, &self.after, &operation(&receipt), &receipt)
                .map_err(registration_error)?;
            let mut result = publication_receipt(&publication);
            result["registries"] = json!(self.after.sources);
            return Ok(result);
        };
        let current_global = file.snapshot().map_err(registration_error)?;
        if current_global.registrations() != &self.global
            || self
                .snapshot
                .as_ref()
                .is_some_and(|s| s.generation() != current_global.generation())
        {
            return Err(Error::StaleSnapshot);
        }
        Store::open(Path::new(&context.project_root))?.update_checkpoint(
            context,
            checkpoint,
            |current| {
                let current_global = file.snapshot().map_err(registration_error)?;
                if current_global.registrations() != &self.global
                    || self
                        .snapshot
                        .as_ref()
                        .is_some_and(|s| s.generation() != current_global.generation())
                {
                    return Err(Error::StaleSnapshot);
                }
                if json!(current.settings.registries) != before_meta {
                    return Err(Error::StaleSnapshot);
                }
                crate::review::settings::write(
                    context,
                    &crate::review::settings::registries(self.after.clone()),
                )?;
                current.settings.registries = self.after.clone();
                let mut receipt = self.receipt;
                receipt["registries"] = json!(self.after.sources);
                Ok(receipt)
            },
        )
    }
}

fn intent(change: Change<'_>) -> Result<Value, Error> {
    Ok(match change {
        Change::Add { name, location } => {
            let absolute;
            let location =
                if !location.starts_with("https://") && !Path::new(location).is_absolute() {
                    absolute = std::env::current_dir()?.join(location);
                    absolute
                        .to_str()
                        .ok_or(Error::Invalid("registry path must be UTF-8"))?
                } else {
                    location
                };
            json!({"change":"add","alias":name,"location":location})
        }
        Change::Remove { name } => json!({"change":"remove","alias":name}),
        Change::Default { name } => json!({"change":"default","alias":name}),
    })
}

fn operation(receipt: &Value) -> String {
    crate::policy::fingerprint(
        json!([receipt["intent"], receipt["source"]])
            .to_string()
            .as_bytes(),
    )
}

fn publication_receipt(publication: &Publication) -> Value {
    let mut receipt = publication.receipt.clone();
    receipt["committed"] = json!(true);
    receipt["global_generation"] = json!(publication.generation);
    receipt["replayed"] = json!(publication.replayed);
    receipt["repair_needed"] = json!(publication.repair_needed);
    receipt["durability"] = json!(if publication.durable {
        "confirmed"
    } else {
        "unconfirmed"
    });
    receipt
}

/// Discovery proves catalog integrity under the presented key, not publisher
/// identity. Only the person's confirmation enrolls the retained origin/key.
fn trust_review(source: &canonic_registry_kit::source::Source) -> Value {
    let fingerprint = source
        .key()
        .and_then(|key| canonic_registry_kit::digest::decode(key, 32))
        .map(|key| canonic_registry_kit::digest::sha256_hex(&key));
    json!({"canonical_location":source.location(),"source_identity":source.identity(),"registry_identity":source.registry_id(),"signing_key_fingerprint_sha256":fingerprint,
        "basis":if source.is_signed(){"Trust on first use: trust this HTTPS origin, registry identity and signing key. The presented key verifies the catalog; this is not independent proof of publisher identity."}else{"Local operator-enrolled directory; unsigned catalog. Trust applies to this canonical directory."},
        "policy_effect":"Enrollment grants no pack permissions. Installing policy requires a separate review."})
}

#[cfg(test)]
mod tests {
    use super::*;
    use canonic_registry_kit::source::Source;

    #[test]
    fn global_generation_receipts_reconcile_exact_add_and_remove_without_reapproval() {
        let home = assert_fs::TempDir::new().unwrap();
        let first = assert_fs::TempDir::new().unwrap();
        let second = assert_fs::TempDir::new().unwrap();
        for source in [&first, &second] {
            std::fs::write(
                source.path().join("catalog.json"),
                r#"{"schema_version":1,"packs":[]}"#,
            )
            .unwrap();
        }
        let file = GlobalFile::new(home.path().join("registries.json"), BUILTIN);
        let fetcher = crate::review::registries::fetcher();
        let add = Change::Add {
            name: "acme",
            location: first.path().to_str().unwrap(),
        };
        let plan = Prepared::at(None, &fetcher, add, file.clone()).unwrap();
        assert!(plan.committed_result().is_none());
        let receipt = plan.commit(None, &|| Ok(())).unwrap();
        assert_eq!(receipt["committed"], true);
        assert_eq!(receipt["durability"], "confirmed");
        let replay = Prepared::at(None, &fetcher, add, file.clone())
            .unwrap()
            .committed_result()
            .unwrap();
        assert_eq!(replay["global_generation"], receipt["global_generation"]);
        assert_eq!(replay["replayed"], true);
        let replacement = Prepared::at(
            None,
            &fetcher,
            Change::Add {
                name: "acme",
                location: second.path().to_str().unwrap(),
            },
            file.clone(),
        )
        .unwrap();
        assert!(replacement.committed_result().is_none());
        assert_eq!(replacement.review()["change"], "replace");
        let remove = Change::Remove { name: "acme" };
        let removed = Prepared::at(None, &fetcher, remove, file.clone())
            .unwrap()
            .commit(None, &|| Ok(()))
            .unwrap();
        let replay = Prepared::at(None, &fetcher, remove, file)
            .unwrap()
            .committed_result()
            .unwrap();
        assert_eq!(replay["global_generation"], removed["global_generation"]);
        assert!(replay["registries"].as_object().unwrap().is_empty());
    }

    #[test]
    fn global_review_rejects_generation_aba_and_new_keys_have_distinct_operation_identity() {
        let home = assert_fs::TempDir::new().unwrap();
        let source = assert_fs::TempDir::new().unwrap();
        std::fs::write(
            source.path().join("catalog.json"),
            r#"{"schema_version":1,"packs":[]}"#,
        )
        .unwrap();
        let file = GlobalFile::new(home.path().join("registries.json"), BUILTIN);
        let fetcher = crate::review::registries::fetcher();
        Prepared::at(
            None,
            &fetcher,
            Change::Add {
                name: "acme",
                location: source.path().to_str().unwrap(),
            },
            file.clone(),
        )
        .unwrap()
        .commit(None, &|| Ok(()))
        .unwrap();
        let before = file.read().unwrap();
        let plan = Prepared::at(
            None,
            &fetcher,
            Change::Default { name: "acme" },
            file.clone(),
        )
        .unwrap();
        file.replace(&before, &Registrations::default()).unwrap();
        file.replace(&Registrations::default(), &before).unwrap();
        assert!(plan.commit(None, &|| Ok(())).is_err());
        let intent =
            json!({"change":"add","alias":"acme","location":"https://registry.example.com/"});
        let first =
            Source::signed("https://registry.example.com/", &"01".repeat(32), "acme").unwrap();
        let second =
            Source::signed("https://registry.example.com/", &"02".repeat(32), "acme").unwrap();
        assert_ne!(
            operation(&json!({"intent":intent,"source":first})),
            operation(&json!({"intent":intent,"source":second}))
        );
    }

    #[test]
    fn replacing_a_full_user_scope_preserves_its_default_and_reviews_both_sources() {
        let old = assert_fs::TempDir::new().unwrap();
        let new = assert_fs::TempDir::new().unwrap();
        std::fs::write(
            new.path().join("catalog.json"),
            r#"{"schema_version":1,"packs":[]}"#,
        )
        .unwrap();
        let source = Source::local(old.path()).unwrap();
        let mut global = Registrations::default();
        global.add("acme", source.clone(), BUILTIN).unwrap();
        for index in 1..canonic_registry_kit::registrations::MAX_REGISTRATIONS {
            global
                .add(&format!("acme-{index}"), source.clone(), BUILTIN)
                .unwrap();
        }
        global.default = Some("acme".into());
        let plan = Prepared::captured(
            None,
            global,
            None,
            &crate::review::registries::fetcher(),
            Change::Add {
                name: "acme",
                location: new.path().to_str().unwrap(),
            },
        )
        .unwrap();
        assert_eq!(plan.before.sources.len(), plan.after.sources.len());
        assert_eq!(plan.after.default.as_deref(), Some("acme"));
        let review = plan.review();
        assert_eq!(review["change"], "replace");
        assert_eq!(review["scope"], "user");
        assert_eq!(review["previous_source"], json!(source));
        assert_eq!(review["effective_source"]["after"]["scope"], "global");
        assert_ne!(
            review["source"]["identity"],
            review["previous_source"]["identity"]
        );
    }

    #[test]
    fn project_removal_names_the_revealed_global_source_and_its_trust() {
        let project = assert_fs::TempDir::new().unwrap();
        let user = assert_fs::TempDir::new().unwrap();
        let project_source = Source::local(project.path()).unwrap();
        let user_source = Source::local(user.path()).unwrap();
        let mut global = Registrations::default();
        global.add("acme", user_source.clone(), BUILTIN).unwrap();
        global.default = Some("acme".into());
        let plan = Prepared::captured(
            None,
            global,
            Some(json!({
                "sources":{"acme":project_source},"default":"acme"
            })),
            &crate::review::registries::fetcher(),
            Change::Remove { name: "acme" },
        )
        .unwrap();
        let review = plan.review();
        assert_eq!(review["effective_source"]["before"]["scope"], "project");
        assert_eq!(review["effective_source"]["after"]["scope"], "global");
        assert_eq!(
            review["effective_source"]["after"]["source"],
            json!(user_source)
        );
        assert_eq!(
            review["effective_source"]["after"]["trust"]["source_identity"],
            user_source.identity()
        );
        assert_eq!(review["default"], json!({"before":"acme","after":"acme"}));
        assert_eq!(review["installed_policy"], "preserved");
    }
}

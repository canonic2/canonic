//! First project publication, shared by terminal and MCP onboarding.
use super::{Context, Error, publication};
use crate::configuration;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(crate) struct Prepared {
    root: PathBuf,
    candidate: Value,
    metadata: Value,
    checkpoints: BTreeMap<String, Value>,
    files: BTreeMap<String, String>,
    evaluator: configuration::Evaluator,
    initially_absent: bool,
}

impl Prepared {
    pub(in crate::review) fn new(
        root: &Path,
        candidate: Value,
        mut metadata: Value,
        checkpoints: BTreeMap<String, Value>,
    ) -> Result<Self, Error> {
        super::packs::refresh_shared_metadata(&candidate, &mut metadata)?;
        let files = publication::projections::projections(&candidate, &metadata)?;
        metadata["projection"] = json!(publication::projections::digest(&files));
        metadata["projection_file_hashes"] = json!(publication::projections::file_hashes(&files));
        let evaluator = configuration::Evaluator::new(super::snapshots(&BTreeMap::from([(
            "project".into(),
            candidate.clone(),
        )]))?)?;
        let initially_absent = !crate::layout::init_destination(root).exists();
        let prepared = Self {
            root: root.into(),
            candidate,
            metadata,
            checkpoints,
            files,
            evaluator,
            initially_absent,
        };
        if prepared.initially_absent && prepared.has_authoring_files() {
            return Err(Error::Invalid(
                "existing Shield authoring files need inspection; initialization will not overwrite them",
            ));
        }
        Ok(prepared)
    }

    pub(in crate::review) fn preview_prefixes(
        &self,
        setup: &mut Value,
        desired: Option<&super::integrations::Registration>,
    ) {
        super::integrations::preview_prefix_rules(
            &self.root,
            setup,
            desired,
            self.evaluator.codex_prefixes(),
            true,
        );
    }

    fn has_authoring_files(&self) -> bool {
        self.files
            .keys()
            .map(String::as_str)
            .chain(crate::layout::LOCAL_FILES)
            .any(|name| {
                crate::layout::bundle_file(&self.root, name)
                    .symlink_metadata()
                    .is_ok()
            })
    }

    /// The caller has obtained human confirmation for this exact candidate.
    pub(in crate::review) fn commit(
        self,
        checkpoint: super::Checkpoint<'_>,
    ) -> Result<(Context, Value), Error> {
        checkpoint()?;
        match std::fs::create_dir(self.root.join(".shield")) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        let publication = publication::ProjectionLease::acquire(&self.root)?;
        checkpoint()?;
        if self.initially_absent
            && (self.has_authoring_files()
                || crate::layout::state(&self.root)
                    .join("state.json")
                    .symlink_metadata()
                    .is_ok())
        {
            return Err(Error::StaleSnapshot);
        }
        publication::initialize(
            &self.root,
            &BTreeMap::from([("project".into(), self.candidate)]),
            self.evaluator.revision(),
            &self.metadata,
            self.checkpoints,
        )?;
        let context = super::open(&self.root)?;
        publication.check(&context)?;
        let repair = publication::projections::materialize(&context, &self.files).is_err()
            || super::storage::Store::open(&self.root)
                .and_then(|store| {
                    store.update_checkpoint(&context, &|| Ok(()), |_| Ok(Value::Null))
                })
                .is_err();
        let mut receipt = super::project::status(&context)?;
        receipt["committed"] = json!(true);
        receipt["repair_needed"] = json!(repair || receipt["repair_needed"] == true);
        Ok((context, receipt))
    }
}

/// A first-run review holds the captured signed baseline and integration plan,
/// never a terminal or a tool-supplied approval flag.
pub(crate) struct Onboarding {
    policy: Prepared,
    plan: super::integrations::Plan,
    review: Value,
}

impl Onboarding {
    #[cfg(all(test, unix))]
    pub(crate) fn fixture(
        root: &Path,
        _: crate::policy::Profile,
        agents: &[String],
    ) -> Result<Self, Error> {
        Self::with_plan(
            root,
            Self::integration(root, agents)?,
            super::packs::bootstrap::Initial {
                rows: Vec::new(),
                bytes: Vec::new(),
                checkpoints: BTreeMap::new(),
            },
        )
    }

    pub(crate) fn prepare(
        root: &Path,
        profile: crate::policy::Profile,
        agents: &[String],
    ) -> Result<Self, Error> {
        if crate::layout::discovery_root(root) {
            return Err(Error::Invalid(
                "project already initialized; use setup to inspect or repair the registration",
            ));
        }
        let plan = Self::integration(root, agents)?;
        let initial =
            super::packs::bootstrap::acquire(root, profile, &super::registries::fetcher())?;
        Self::with_plan(root, plan, initial)
    }

    fn integration(root: &Path, agents: &[String]) -> Result<super::integrations::Plan, Error> {
        if agents.is_empty() {
            return Err(Error::Invalid(
                "select at least one integration; use hub for Hub publication or name native agents",
            ));
        }
        let choice = super::integrations::IntegrationChoice::Agents(
            super::integrations::IntegrationChoice::bits(agents)?,
        );
        choice.check_routes()?;
        let mut plan = super::integrations::setup::prepare_approving(
            root,
            &json!({}),
            choice,
            true,
            None,
            true,
        )?
        .ok_or(Error::Invalid("initial integration plan missing"))?;
        super::integrations::setup::with_fallbacks(
            &configuration::settings::Values::default(),
            choice,
            Some(&mut plan),
        );
        Ok(plan)
    }

    fn with_plan(
        root: &Path,
        plan: super::integrations::Plan,
        initial: super::packs::bootstrap::Initial,
    ) -> Result<Self, Error> {
        let mut metadata =
            json!({"sources":{},"default":null,"installed":initial.rows,"projection":null});
        let base = json!(configuration::Bundle::capture(
            "schema_version: 1\npacks: []\n",
            &[],
            None
        )?);
        let candidate = super::packs::rebuild(
            &base,
            metadata["installed"]
                .as_array_mut()
                .ok_or(Error::Invalid("installed packs missing"))?,
            &initial.bytes,
            None,
        )?;
        let policy = Prepared::new(root, candidate, metadata.clone(), initial.checkpoints)?;
        if !policy.initially_absent {
            return Err(Error::Invalid(
                "existing Shield authoring files need inspection; initialization will not overwrite them",
            ));
        }
        let mut integration = super::integrations::setup::preview(root, Some(&plan));
        policy.preview_prefixes(&mut integration, plan.desired.as_ref());
        let review = json!({"project":root,"operation":"initialize project", "packs":metadata["installed"], "integration_setup":integration});
        Ok(Self {
            policy,
            plan,
            review,
        })
    }

    pub(crate) fn review(&self) -> &Value {
        &self.review
    }

    pub(crate) fn message(&self) -> Result<String, Error> {
        super::operator::administrative_review(&self.review, &self.policy.root.to_string_lossy())
            .form()
    }

    /// Only a validated human confirmation may reach this method.
    pub(crate) fn commit(
        self,
        checkpoint: super::Checkpoint<'_>,
        surface: &str,
        cache_dir: Option<&Path>,
    ) -> Result<Value, Error> {
        let (mut context, receipt) = self.policy.commit(checkpoint)?;
        context.surface = surface.into();
        let mut receipt = super::integrations::setup::finish_checked(
            &context,
            &json!({}),
            Some(&self.plan),
            Some(receipt),
            checkpoint,
        )?;
        if receipt.get("setup_error").is_none()
            && let Some(directory) = cache_dir
        {
            receipt["cache_location"] = super::files::relocate_cache(&context, directory)?;
        }
        super::storage::Store::open(Path::new(&context.project_root))?.update_checkpoint(
            &context,
            &|| Ok(()),
            |state| {
                super::integrations::recovery::record_review(state, &context);
                Ok(Value::Null)
            },
        )?;
        Ok(receipt)
    }
}

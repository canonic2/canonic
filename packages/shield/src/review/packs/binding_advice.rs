//! Check an installed pack binding for a recorded Ask without changing policy.
use crate::{
    configuration::{Bundle, Evaluator},
    policy::{self, Decision, Request, Scope, model::variables},
    review::{Context, inspection, model::State, project},
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::BTreeMap,
    path::Path,
};

const MAX_PROBES: usize = 4;

/// Source capture and a small probe budget shared by one suggestions page.
#[derive(Default)]
pub(in crate::review) struct Lookup {
    capture: OnceCell<Option<(State, Evaluator)>>,
    probes: Cell<usize>,
    previews: RefCell<BTreeMap<String, Option<Vec<Preview>>>>,
}

struct Preview {
    evaluator: Evaluator,
    id: String,
    selector: String,
    value: Value,
}

impl Lookup {
    pub(in crate::review) fn action(
        &self,
        context: &Context,
        request: &Value,
        cwd: &str,
    ) -> Option<Value> {
        let executable = request["executable"].as_str()?;
        let path = Path::new(executable);
        if !path.is_absolute() {
            return None;
        }
        let tool = path
            .file_name()?
            .to_str()?
            .trim_end_matches(std::env::consts::EXE_SUFFIX);
        let name = format!("{}_executable", tool.replace('-', "_"));
        if !variables::valid_name(&name) {
            return None;
        }
        let request = Request {
            executable: executable.into(),
            arguments: serde_json::from_value(request["arguments"].clone()).ok()?,
            cwd: cwd.into(),
            project: context.project_root.clone(),
        };
        let (state, evaluator) = self
            .capture
            .get_or_init(|| project::load_prepared(context).ok())
            .as_ref()?;
        let scope = super::discovery::Scope::for_candidate(
            Path::new(&context.project_root),
            &state.candidates["project"],
        )
        .ok()?;
        if !scope.permits(path) || !super::discovery::executable(path) {
            return None;
        }
        if !evaluator.evaluate(&request).ok()?.unmatched_ask() {
            return None;
        }
        let mut previews = self.previews.borrow_mut();
        let previews = previews
            .entry(executable.to_owned())
            .or_insert_with(|| self.prepare(state, executable, &name))
            .as_ref()?;
        let mut matching = previews.iter().filter(|preview| {
            preview
                .evaluator
                .evaluate(&request)
                .is_ok_and(|result| result.decision == Decision::Allow)
        });
        let preview = matching.next()?;
        if matching.next().is_some() {
            return None;
        }
        Some(json!({"tool":"pack_configure",
            "arguments":{"reference":preview.selector,"set":{(name.clone()):preview.value},"layer":"local"},
            "instruction":format!("The installed {} pack would allow this command with the exact executable path added to its {name} setting. If this is the intended tool, configure the path locally and submit the returned apply_arguments unchanged to policy_apply for review.", preview.id)}))
    }

    fn prepare(&self, state: &State, executable: &str, name: &str) -> Option<Vec<Preview>> {
        let candidate = &state.candidates["project"];
        let mut previews = Vec::new();
        for (index, row) in state.operator["installed"].as_array()?.iter().enumerate() {
            let Some(source) = candidate["packs"][index]
                .as_str()
                .filter(|source| source.contains(name))
            else {
                continue;
            };
            let Ok(declarations) = policy::declarations(source) else {
                continue;
            };
            let Some(declaration) = declarations.get(name) else {
                continue;
            };
            if !(declaration.kind == variables::Kind::String || declaration.executable_paths())
                || declaration.layer() != variables::Layer::Local
                || declaration.default.is_some()
                || !super::discovery::declares_path(declaration, Path::new(executable))
            {
                continue;
            }
            let settings = inspection::selection_settings(candidate, index).ok()?;
            if settings
                .local
                .as_ref()
                .is_some_and(|local| local.unset.iter().any(|unset| unset == name))
            {
                continue;
            }
            let mut value = json!(executable);
            if let Some(existing) = settings.effective().variables.get(name) {
                if !declaration.executable_paths() {
                    continue;
                }
                let Value::Array(mut paths) = declaration.normalize(existing) else {
                    continue;
                };
                paths.push(value);
                value = Value::Array(paths);
            }
            let value = declaration.normalize(&value);
            if declaration.check(&value).is_err() {
                continue;
            }
            let probes = self.probes.get();
            if probes >= MAX_PROBES {
                return None;
            }
            self.probes.set(probes + 1);
            let selector = row["selector"].as_str()?;
            let set = BTreeMap::from([(name.to_owned(), value.clone())]);
            let (configured, _) = super::prepare_configure(state, selector, |source, settings| {
                settings.edit(true, |binding| {
                    super::settings::assign(source, binding, &set, &[])
                })
            })
            .ok()?;
            let Ok(bundle) = serde_json::from_value::<Bundle>(configured) else {
                continue;
            };
            let Ok(snapshot) = bundle.snapshot() else {
                continue;
            };
            previews.push(Preview {
                evaluator: Evaluator::new(vec![(Scope::Project, snapshot)]).ok()?,
                id: row["id"].as_str()?.to_owned(),
                selector: selector.to_owned(),
                value,
            });
        }
        Some(previews)
    }
}

//! Bounded project document reads and declarative registry validation.

use crate::{
    cli::Selection,
    policy::{self, CustomRules, Pack, Request},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{fs::File, io::Read, path::Path};

#[cfg(test)]
mod tests;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("filesystem access failed: {0}")]
    Read(#[from] std::io::Error),
    #[error(transparent)]
    Policy(#[from] policy::Error),
    #[error("candidate policy must be UTF-8")]
    Encoding,
    #[error("context must resolve to a directory with a UTF-8 path")]
    Context,
}

impl Error {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::Policy(policy::Error::Interrupted) => "interrupted",
            Self::Context => "context",
            Self::Policy(policy::Error::Limit) => "resource-limit",
            Self::Read(_) => "io",
            Self::Encoding => "encoding",
            Self::Policy(policy::Error::Syntax) => "invalid-document",
            Self::Policy(
                policy::Error::Contract(_)
                | policy::Error::DirectoryOptionConflict(_)
                | policy::Error::McpDefaults { .. }
                | policy::Error::Binding { .. },
            ) => "invalid-contract",
        }
    }
}

pub(crate) fn read(path: &Path) -> Result<String, Error> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take((policy::MAX_DOCUMENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > policy::MAX_DOCUMENT_BYTES {
        return Err(policy::Error::Limit.into());
    }
    String::from_utf8(bytes).map_err(|_| Error::Encoding)
}

fn load(selection: &Selection) -> Result<(Vec<Pack>, Option<CustomRules>), Error> {
    if selection.pack.len() > 64 {
        return Err(policy::Error::Limit.into());
    }
    let packs = selection
        .pack
        .iter()
        .map(|path| Ok(Pack::from_yaml(&read(path)?)?))
        .collect::<Result<Vec<_>, Error>>()?;
    let custom = selection
        .rules
        .as_ref()
        .map(|path| -> Result<_, Error> { Ok(CustomRules::from_yaml(&read(path)?)?) })
        .transpose()?;
    let mut ids = std::collections::BTreeSet::new();
    if packs.iter().any(|pack| !ids.insert(pack.id())) {
        return Err(policy::Error::Contract("duplicate pack identity").into());
    }
    Ok((packs, custom))
}

pub(crate) fn validate(selection: &Selection) -> Result<Value, Error> {
    let (packs, custom) = load(selection)?;
    Ok(
        json!({"schema_version":1,"mode":"preview","valid":true,"packs":packs.iter().map(Pack::id).collect::<Vec<_>>(),"custom_rules":custom.is_some()}),
    )
}

fn context(path: &Path) -> Result<String, Error> {
    let canonical = path.canonicalize()?;
    if !canonical.is_dir() {
        return Err(Error::Context);
    }
    canonical
        .into_os_string()
        .into_string()
        .map_err(|_| Error::Context)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cases {
    schema_version: u8,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    profile: policy::Profile,
    argv: Option<Vec<String>>,
    input: Option<policy::normalized::Input>,
    expected: policy::Decision,
    #[serde(default)]
    variables: std::collections::BTreeMap<String, Value>,
    #[serde(default)]
    mcp: crate::mcp_connections::Connections,
}

pub(crate) fn test_pack(input: &crate::cli::PackTest) -> Result<(Value, u8), Error> {
    let packs = fixture_packs(input)?;
    let pack = &packs[0].1;
    // One prepared evaluator per profile and set of case variable values.
    let mut engines = std::collections::BTreeMap::new();
    let engine = |profile: policy::Profile,
                  variables: &std::collections::BTreeMap<String, Value>,
                  mcp: &crate::mcp_connections::Connections|
     -> Result<crate::configuration::Evaluator, policy::Error> {
        let selections = packs.iter().enumerate().map(|(index, (_, pack))| {
            let mut selection = json!({"id":pack.id(),"version":pack.version(),"profile":profile,"path":format!("pack{index}.yml")});
            if index == 0 && !variables.is_empty() {
                selection["variables"] = json!(variables);
            }
            selection
        }).collect::<Vec<_>>();
        let config = json!({"schema_version":1,"packs":selections,"mcp":mcp}).to_string();
        let snapshot = crate::configuration::Candidate::from_yaml(&config)?.capture(
            &packs
                .iter()
                .map(|(source, _)| source.clone())
                .collect::<Vec<_>>(),
            None,
        )?;
        crate::configuration::Evaluator::new(vec![(policy::Scope::Project, snapshot)])
    };
    let cases: Cases = policy::parse(&read(&input.cases)?)?;
    if cases.schema_version != 1 || cases.cases.is_empty() || cases.cases.len() > 1024 {
        return Err(policy::Error::Contract("unsupported fixture version or case count").into());
    }
    let cwd = context(Path::new("."))?;
    let mut names = std::collections::BTreeSet::new();
    let mut results = Vec::new();
    let mut passed = true;
    for case in cases.cases {
        if case.name.trim().is_empty() || case.name.len() > 256 || !names.insert(case.name.clone())
        {
            return Err(
                policy::Error::Contract("fixture names must be nonempty and unique").into(),
            );
        }
        let key = (case.profile, json!([case.variables, case.mcp]).to_string());
        if !engines.contains_key(&key) {
            let prepared = engine(case.profile, &case.variables, &case.mcp)?;
            engines.insert(key.clone(), prepared);
        }
        let (actual, evaluation) = fixture_evaluation(&case, &engines[&key], &cwd, input.details)?;
        let matches = actual == case.expected;
        passed &= matches;
        let mut result = json!({"name":case.name,"profile":case.profile,"expected":case.expected,"actual":actual,"passed":matches});
        if !case.variables.is_empty() {
            result["variables"] = json!(case.variables);
        }
        if let Some(evaluation) = evaluation {
            result["evaluation"] = evaluation;
        }
        results.push(result);
    }
    Ok((
        json!({"schema_version":1,"mode":"preview","pack":pack.id(),"passed":passed,"cases":results}),
        u8::from(!passed),
    ))
}

fn fixture_packs(input: &crate::cli::PackTest) -> Result<Vec<(String, Pack)>, Error> {
    if input.with_pack.len() >= 64 {
        return Err(policy::Error::Limit.into());
    }
    let mut ids = std::collections::BTreeSet::new();
    std::iter::once(&input.pack)
        .chain(&input.with_pack)
        .map(|path| {
            let source = read(path)?;
            let pack = Pack::from_yaml(&source)?;
            if !ids.insert(pack.id().to_owned()) {
                return Err(policy::Error::Contract("duplicate pack identity").into());
            }
            Ok((source, pack))
        })
        .collect()
}

fn fixture_evaluation(
    case: &Case,
    engine: &crate::configuration::Evaluator,
    cwd: &str,
    details: bool,
) -> Result<(policy::Decision, Option<Value>), Error> {
    match (&case.argv, &case.input) {
        (Some(argv), None) => {
            let request = Request {
                executable: argv
                    .first()
                    .ok_or(policy::Error::Contract("fixture executable required"))?
                    .clone(),
                arguments: argv[1..].to_vec(),
                cwd: cwd.into(),
                project: cwd.into(),
            };
            let evaluation = engine.evaluate(&request)?;
            let decision = evaluation.decision;
            let value = if details {
                Some(serde_json::to_value(evaluation)
                    .map_err(|_| policy::Error::Contract("fixture serialization failed"))?["evaluation"].take())
            } else {
                None
            };
            Ok((decision, value))
        }
        (None, Some(input)) => {
            let context = policy::normalized::Context {
                working_files: None,
                cwd: cwd.into(),
                environment: Some(std::collections::BTreeMap::new()),
            };
            input.validate(&context, cwd)?;
            let value = engine.evaluate_normalized(
                input,
                &context,
                cwd,
                None,
                &crate::evidence::now(),
                &|| Ok(()),
            )?;
            let actual = serde_json::from_value(value["decision"].clone())
                .map_err(|_| policy::Error::Contract("fixture decision unavailable"))?;
            Ok((actual, details.then_some(value)))
        }
        _ => Err(policy::Error::Contract("fixture requires exactly one of argv or input").into()),
    }
}

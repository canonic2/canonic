//! Human project onboarding; structured/piped use is explicitly noninteractive.
use crate::{
    configuration::{onboarding as model, projects},
    error::{Error, Result},
};
use canonic_tui_kit::{picker::Choice, runtime::prompts};
use clap::Args;
use serde_json::json;
use std::{
    io::{self, IsTerminal, Write},
    path::PathBuf,
};

#[derive(Args)]
pub(super) struct Options {
    /// Configuration root to inspect; defaults to this directory.
    #[arg(long, default_value = ".")]
    root: PathBuf,
    /// Create this configuration file instead of testrun.yml in the root.
    #[arg(long)]
    config: Option<PathBuf>,
    /// Inspect these application directories only; repeat to select several.
    #[arg(long = "path")]
    paths: Vec<PathBuf>,
    /// Project kind for explicit paths; custom namespaced identifiers are accepted.
    #[arg(long, requires = "paths")]
    kind: Option<String>,
    /// Platforms for explicit paths; repeat for several. No platform limits apply.
    #[arg(long = "platform", requires = "paths")]
    platforms: Vec<String>,
    /// Return a discovery and configuration proposal without writing files.
    #[arg(long)]
    dry_run: bool,
    /// Create the reviewed/detected configuration without terminal questions.
    #[arg(long)]
    yes: bool,
}
pub(super) fn run(options: Options, machine: bool) -> Result<()> {
    let root = options
        .root
        .canonicalize()
        .map_err(|_| Error::new("CONFIG_PATH", "Configuration root is unavailable"))?;
    let config = super::config_path(options.config)?;
    let config = config.as_deref();
    if !options.dry_run {
        model::available(&root, config)?;
    }
    let mut discovery = model::discover(&root, &options.paths)?;
    for candidate in &mut discovery.candidates {
        if let Some(kind) = &options.kind {
            candidate.kind = kind.clone();
        }
        if !options.platforms.is_empty() {
            candidate.platforms = options.platforms.clone();
        }
    }
    if options.dry_run {
        return super::presentation::print_result(
            model::api::review(&discovery, &discovery.candidates, config)?,
            machine,
            "Testrun project discovery",
        );
    }
    let interactive = !machine && io::stdin().is_terminal() && io::stdout().is_terminal();
    let candidates = if options.yes {
        if discovery.truncated {
            return Err(Error::new(
                "DISCOVERY_LIMIT",
                "Discovery was incomplete; select explicit --path values or review interactively",
            ));
        }
        discovery.candidates.clone()
    } else {
        if !interactive {
            return Err(Error::new(
                "INIT_CONFIRMATION_REQUIRED",
                "Use testrun init --dry-run --json to inspect; create with --yes or run in a terminal",
            ));
        }
        select(&discovery)?
    };
    let configuration = model::configuration(&candidates)?;
    if !options.yes {
        let review = model::api::review(&discovery, &candidates, config)?;
        writeln!(
            io::stdout().lock(),
            "{}",
            serde_json::to_string_pretty(&review).unwrap()
        )
        .map_err(|_| Error::new("HOST_IO", "Cannot print project review"))?;
        let choice = prompt(prompts::choose(
            "Create Testrun project configuration?",
            vec!["Cancel".into(), "Create reviewed configuration".into()],
            Some(0),
        ))?;
        if choice != Some(1) {
            return Err(cancelled());
        }
    }
    let mut result = model::publish_at(&root, config, &configuration)?;
    result["warnings"] = json!(discovery.warnings);
    super::presentation::print_result(result, machine, "Testrun initialized")
}
fn prompt<T>(result: io::Result<T>) -> Result<T> {
    result.map_err(|error|Error::new("HOST_IO","Project wizard could not display its prompt").details(json!({"cause":error.to_string(),"nextAction":"Use --dry-run --json, then explicit --path/--kind/--platform and --yes"})))
}
fn cancelled() -> Error {
    Error::new(
        "INIT_CANCELLED",
        "Project initialization cancelled; configuration was not created",
    )
}
fn text(title: &str, default: &str) -> Result<String> {
    let mut value = default.to_owned();
    if prompt(prompts::read(
        title,
        &mut value,
        false,
        256,
        &mut |_| Ok(()),
    ))? != prompts::Answer::Submitted
    {
        return Err(cancelled());
    }
    Ok(value)
}
fn select(discovery: &model::Discovery) -> Result<Vec<model::Candidate>> {
    let mut choices: Vec<_> = discovery
        .candidates
        .iter()
        .map(|candidate| {
            let mut choice = Choice::new(&candidate.path, &candidate.path);
            choice.detail = format!("{} · {}", candidate.kind, candidate.evidence.join(", "));
            choice
        })
        .collect();
    choices.push(Choice::new("@manual", "Add a project manually"));
    let marked: Vec<_> = discovery
        .candidates
        .iter()
        .map(|c| c.path.as_str())
        .collect();
    let ids = prompt(prompts::choose_many(
        "Select application projects",
        "Space marks projects; Enter continues. Detected projects are suggestions.",
        choices,
        &marked,
        1,
        "continue",
    ))?
    .ok_or_else(cancelled)?;
    let mut selected = Vec::new();
    for id in ids {
        let mut candidate = if id == "@manual" {
            let path = text(
                "Application directory relative to the configuration root",
                ".",
            )?;
            model::discover(&discovery.root, &[PathBuf::from(path)])?
                .candidates
                .into_iter()
                .next()
                .ok_or_else(|| Error::invalid("Application could not be inspected"))?
        } else {
            discovery
                .candidates
                .iter()
                .find(|candidate| candidate.path == id)
                .cloned()
                .ok_or_else(|| Error::invalid("Unknown project selection"))?
        };
        let mut kinds: Vec<_> = projects::CORE_KINDS
            .iter()
            .map(|kind| Choice::new(*kind, *kind))
            .collect();
        if !projects::CORE_KINDS.contains(&candidate.kind.as_str()) {
            kinds.push(Choice::new(&candidate.kind, &candidate.kind));
        }
        kinds.push(Choice::new("@custom", "Another project kind"));
        let kind = prompt(prompts::choose_choices(
            &format!("Project kind: {}", candidate.path),
            "Choose the framework or workflow.",
            kinds,
            Some(&candidate.kind),
        ))?
        .ok_or_else(cancelled)?;
        candidate.kind = if kind == "@custom" {
            text("Project kind identifier", "custom")?
        } else {
            kind
        };
        let choices = ["web", "ios", "android", "macos", "windows", "linux"]
            .into_iter()
            .map(|p| Choice::new(p, p))
            .chain([Choice::new("@custom", "Another platform")])
            .collect();
        let marked: Vec<_> = candidate.platforms.iter().map(String::as_str).collect();
        let platforms=prompt(prompts::choose_many(&format!("Platforms: {}",candidate.path),"Create starter profiles, or leave empty to configure profiles later. Builds and drivers are configured separately.",choices,&marked,0,"skip profiles"))?.ok_or_else(cancelled)?;
        candidate.platforms = platforms
            .into_iter()
            .map(|platform| {
                if platform == "@custom" {
                    text("Platform identifier", "custom")
                } else {
                    Ok(platform)
                }
            })
            .collect::<Result<_>>()?;
        selected.push(candidate);
    }
    Ok(selected)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    #[test]
    fn onboarding_cli_supports_review_and_unattended_modes_without_creating_on_inspection() {
        for args in [
            vec!["testrun", "init"],
            vec!["testrun", "init", "--dry-run", "--json"],
            vec![
                "testrun",
                "init",
                "--path",
                "apps/web",
                "--kind",
                "vue",
                "--platform",
                "web",
                "--yes",
            ],
        ] {
            assert!(crate::cli::CommandLine::try_parse_from(args).is_ok());
        }
        assert!(
            crate::cli::CommandLine::try_parse_from(["testrun", "init", "--kind", "react"])
                .is_err()
        );
        let root = tempfile::tempdir().unwrap();
        let options = Options {
            root: root.path().into(),
            config: None,
            paths: vec![],
            kind: None,
            platforms: vec![],
            dry_run: false,
            yes: false,
        };
        assert_eq!(
            run(options, true).err().unwrap().code,
            "INIT_CONFIRMATION_REQUIRED"
        );
        assert!(!root.path().join("testrun.yml").exists());
    }
    #[test]
    fn configuration_consuming_commands_accept_a_config_file() {
        for args in [
            vec![
                "testrun",
                "init",
                "--config",
                ".config/testrun.yml",
                "--dry-run",
            ],
            vec!["testrun", "config", "show", "--config", "qa/acme.yml"],
            vec!["testrun", "config", "--config", "qa/acme.yml", "init"],
            vec![
                "testrun",
                "build",
                "start",
                "acme",
                "--config",
                "qa/acme.yml",
            ],
            vec!["testrun", "project", "list", "--config", "qa/acme.yml"],
            vec![
                "testrun",
                "project",
                "status",
                "dev",
                "--config",
                "qa/acme.yml",
            ],
        ] {
            assert!(
                crate::cli::CommandLine::try_parse_from(&args).is_ok(),
                "{args:?}"
            );
        }
        assert!(
            crate::cli::CommandLine::try_parse_from([
                "testrun", "build", "start", "--inline", "{}", "--config", "acme.yml"
            ])
            .is_err()
        );
        let resolved = crate::cli::config_path(Some("qa/acme.yml".into()))
            .unwrap()
            .unwrap();
        assert_eq!(
            resolved,
            std::env::current_dir().unwrap().join("qa/acme.yml")
        );
    }
    #[test]
    fn init_dry_run_reviews_the_selected_destination_without_writing() {
        let root = tempfile::tempdir().unwrap();
        let options = Options {
            root: root.path().into(),
            config: Some(root.path().join(".config/testrun.yml")),
            paths: vec![],
            kind: None,
            platforms: vec![],
            dry_run: true,
            yes: false,
        };
        run(options, true).unwrap();
        assert!(!root.path().join(".config").exists());
        assert!(!root.path().join(".gitignore").exists());
    }
}

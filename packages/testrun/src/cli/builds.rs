//! CLI test-build profile selection, client environment capture and retained polling.
use crate::{
    builds::Request,
    error::{Error, Result},
};
use clap::{Args, Subcommand};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Subcommand)]
pub(super) enum ConfigCommand {
    /// Create an empty testrun.yml, or the --config file, and ignore its local overlay, preserving existing files.
    Init,
    /// Validate project build profiles without running tools or contacting devices.
    Check,
    /// Show redacted effective build profiles and where each value comes from.
    Show,
    /// Print the configuration schema, or with --local the partial local overlay's.
    Schema {
        #[arg(long,default_value_t=2,value_parser=clap::value_parser!(u32).range(1..=2))]
        version: u32,
        #[arg(long)]
        local: bool,
    },
}
#[derive(Args)]
pub(super) struct Options {
    #[arg(required_unless_present = "inline", conflicts_with = "inline")]
    profile: Option<String>,
    /// Stable retry key; recover it with `build status --submission-id`.
    #[arg(long)]
    submission_id: Option<String>,
    /// Complete profile JSON instead of project YAML.
    #[arg(long)]
    inline: Option<String>,
    #[arg(long, alias = "root", default_value = ".")]
    project: PathBuf,
    /// Configuration file to use instead of testrun.yml or .config/testrun.yml.
    #[arg(long, conflicts_with = "inline")]
    config: Option<PathBuf>,
    /// Select an application project inside the configuration root.
    #[arg(long, conflicts_with = "inline")]
    app: Option<String>,
    #[arg(long)]
    install: bool,
    #[arg(long, requires = "install")]
    launch: bool,
    #[arg(long, requires = "install")]
    replace: bool,
    #[arg(long, requires = "install")]
    boot: bool,
    /// Override a build-process variable; repeat for several keys.
    #[arg(long = "set-env")]
    env: Vec<String>,
    /// Return operationId immediately; poll `build status` for the outcome.
    #[arg(long)]
    no_wait: bool,
    /// Run this profile's declared development preparation before building.
    #[arg(long)]
    prepare: bool,
    #[arg(long)]
    configuration: Option<String>,
    #[arg(long)]
    variant: Option<String>,
    #[arg(long)]
    flavor: Option<String>,
    #[arg(long)]
    entry_point: Option<String>,
    #[arg(long,value_parser=["debug","profile","release"])]
    mode: Option<String>,
    #[arg(long,value_parser=clap::value_parser!(u64).range(1000..=3600000))]
    timeout_ms: Option<u64>,
    #[arg(long = "launch-arg", allow_hyphen_values = true, requires = "launch")]
    launch_args: Vec<String>,
    #[arg(long = "launch-env", requires = "launch")]
    launch_env: Vec<String>,
}
pub(super) fn config(
    project: PathBuf,
    config: Option<PathBuf>,
    command: ConfigCommand,
    machine: bool,
) -> Result<()> {
    let root = project
        .canonicalize()
        .map_err(|_| Error::invalid("Project directory is unavailable"))?;
    let config = super::config_path(config)?;
    let (name, local, version) = match command {
        ConfigCommand::Init => ("config_init", false, 2),
        ConfigCommand::Check => ("config_check", false, 2),
        ConfigCommand::Show => ("config_show", false, 2),
        ConfigCommand::Schema { local, version } => ("config_schema", local, version),
    };
    super::presentation::print_result(
        crate::builds::config(
            name,
            &json!({"root":root,"config":config,"local":local,"version":version}),
        )?,
        machine,
        "Build configuration",
    )
}
fn environment(items: Vec<String>, option: &str) -> Result<BTreeMap<String, String>> {
    let mut env = BTreeMap::new();
    for item in items {
        let (key, value) = item
            .split_once('=')
            .ok_or_else(|| Error::invalid(format!("{option} requires NAME=VALUE")))?;
        if env.insert(key.into(), value.into()).is_some() {
            return Err(Error::invalid(format!("Duplicate {option} key")));
        }
    }
    Ok(env)
}

pub(super) async fn run(options: Options, target: Option<String>) -> Result<Value> {
    let env = environment(options.env, "--set-env")?;
    let launch_env = environment(options.launch_env, "--launch-env")?;
    let overrides = crate::builds::Overrides {
        configuration: options.configuration,
        variant: options.variant,
        flavor: options.flavor,
        entry_point: options.entry_point,
        mode: options.mode,
        timeout_ms: options.timeout_ms,
        launch_args: (!options.launch_args.is_empty()).then_some(options.launch_args),
        launch_env,
    };
    let mut request = Request {
        root: options
            .app
            .as_ref()
            .map(|_| {
                options
                    .project
                    .canonicalize()
                    .map_err(|_| Error::invalid("Configuration root is unavailable"))
            })
            .transpose()?,
        task: None,
        check: None,
        prepare: options.prepare,
        submission_id: options.submission_id,
        client_capture: true,
        project: if let Some(app) = options.app {
            PathBuf::from(app)
        } else {
            options
                .project
                .canonicalize()
                .map_err(|_| Error::invalid("Project directory is unavailable"))?
        },
        config: super::config_path(options.config)?,
        profile: options.profile,
        inline: options
            .inline
            .map(|v| {
                serde_json::from_str(&v)
                    .map_err(|_| Error::invalid("--inline must contain profile JSON"))
            })
            .transpose()?,
        env,
        bindings: BTreeMap::new(),
        baseline: BTreeMap::new(),
        overrides,
        install: options.install,
        launch: options.launch,
        replace: options.replace,
        boot: options.boot,
        target,
    };
    crate::builds::capture_cli(&mut request)?;
    let mut client = crate::managed::Client::new();
    let mut record = client
        .call("build_start", serde_json::to_value(request).unwrap())
        .await?;
    let id = record["operationId"]
        .as_str()
        .ok_or_else(|| Error::new("BUILD_IO", "Build submission returned no operationId"))?
        .to_owned();
    if options.no_wait {
        return Ok(record);
    }
    loop {
        if record["settled"] == true {
            if let Some(error) = record.get("error") {
                return Err(Error::new(
                    error["code"].as_str().unwrap_or("BUILD_FAILED"),
                    error["message"].as_str().unwrap_or("Build failed"),
                )
                .details(json!({"operation":record})));
            }
            return Ok(record);
        }
        tokio::select! {
            _=tokio::signal::ctrl_c()=>{
                client.call("build_cancel",json!({"operationId":id})).await?;
            },
            _=tokio::time::sleep(std::time::Duration::from_millis(250))=>{},
        }
        record = client
            .call("build_status", json!({"operationId":id}))
            .await?;
    }
}

//! Project commands compose the existing retained build execution surface.
use crate::{
    builds::Request,
    error::{Error, Result},
};
use clap::Subcommand;
use serde_json::json;
use std::path::PathBuf;

#[derive(Subcommand)]
pub(super) enum ProjectCommand {
    /// Page detected projects without running project code or writing configuration.
    Discover {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long = "path")]
        paths: Vec<PathBuf>,
        #[command(flatten)]
        page: super::arguments::PageArgs,
    },
    /// List applications configured under a root.
    List {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[command(flatten)]
        page: super::arguments::PageArgs,
    },
    /// Show a named application's configuration.
    Show {
        project: String,
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Open the configured Expo URL on a connected simulator or Android target.
    Open {
        profile: String,
        #[arg(long = "profile", requires = "root")]
        app_profile: Option<String>,
        #[arg(long, requires = "app_profile", conflicts_with = "project")]
        root: Option<PathBuf>,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Inspect Expo and optional --target app inventory without starting a build.
    Status {
        profile: String,
        #[arg(long = "profile", requires = "root")]
        app_profile: Option<String>,
        #[arg(long, requires = "app_profile", conflicts_with = "project")]
        root: Option<PathBuf>,
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Start a declared task; inspect with build status/log and stop with build cancel.
    Start {
        profile: String,
        #[arg(long = "profile", requires = "root")]
        app_profile: Option<String>,
        #[arg(long, requires = "app_profile", conflicts_with = "project")]
        root: Option<PathBuf>,
        #[arg(long, default_value = ".")]
        project: PathBuf,
        #[arg(long,value_parser=["preflight","prepare","server","check","build"])]
        task: String,
        #[arg(long)]
        check: Option<String>,
        #[arg(long)]
        submission_id: Option<String>,
        #[arg(long)]
        prepare: bool,
        #[arg(long)]
        install: bool,
        #[arg(long, requires = "install")]
        launch: bool,
        #[arg(long, requires = "install")]
        replace: bool,
    },
}
pub(super) async fn run(
    command: ProjectCommand,
    config: Option<PathBuf>,
    target: Option<String>,
    machine: bool,
) -> Result<()> {
    let config = super::config_path(config)?;
    let (name, mut args) = match command {
        ProjectCommand::Discover { root, paths, page } => ("project_discover", {
            let mut args = json!({"root":canonical(root)?,"paths":paths});
            page.add_to(&mut args);
            args
        }),
        ProjectCommand::List { root, page } => ("project_list", {
            let mut args = json!({"root":canonical(root)?});
            page.add_to(&mut args);
            args
        }),
        ProjectCommand::Show { root, project } => (
            "project_show",
            json!({"root":canonical(root)?,"project":project}),
        ),
        ProjectCommand::Open {
            profile,
            project,
            app_profile,
            root,
        } => ("project_open", {
            let mut args = selection(profile, project, app_profile, root)?;
            args["targetId"] =
                json!(target.ok_or_else(|| Error::invalid("project open requires --target"))?);
            args
        }),
        ProjectCommand::Status {
            profile,
            project,
            app_profile,
            root,
        } => ("project_status", {
            let mut args = selection(profile, project, app_profile, root)?;
            args["targetId"] = json!(target);
            args
        }),
        ProjectCommand::Start {
            profile,
            project,
            app_profile,
            root,
            task,
            check,
            submission_id,
            prepare,
            install,
            launch,
            replace,
        } => {
            let mut value = selection(profile, project, app_profile, root)?;
            value.as_object_mut().unwrap().extend(json!({"config":config,"task":task,"check":check,"submissionId":submission_id,"prepare":prepare,"install":install,"launch":launch,"replace":replace,"targetId":target}).as_object().unwrap().clone());
            let mut request: Request = serde_json::from_value(value)
                .map_err(|_| Error::invalid("Invalid project task"))?;
            request.client_capture = true;
            crate::builds::capture_cli(&mut request)?;
            ("project_start", serde_json::to_value(request).unwrap())
        }
    };
    if let Some(map) = args.as_object_mut() {
        map.insert("config".into(), json!(config));
        map.retain(|_, v| !v.is_null());
    }
    let result = if matches!(name, "project_list" | "project_show" | "project_discover") {
        crate::builds::config(name, &args)?
    } else {
        crate::managed::Client::new().call(name, args).await?
    };
    super::presentation::print_result(result, machine, "Project")
}
fn selection(
    profile: String,
    project: PathBuf,
    app_profile: Option<String>,
    root: Option<PathBuf>,
) -> Result<serde_json::Value> {
    match (root, app_profile) {
        (Some(root), Some(app_profile)) => {
            Ok(json!({"root":canonical(root)?,"project":profile,"profile":app_profile}))
        }
        (None, None) => Ok(json!({"project":canonical(project)?,"profile":profile})),
        _ => Err(Error::invalid(
            "Named selection requires --root and --profile",
        )),
    }
}
fn canonical(path: PathBuf) -> Result<PathBuf> {
    path.canonicalize()
        .map_err(|_| Error::invalid("Project directory is unavailable"))
}

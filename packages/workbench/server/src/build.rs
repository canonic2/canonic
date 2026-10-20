//! The command-line builds, without a server: `init` writes a project's
//! authoring types and manifest, `list` prints what the page index finds,
//! `check` builds every preview and reports the ones that fail, and `build` writes the portable browser build to a
//! folder a static server can serve.
//!
//! Project code runs in the Node evaluator, on `--node` or the `node` on the
//! `PATH`.
use crate::catalog::{compiler_catalog, index_request, read_response};
use crate::compiler::{Previews, Settings};
use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Workbench's own folder: the one holding `server/bin/<target>/` this binary runs from.
fn product_of_binary() -> Result<PathBuf> {
    let binary = std::env::current_exe()?;
    binary
        .ancestors()
        .nth(4)
        .filter(|folder| folder.join("sdk/api.d.ts").is_file())
        .map(Path::to_path_buf)
        .context("couldn’t find Workbench’s folder beside this binary; pass --product <folder>")
}

/// `init <project>`: authoring types, and a manifest when there is none.
fn init(root: &Path, product: &Path) -> Result<()> {
    std::fs::create_dir_all(root)?;
    let declarations = std::fs::read_to_string(product.join("sdk/api.d.ts"))?;
    let types = root.join("workbench-env.d.ts");
    if !types.exists() {
        let indented: Vec<String> = declarations
            .split('\n')
            .map(|line| format!("  {line}"))
            .collect();
        std::fs::write(
            &types,
            format!(
                "// Workbench authoring types. No runtime package installation is required.\ndeclare module \"@canonic2/workbench\" {{\n{}\n}}\n",
                indented.join("\n")
            ),
        )?;
    }
    let manifest = root.join("workbench.yaml");
    if !manifest.exists() {
        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        std::fs::write(&manifest, format!("name: {}\n", json!(name)))?;
    }
    println!(
        "Workbench authoring types and manifest ready in {}",
        root.display()
    );
    Ok(())
}

/// What the page index finds in `root`, with the Node runtime for definitions that need it.
fn listed(
    root: &Path,
    product: &Path,
    node: &Path,
    config: &Value,
) -> Result<crate::catalog::IndexAnswer> {
    let options = if config["previews"].is_object() {
        config["previews"].clone()
    } else {
        json!({})
    };
    let request = index_request(root, product, &options, Some(node), "")?;
    read_response(&crate::index::answer(&request)?)
}

/// The compiler for `root`, configured with what the page index lists.
async fn compiler(
    root: &Path,
    product: &Path,
    node: &Path,
    config: &Value,
) -> Result<(Arc<Previews>, crate::catalog::IndexAnswer)> {
    let options = if config["previews"].is_object() {
        config["previews"].clone()
    } else {
        json!({})
    };
    let request = index_request(root, product, &options, Some(node), "")?;
    let response = tokio::task::spawn_blocking(move || crate::index::answer(&request)).await??;
    let index = read_response(&response)?;
    let previews = Previews::new(root, product)?;
    previews
        .configure(Some(Settings {
            options,
            node: Some(node.to_path_buf()),
            catalog: compiler_catalog(config, &index),
        }))
        .await;
    Ok((previews, index))
}

async fn check(root: &Path, product: &Path, node: &Path, config: &Value) -> Result<bool> {
    let (previews, index) = compiler(root, product, node, config).await?;
    let mut failures = index.errors.clone();
    for preview in &index.previews {
        let file = preview["file"].as_str().unwrap_or_default();
        match previews.portable_preview(file).await {
            Ok(_) => println!("Built {}", preview["id"].as_str().unwrap_or_default()),
            Err(error) => failures.push(format!("{file}: {error:#}")),
        }
    }
    previews.close().await;
    if !failures.is_empty() {
        eprintln!("{}", failures.join("\n"));
    }
    Ok(failures.is_empty())
}

async fn build(
    root: &Path,
    product: &Path,
    node: &Path,
    config: &Value,
    destination: &Path,
) -> Result<bool> {
    if destination.exists() && std::fs::read_dir(destination)?.next().is_some() {
        bail!("Output directory must be empty: {}", destination.display());
    }
    let (previews, index) = compiler(root, product, node, config).await?;
    let name = config["name"]
        .as_str()
        .filter(|name| !name.is_empty())
        .map_or_else(
            || {
                root.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            },
            str::to_owned,
        );
    let request = crate::portable::Request {
        root,
        product,
        index: &index,
        config: Ok(Some(config.clone())),
        name,
        selection: None,
    };
    let built = crate::portable::build(&previews, request).await;
    previews.close().await;
    let built = built?;
    for file in &built.files {
        use base64::Engine as _;
        let target = destination.join(file.path.strip_prefix("browser/").unwrap_or(&file.path));
        if let Some(folder) = target.parent() {
            std::fs::create_dir_all(folder)?;
        }
        std::fs::write(
            &target,
            base64::engine::general_purpose::STANDARD.decode(&file.data)?,
        )?;
    }
    println!(
        "Built {} previews in {}",
        built.previews.len(),
        destination.display()
    );
    if !built.warnings.is_empty() {
        eprintln!("{}", built.warnings.join("\n"));
    }
    Ok(built.warnings.is_empty())
}

/// Runs `init`, `check` or `build`; false when something failed to build.
///
/// # Errors
/// Fails on a bad command line or an unreadable project.
pub(crate) fn run(command: &str, mut args: impl Iterator<Item = String>) -> Result<bool> {
    let mut positional = Vec::new();
    let (mut product, mut node) = (None, None);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--product" => {
                product = Some(PathBuf::from(
                    args.next().context("--product needs a folder")?,
                ));
            }
            "--node" => {
                node = Some(PathBuf::from(
                    args.next().context("--node needs a program")?,
                ));
            }
            _ => positional.push(arg),
        }
    }
    let root = std::path::absolute(positional.first().map_or(".", String::as_str))?;
    let product = match product {
        Some(product) => std::path::absolute(product)?,
        None => product_of_binary()?,
    };
    if command == "init" {
        init(&root, &product)?;
        return Ok(true);
    }
    let config = crate::config::answer(&root, None)?;
    if config["previews"] == false {
        println!("Workbench previews are disabled");
        return Ok(true);
    }
    let node = node.unwrap_or_else(|| PathBuf::from("node"));
    if command == "list" {
        let index = listed(&root, &product, &node, &config)?;
        println!(
            "{}",
            json!({ "previews": index.previews, "docs": index.docs, "errors": index.errors })
        );
        return Ok(true);
    }
    let destination = std::path::absolute(
        positional
            .get(1)
            .map_or_else(|| root.join("workbench-static"), PathBuf::from),
    )?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let work = async {
        if command == "check" {
            check(&root, &product, &node, &config).await
        } else {
            build(&root, &product, &node, &config, &destination).await
        }
    };
    let result = runtime.block_on(rspack_tasks::within_compiler_context(
        Arc::new(rspack_tasks::CompilerContext::new()),
        work,
    ));
    runtime.shutdown_background();
    result
}

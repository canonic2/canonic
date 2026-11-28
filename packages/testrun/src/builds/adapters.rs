//! Native builder argv and output contracts; build commands never install or launch.
use super::*;

pub(super) struct Plan {
    pub commands: Vec<Vec<String>>,
    pub cwd: PathBuf,
    pub selector: crate::configuration::Artifact,
    pub archive: bool,
}
fn argv(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).into()).collect()
}
fn select(path: PathBuf) -> crate::configuration::Artifact {
    crate::configuration::Artifact {
        path: Some(path.to_string_lossy().into()),
        glob: None,
    }
}
fn glob(pattern: String) -> crate::configuration::Artifact {
    crate::configuration::Artifact {
        path: None,
        glob: Some(pattern),
    }
}

pub(super) fn plan(prepared: &Prepared, output: &Path) -> Result<Plan> {
    let p = &prepared.profile;
    let mut cwd = prepared.cwd.clone();
    let mut commands = Vec::new();
    let mut archive = false;
    let mut selector = p.artifact.clone().map(|mut artifact| {
        if let Some(path) = &mut artifact.path {
            *path = prepared.cwd.join(&*path).to_string_lossy().into_owned();
        }
        if let Some(glob) = &mut artifact.glob {
            *glob = prepared.cwd.join(&*glob).to_string_lossy().into_owned();
        }
        artifact
    });
    match p.builder.as_str() {
        "custom" => commands.push(p.custom.as_ref().unwrap().command.clone()),
        "xcode" => {
            let x = p.xcode.as_ref().unwrap();
            commands.push(xcode(x, p, output));
            selector.get_or_insert(glob(xcode_output(x, p, output)));
        }
        "gradle" => {
            commands.push(gradle(&cwd, p.gradle.as_ref().unwrap()));
            selector.get_or_insert(glob("**/build/outputs/apk/**/*.apk".into()));
        }
        "react-native" => {
            let r = p.react_native.as_ref().unwrap();
            if p.platform == "ios" {
                cwd = cwd.join("ios");
                let workspace = one_project(&cwd, "xcworkspace")?;
                let project = if workspace.is_none() {
                    one_project(&cwd, "xcodeproj")?
                } else {
                    None
                };
                if workspace.is_none() && project.is_none() {
                    return Err(Error::new(
                        "PREREQUISITE_MISSING",
                        "React Native ios needs one Xcode workspace or project",
                    ));
                }
                let x = crate::configuration::Xcode {
                    workspace,
                    project,
                    scheme: r.scheme.clone().unwrap(),
                    configuration: r.configuration.clone().unwrap(),
                    sdk: None,
                    build_settings: Env::new(),
                    development_team: None,
                };
                commands.push(xcode(&x, p, output));
                selector.get_or_insert(glob(xcode_output(&x, p, output)));
            } else {
                cwd = cwd.join("android");
                let variant = r.variant.as_deref().unwrap();
                let task = format!(
                    "assemble{}{}",
                    variant[..1].to_ascii_uppercase(),
                    &variant[1..]
                );
                commands.push(gradle(
                    &cwd,
                    &crate::configuration::Gradle {
                        tasks: vec![task],
                        properties: Env::new(),
                    },
                ));
                selector.get_or_insert(glob("app/build/outputs/apk/**/*.apk".into()));
            }
        }
        "flutter" => {
            let f = p.flutter.as_ref().unwrap();
            let mut command = argv(&[
                "flutter",
                "build",
                if p.platform == "ios" { "ios" } else { "apk" },
            ]);
            command.push(format!("--{}", f.mode));
            if p.target_kind == "simulator" {
                command.push("--simulator".into());
            }
            if let Some(flavor) = &f.flavor {
                command.extend(["--flavor".into(), flavor.clone()]);
            }
            if let Some(entry) = &f.entry_point {
                command.extend(["--target".into(), entry.clone()]);
            }
            for (key, value) in &f.dart_defines {
                command.push(format!("--dart-define={key}={value}"));
            }
            commands.push(command);
            selector.get_or_insert(crate::configuration::Artifact {
                path: None,
                glob: Some(if p.platform == "ios" {
                    format!(
                        "build/ios/{}/*.app",
                        if p.target_kind == "simulator" {
                            "iphonesimulator"
                        } else {
                            "iphoneos"
                        }
                    )
                } else {
                    format!(
                        "build/app/outputs/flutter-apk/app-{}{}.apk",
                        f.flavor.as_ref().map_or(String::new(), |v| format!("{v}-")),
                        f.mode
                    )
                }),
            });
        }
        "expo" => {
            let profile = &p.expo.as_ref().unwrap().profile;
            expo_profile(&cwd, profile, p)?;
            let executable = cwd.join("node_modules/.bin/eas");
            let artifact = output.join(if p.platform == "ios" {
                if p.target_kind == "simulator" {
                    "acme.tar.gz"
                } else {
                    "acme.ipa"
                }
            } else {
                "acme.apk"
            });
            commands.push(vec![
                executable.to_string_lossy().into(),
                "build".into(),
                "--local".into(),
                "--non-interactive".into(),
                "--platform".into(),
                p.platform.clone(),
                "--profile".into(),
                profile.clone(),
                "--output".into(),
                artifact.to_string_lossy().into(),
            ]);
            archive = p.platform == "ios";
            selector = Some(select(artifact));
        }
        _ => return Err(Error::invalid("Unknown builder")),
    }
    Ok(Plan {
        commands,
        cwd,
        selector: selector.ok_or_else(|| Error::invalid("Build requires an artifact selector"))?,
        archive,
    })
}
fn xcode(x: &crate::configuration::Xcode, p: &Profile, output: &Path) -> Vec<String> {
    let mut command = argv(&["/usr/bin/xcrun", "xcodebuild"]);
    if let Some(workspace) = &x.workspace {
        command.extend(["-workspace".into(), workspace.clone()]);
    }
    if let Some(project) = &x.project {
        command.extend(["-project".into(), project.clone()]);
    }
    command.extend([
        "-scheme".into(),
        x.scheme.clone(),
        "-configuration".into(),
        x.configuration.clone(),
        "-sdk".into(),
        x.sdk.clone().unwrap_or_else(|| {
            if p.target_kind == "simulator" {
                "iphonesimulator".into()
            } else {
                "iphoneos".into()
            }
        }),
        "-destination".into(),
        format!(
            "generic/platform={}",
            if p.target_kind == "simulator" {
                "iOS Simulator"
            } else {
                "iOS"
            }
        ),
        "-derivedDataPath".into(),
        output.join("derived").to_string_lossy().into(),
        format!("SYMROOT={}", output.join("products").display()),
    ]);
    if let Some(team) = &x.development_team {
        command.push(format!("DEVELOPMENT_TEAM={team}"));
    }
    command.extend(x.build_settings.iter().map(|(k, v)| format!("{k}={v}")));
    command.push("build".into());
    command
}
fn xcode_output(x: &crate::configuration::Xcode, p: &Profile, output: &Path) -> String {
    format!(
        "{}/{}-{}/*.app",
        output.join("products").display(),
        x.configuration,
        if p.target_kind == "simulator" {
            "iphonesimulator"
        } else {
            "iphoneos"
        }
    )
}
fn gradle(cwd: &Path, g: &crate::configuration::Gradle) -> Vec<String> {
    let mut command = vec![
        cwd.join("gradlew").to_string_lossy().into(),
        "--no-daemon".into(),
        "--console=plain".into(),
    ];
    command.extend(g.tasks.clone());
    command.extend(g.properties.iter().map(|(k, v)| format!("-P{k}={v}")));
    command
}
fn one_project(root: &Path, extension: &str) -> Result<Option<String>> {
    let entries = std::fs::read_dir(root)
        .map_err(|_| Error::new("CONFIG_PATH", "Native project directory is unavailable"))?;
    let mut paths = Vec::new();
    for entry in entries.take(4097) {
        let path = entry
            .map_err(|_| Error::new("CONFIG_PATH", "Cannot inspect native project"))?
            .path();
        if path.extension().and_then(|s| s.to_str()) == Some(extension) {
            paths.push(path.file_name().unwrap().to_string_lossy().into_owned());
        }
    }
    if paths.len() > 1 {
        return Err(Error::new(
            "BUILD_AMBIGUOUS",
            "Multiple native projects; use an explicit Xcode profile",
        ));
    }
    Ok(paths.pop())
}
fn expo_profile(cwd: &Path, name: &str, p: &Profile) -> Result<()> {
    let bytes = std::fs::read(cwd.join("eas.json"))
        .map_err(|_| Error::new("CONFIG_PATH", "Expo requires eas.json"))?;
    if bytes.len() > 1_048_576 {
        return Err(Error::invalid("eas.json exceeds 1 MiB"));
    }
    let file: Value =
        serde_json::from_slice(&bytes).map_err(|_| Error::invalid("Invalid eas.json"))?;
    let profile = &file["build"][name];
    if !profile.is_object() {
        return Err(Error::invalid("Expo profile is missing from eas.json"));
    }
    if profile.get("extends").is_some() {
        return Err(Error::new(
            "CAPABILITY_UNAVAILABLE",
            "Use a concrete EAS profile without extends for managed builds, or a custom command",
        ));
    }
    if p.platform == "ios" && p.target_kind == "simulator" && profile["ios"]["simulator"] != true {
        return Err(Error::invalid(
            "EAS iOS simulator profile requires ios.simulator: true",
        ));
    }
    if p.platform == "ios"
        && p.target_kind == "device"
        && profile["distribution"] != "internal"
        && profile["developmentClient"] != true
    {
        return Err(Error::invalid(
            "EAS physical iOS profile must use internal distribution or a development client",
        ));
    }
    if p.platform == "android"
        && profile["android"]["buildType"] != "apk"
        && profile["developmentClient"] != true
    {
        return Err(Error::invalid("EAS Android profile must produce an APK"));
    }
    Ok(())
}

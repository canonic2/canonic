//! Selected-device platform commands, structured inventory and installation verification.
use super::*;
use crate::process;
use std::time::Duration;

pub(super) struct Tools {
    pub xcrun: PathBuf,
    pub adb: PathBuf,
    pub aapt2: Option<PathBuf>,
    pub native_inventory: bool,
    pub play_protect: super::play_protect::Cadence,
}
impl Default for Tools {
    fn default() -> Self {
        Self {
            xcrun: crate::simulator::device_set::executable(),
            adb: crate::android::Android::default().adb,
            aapt2: std::env::var_os("TESTRUN_AAPT2").map(PathBuf::from),
            native_inventory: std::env::var_os("TESTRUN_XCRUN").is_none(),
            play_protect: super::play_protect::Cadence::default(),
        }
    }
}
struct JsonFile(PathBuf);
impl Drop for JsonFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
impl Tools {
    pub(super) async fn inventory(&self, target: &str) -> Result<Value> {
        let (kind, id) = target.split_once(':').unwrap();
        if kind == "device" && self.native_inventory {
            return super::physical::inventory(id).await;
        }
        let apps = if kind == "simulator" {
            let set = crate::simulator::device_set::DeviceSet::current()?;
            let id = set.uuid(target)?;
            let output = process::run(
                &self.xcrun,
                &set.arguments(&["listapps", id])
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                Duration::from_secs(15),
                1024 * 1024,
            )
            .await?;
            let value: plist::Value = plist::from_bytes(&output.bytes)
                .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid simulator app inventory"))?;
            value.as_dictionary().ok_or_else(|| Error::new("PLATFORM_PROTOCOL", "App inventory is not a dictionary"))?
                .iter().map(|(bundle, value)| {
                    let text = |key| value.as_dictionary().and_then(|v| v.get(key)).and_then(plist::Value::as_string).unwrap_or("");
                    json!({"appId":bundle,"name":text("CFBundleDisplayName"),"version":text("CFBundleShortVersionString"),"build":text("CFBundleVersion")})
                }).collect::<Vec<_>>()
        } else {
            let value = self
                .json(&[
                    "devicectl",
                    "device",
                    "info",
                    "apps",
                    "--device",
                    id,
                    "--include-all-apps",
                ])
                .await?;
            let apps = value["result"]["apps"].as_array().ok_or_else(|| {
                Error::new("PLATFORM_PROTOCOL", "Device app inventory omitted apps")
            })?;
            // `--include-all-apps` always lists system apps, so an empty list
            // means CoreDevice could not read the inventory, not that nothing is installed.
            if apps.is_empty() {
                return Err(Error::new(
                    "CAPABILITY_UNAVAILABLE",
                    "devicectl returned no apps, not even system apps; this device's installed-app inventory is unavailable through Apple tooling",
                ));
            }
            apps.iter().map(|app| {
                    let bundle = app["bundleIdentifier"].as_str().filter(|v| !v.is_empty())
                        .ok_or_else(|| Error::new("PLATFORM_PROTOCOL", "Device app inventory omitted bundle identifier"))?;
                    Ok(json!({"appId":bundle,"name":app["name"].as_str().unwrap_or(""),"version":app["version"].as_str().unwrap_or(""),"build":app["bundleVersion"].as_str().unwrap_or("")}))
                }).collect::<Result<Vec<_>>>()?
        };
        let mut apps = apps;
        apps.sort_by(|a, b| a["appId"].as_str().cmp(&b["appId"].as_str()));
        Ok(json!({"apps":apps}))
    }
    async fn json(&self, args: &[&str]) -> Result<Value> {
        let temp = JsonFile(
            std::env::temp_dir().join(format!("testrun-app-json-{}", uuid::Uuid::new_v4())),
        );
        crate::artifacts::directory(&temp.0)?;
        let path = temp.0.join("result.json");
        let mut args = args.to_vec();
        let mutation = args
            .get(2)
            .is_some_and(|s| matches!(*s, "install" | "uninstall"));
        args.extend([
            "--json-output",
            utf8(&path)?,
            "--timeout",
            if mutation { "120" } else { "10" },
        ]);
        process::run_diagnostic(
            &self.xcrun,
            &args,
            Duration::from_secs(if mutation { 125 } else { 15 }),
            1024 * 1024,
        )
        .await?;
        let metadata = std::fs::metadata(&path)
            .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Apple tool omitted JSON output"))?;
        if metadata.len() > 1024 * 1024 {
            return Err(Error::new("OUTPUT_LIMIT", "App result JSON is too large"));
        }
        serde_json::from_slice(
            &std::fs::read(path).map_err(|_| Error::new("HOST_IO", "Cannot read app result"))?,
        )
        .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid Apple app result JSON"))
    }
    async fn adb(&self, serial: &str, args: &[&str]) -> Result<String> {
        let mut all = vec!["-s", serial];
        all.extend_from_slice(args);
        let output =
            process::run_diagnostic(&self.adb, &all, Duration::from_secs(125), 1024 * 1024).await?;
        String::from_utf8(output.bytes)
            .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Android app result is not UTF-8"))
    }
    pub async fn check_target(&self, target: &str) -> Result<()> {
        let (kind, id) = target.split_once(':').unwrap();
        let present = match kind {
            "android" => crate::android::Android {
                adb: self.adb.clone(),
            }
            .devices()
            .await?
            .iter()
            .any(|d| d.serial == id && d.state == "device"),
            "device" => self.json(&["devicectl", "list", "devices"]).await?["result"]["devices"]
                .as_array()
                .is_some_and(|devices| {
                    devices.iter().any(|d| {
                        d["hardwareProperties"]["udid"] == id
                            && d["hardwareProperties"]["platform"] == "iOS"
                            && d["connectionProperties"]["transportType"] == "wired"
                    })
                }),
            "simulator" => {
                let set = crate::simulator::device_set::DeviceSet::current()?;
                let id = set.uuid(target)?;
                let output = process::run(
                    &self.xcrun,
                    &set.arguments(&["list", "devices", "--json"])
                        .iter()
                        .map(String::as_str)
                        .collect::<Vec<_>>(),
                    Duration::from_secs(15),
                    1024 * 1024,
                )
                .await?;
                let value: Value = serde_json::from_slice(&output.bytes)
                    .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid simulator inventory"))?;
                value["devices"].as_object().is_some_and(|groups| {
                    groups
                        .values()
                        .filter_map(Value::as_array)
                        .flatten()
                        .any(|d| d["udid"] == id && d["state"] == "Booted")
                })
            }
            _ => false,
        };
        if !present {
            return Err(Error::new(
                "TARGET_UNAVAILABLE",
                "The explicit target must be available and authorized; physical iOS requires USB and simulators must be booted",
            ));
        }
        Ok(())
    }
    pub async fn identity(&self, target: &str, path: &Path) -> Result<Identity> {
        if !target.starts_with("android:") {
            return package::ios_identity(path, target.starts_with("simulator:"));
        }
        if path.extension().and_then(|p| p.to_str()) != Some("apk") {
            return Err(Error::invalid("Android requires one .apk file"));
        }
        let tool = self.find_aapt2()?;
        let output = process::run_diagnostic(
            tool,
            &["dump", "badging", utf8(path)?],
            Duration::from_secs(20),
            1024 * 1024,
        )
        .await?;
        let text = String::from_utf8_lossy(&output.bytes);
        let line = text
            .lines()
            .find(|l| l.starts_with("package:"))
            .ok_or_else(|| Error::invalid("APK metadata is unavailable"))?;
        if line.contains(" split=") {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Split APK installation requires a complete split set; choose a single standalone APK",
            ));
        }
        let field = |name: &str| -> Result<String> {
            let needle = format!("{name}='");
            line.split_once(&needle)
                .and_then(|(_, s)| s.split_once('\''))
                .map(|(s, _)| s.to_owned())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| Error::invalid("APK omitted package/version metadata"))
        };
        Ok(Identity {
            bundle_id: field("name")?,
            version: field("versionName").unwrap_or_default(),
            build: field("versionCode")?,
        })
    }
    fn find_aapt2(&self) -> Result<PathBuf> {
        if let Some(path) = &self.aapt2 {
            return Ok(path.clone());
        }
        let adb = if self.adb.is_absolute() {
            Some(self.adb.clone())
        } else {
            std::env::var_os("PATH").and_then(|value| {
                std::env::split_paths(&value)
                    .map(|p| p.join(&self.adb))
                    .find(|p| p.is_file())
            })
        };
        let roots = [
            std::env::var_os("ANDROID_HOME").map(PathBuf::from),
            std::env::var_os("ANDROID_SDK_ROOT").map(PathBuf::from),
            adb.and_then(|p| p.parent()?.parent().map(Path::to_path_buf)),
        ];
        for root in roots.into_iter().flatten() {
            if let Ok(entries) = std::fs::read_dir(root.join("build-tools")) {
                let mut paths: Vec<_> = entries
                    .filter_map(std::result::Result::ok)
                    .take(100)
                    .map(|e| {
                        e.path()
                            .join(if cfg!(windows) { "aapt2.exe" } else { "aapt2" })
                    })
                    .filter(|p| p.is_file())
                    .collect();
                paths.sort();
                if let Some(path) = paths.pop() {
                    return Ok(path);
                }
            }
        }
        Err(Error::new(
            "PREREQUISITE_MISSING",
            "Android SDK Build Tools aapt2 is required to inspect APK identity; set ANDROID_HOME or TESTRUN_AAPT2",
        ))
    }
    pub async fn installed(&self, target: &str, bundle: &str) -> Result<Option<Identity>> {
        let (kind, id) = target.split_once(':').unwrap();
        if kind == "device" && self.native_inventory {
            let inventory = super::physical::inventory(id).await?;
            return Ok(inventory["apps"]
                .as_array()
                .unwrap()
                .iter()
                .find(|app| app["appId"] == bundle)
                .map(|app| Identity {
                    bundle_id: bundle.into(),
                    version: app["version"].as_str().unwrap_or("").into(),
                    build: app["build"].as_str().unwrap_or("").into(),
                }));
        }
        if kind == "android" {
            return self.installed_android(id, bundle).await;
        }
        if kind == "simulator" {
            let set = crate::simulator::device_set::DeviceSet::current()?;
            let id = set.uuid(target)?;
            let output = process::run(
                &self.xcrun,
                &set.arguments(&["listapps", id])
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                Duration::from_secs(15),
                1024 * 1024,
            )
            .await?;
            let value: plist::Value = plist::from_bytes(&output.bytes)
                .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid simulator app inventory"))?;
            let installed = value
                .as_dictionary()
                .and_then(|d| d.get(bundle))
                .and_then(plist::Value::as_dictionary)
                .map(|v| Identity {
                    bundle_id: bundle.into(),
                    version: v
                        .get("CFBundleShortVersionString")
                        .and_then(plist::Value::as_string)
                        .unwrap_or("")
                        .into(),
                    build: v
                        .get("CFBundleVersion")
                        .and_then(plist::Value::as_string)
                        .unwrap_or("")
                        .into(),
                });
            let Some(mut installed) = installed else {
                return Ok(None);
            };
            if installed.version.is_empty() || installed.build.is_empty() {
                // CoreSimulator's listapps can omit the marketing version and
                // encode the build as an integer. Inspect the selected app's
                // installed bundle instead of treating omitted metadata as a
                // version mismatch or accepting an unverifiable installation.
                installed = self.simulator_bundle(&set, id, bundle).await?;
            }
            return Ok(Some(installed));
        }
        let value = self
            .json(&[
                "devicectl",
                "device",
                "info",
                "apps",
                "--device",
                id,
                "--bundle-id",
                bundle,
                "--include-default-apps",
            ])
            .await?;
        let apps = value["result"]["apps"]
            .as_array()
            .ok_or_else(|| Error::new("PLATFORM_PROTOCOL", "Device app inventory omitted apps"))?;
        let found = apps
            .iter()
            .find(|v| v["bundleIdentifier"] == bundle)
            .map(|v| Identity {
                bundle_id: bundle.into(),
                version: v["version"].as_str().unwrap_or("").into(),
                build: v["bundleVersion"].as_str().unwrap_or("").into(),
            });
        if found.is_none() {
            // Absence is only trustworthy when the full inventory is readable.
            self.inventory(target).await?;
        }
        Ok(found)
    }
    async fn simulator_bundle(
        &self,
        set: &crate::simulator::device_set::DeviceSet,
        id: &str,
        bundle: &str,
    ) -> Result<Identity> {
        let output = process::run(
            &self.xcrun,
            &set.arguments(&["get_app_container", id, bundle, "app"])
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            Duration::from_secs(15),
            16384,
        )
        .await?;
        let path = std::str::from_utf8(&output.bytes)
            .map_err(|_| Error::new("PLATFORM_PROTOCOL", "Invalid installed app path"))?
            .trim();
        if !Path::new(path).is_absolute() {
            return Err(Error::new(
                "PLATFORM_PROTOCOL",
                "Installed app path must be absolute",
            ));
        }
        let identity = package::ios_identity(Path::new(path), true)?;
        if identity.bundle_id != bundle {
            return Err(Error::new(
                "APP_VERIFICATION_FAILED",
                "Installed bundle identity does not match the selected app",
            ));
        }
        Ok(identity)
    }
    async fn installed_android(&self, id: &str, bundle: &str) -> Result<Option<Identity>> {
        let adapter = crate::android::Android {
            adb: self.adb.clone(),
        };
        let bytes = adapter
            .shell(
                id,
                &["cmd", "package", "list", "packages", bundle],
                Duration::from_secs(15),
                1024 * 1024,
            )
            .await?;
        if !String::from_utf8_lossy(&bytes)
            .lines()
            .any(|l| l.trim() == format!("package:{bundle}"))
        {
            return Ok(None);
        }
        let bytes = adapter
            .shell(
                id,
                &["dumpsys", "package", bundle],
                Duration::from_secs(15),
                1024 * 1024,
            )
            .await?;
        let text = String::from_utf8_lossy(&bytes);
        let code = regex::Regex::new(r"\bversionCode=(\d+)")
            .unwrap()
            .captures(&text)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_owned())
            .ok_or_else(|| {
                Error::new(
                    "PLATFORM_PROTOCOL",
                    "Installed Android version code is unavailable",
                )
            })?;
        let version = text
            .lines()
            .find_map(|l| l.trim().strip_prefix("versionName="))
            .unwrap_or("")
            .trim()
            .to_owned();
        Ok(Some(Identity {
            bundle_id: bundle.into(),
            version,
            build: code,
        }))
    }
    pub async fn mutate(
        &self,
        request: &Request,
        operation: &str,
        path: Option<&Path>,
        bundle: &str,
    ) -> Result<Value> {
        let (kind, id) = request.target_id.split_once(':').unwrap();
        if kind == "android" {
            let args = if operation == "install" {
                let mut args = vec!["install", "--no-streaming"];
                if request.replace {
                    args.push("-r");
                }
                args.push(utf8(path.unwrap())?);
                args
            } else {
                vec!["uninstall", bundle]
            };
            let answers = std::sync::Mutex::new(Vec::new());
            let output = if operation == "install" {
                let android = crate::android::Android {
                    adb: self.adb.clone(),
                };
                // Dropping the install stops the adb client; the device may still finish it.
                tokio::select! {
                    output = self.adb(id, &args) => output?,
                    error = super::play_protect::watch(&android, id, request.play_protect, self.play_protect, &answers) => {
                        let mut error = error;
                        error.details.as_mut().unwrap()["answers"] = json!(*answers.lock().unwrap());
                        return Err(error);
                    }
                }
            } else {
                self.adb(id, &args).await?
            };
            if !output.lines().any(|l| l.trim() == "Success") {
                return Err(Error::new(
                    "APP_PLATFORM_REJECTED",
                    "Android did not report successful package installation/removal",
                )
                .details(json!({"diagnostic":output.chars().take(4096).collect::<String>()})));
            }
            let answers = answers.into_inner().unwrap();
            if !answers.is_empty() {
                return Ok(json!({"prompt":"play_protect_security_check","answers":answers}));
            }
        } else if kind == "simulator" {
            let set = crate::simulator::device_set::DeviceSet::current()?;
            let id = set.uuid(&request.target_id)?;
            process::run_diagnostic(
                &self.xcrun,
                &set.arguments(&[
                    operation,
                    id,
                    if operation == "install" {
                        utf8(path.unwrap())?
                    } else {
                        bundle
                    },
                ])
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
                Duration::from_secs(125),
                1024 * 1024,
            )
            .await?;
        } else {
            let result = self
                .json(&[
                    "devicectl",
                    "device",
                    operation,
                    "app",
                    "--device",
                    id,
                    if operation == "install" {
                        utf8(path.unwrap())?
                    } else {
                        bundle
                    },
                ])
                .await?;
            if result["info"]["outcome"] != "success" {
                return Err(Error::new(
                    "APP_PLATFORM_REJECTED",
                    "Apple device service did not confirm the operation",
                ));
            }
        }
        Ok(Value::Null)
    }
    /// For an Android install of uncertain outcome: whether the installed base
    /// APK has the submitted package's SHA-256. Other targets have no digest route.
    pub async fn reconcile(&self, target: &str, bundle: &str, submitted: &str) -> Option<Value> {
        let serial = target.strip_prefix("android:")?;
        Some(super::play_protect::reconcile(&self.adb, serial, bundle, submitted).await)
    }
}
fn utf8(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| Error::invalid("App path must be UTF-8"))
}

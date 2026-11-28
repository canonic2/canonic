//! Disposable native targets, isolated from the user's simulator and AVD sets.
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub(super) struct Target {
    pub id: String,
    set: PathBuf,
    uuid: Option<String>,
}
impl Target {
    #[cfg(target_os = "macos")]
    pub fn app_data(&self, app: &str) -> PathBuf {
        let output = Command::new("/usr/bin/xcrun")
            .arg("simctl")
            .arg("--set")
            .arg(&self.set)
            .args([
                "get_app_container",
                self.uuid.as_ref().unwrap(),
                app,
                "data",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
    }
    pub fn select_simulator(&mut self, listing: &Value) {
        let uuid = self.uuid.as_ref().unwrap();
        self.id = listing["targets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|target| {
                target["targetId"]
                    .as_str()
                    .is_some_and(|id| id.ends_with(&format!(":{uuid}")))
            })
            .expect("Discover disposable simulator in its isolated device set")["targetId"]
            .as_str()
            .unwrap()
            .to_owned();
    }
    #[cfg(target_os = "macos")]
    pub fn ios(root: &Path) -> Self {
        let set = root.join("simulators");
        std::fs::create_dir_all(&set).unwrap();
        let output = Command::new("/usr/bin/xcrun")
            .args(["simctl", "list", "runtimes", "--json"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let inventory: Value = serde_json::from_slice(&output.stdout).unwrap();
        let runtime = inventory["runtimes"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|r| {
                r["isAvailable"] == true
                    && r["identifier"]
                        .as_str()
                        .is_some_and(|id| id.contains(".iOS-"))
            })
            .unwrap()["identifier"]
            .as_str()
            .unwrap();
        let name = format!("Testrun-QA-Build-{}", uuid::Uuid::new_v4().simple());
        let output = Command::new("/usr/bin/xcrun")
            .arg("simctl")
            .arg("--set")
            .arg(&set)
            .args([
                "create",
                &name,
                "com.apple.CoreSimulator.SimDeviceType.iPhone-16",
                runtime,
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let uuid = String::from_utf8(output.stdout).unwrap().trim().to_owned();
        Self {
            id: format!("simulator:{uuid}"),
            set,
            uuid: Some(uuid),
        }
    }
    pub fn android(root: &Path, sdk: &Path) -> Self {
        let set = root.join("avd");
        std::fs::create_dir_all(&set).unwrap();
        let name = format!("Testrun-QA-Build-{}", uuid::Uuid::new_v4().simple());
        let output = Command::new(sdk.join("cmdline-tools/latest/bin/avdmanager"))
            .args([
                "create",
                "avd",
                "--name",
                &name,
                "--package",
                "system-images;android-36;google_apis_playstore;x86_64",
                "--device",
                "pixel_6",
            ])
            .env("ANDROID_AVD_HOME", &set)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let config = set.join(format!("{name}.avd/config.ini"));
        let contents = std::fs::read_to_string(&config).unwrap();
        let mut contents = contents
            .lines()
            .filter(|line| {
                ![
                    "hw.lcd.width",
                    "hw.lcd.height",
                    "hw.lcd.density",
                    "skin.name",
                    "skin.path",
                ]
                .iter()
                .any(|key| line.starts_with(&format!("{key}=")))
            })
            .collect::<Vec<_>>()
            .join("\n");
        contents.push_str("\nhw.lcd.width=720\nhw.lcd.height=1280\nhw.lcd.density=320\nskin.name=720x1280\nskin.path=_no_skin\n");
        std::fs::write(config, contents).unwrap();
        Self {
            id: format!("android:avd:{name}"),
            set,
            uuid: None,
        }
    }
}
impl Drop for Target {
    fn drop(&mut self) {
        if let Some(uuid) = &self.uuid {
            let _ = Command::new("/usr/bin/xcrun")
                .arg("simctl")
                .arg("--set")
                .arg(&self.set)
                .args(["shutdown", uuid])
                .output();
            let result = Command::new("/usr/bin/xcrun")
                .arg("simctl")
                .arg("--set")
                .arg(&self.set)
                .args(["delete", uuid])
                .output();
            if !result.is_ok_and(|output| output.status.success()) {
                eprintln!("Disposable build simulator deletion failed: {uuid}");
            }
        }
        // Android is started read-only by Testrun and stopped by disconnect;
        // the enclosing Workspace owns this isolated AVD definition directory.
    }
}

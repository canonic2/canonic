//! Native unpacked extension lifecycle using connection-owned directory fixtures.
use super::*;
use crate::browser::{directory::Directory, extensions::Action};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Default)]
pub(super) struct Extensions {
    installed: BTreeMap<Uuid, Extension>,
}

impl Extensions {
    pub(super) fn possible_registrations(&self) -> Vec<Uuid> {
        self.installed.keys().copied().collect()
    }
}

#[derive(Clone)]
struct Extension {
    source: Directory,
    native: Option<String>,
    has_action: bool,
}

fn extension_id(value: &Value) -> Result<&str, Error> {
    value
        .as_str()
        .filter(|id| id.len() == 32 && id.bytes().all(|byte| (b'a'..=b'p').contains(&byte)))
        .ok_or_else(|| Error::from("EXTENSION_IDENTITY_UNAVAILABLE"))
}

fn extension_at<'a>(extensions: &'a [Value], source: &Path) -> Option<&'a Value> {
    extensions.iter().find(|entry| {
        entry["path"]
            .as_str()
            .is_some_and(|path| Path::new(path) == source)
    })
}

fn manifest_id(manifest: &Value) -> Result<Option<String>, Error> {
    let Some(key) = manifest.get("key") else {
        return Ok(None);
    };
    let key = key.as_str().ok_or("EXTENSION_KEY_INVALID")?;
    let key = key
        .lines()
        .filter(|line| !line.trim().starts_with("-----"))
        .flat_map(str::split_ascii_whitespace)
        .collect::<String>();
    let key = STANDARD.decode(key).map_err(|_| "EXTENSION_KEY_INVALID")?;
    if key.is_empty() {
        return Err("EXTENSION_KEY_INVALID".into());
    }
    let id = Sha256::digest(key)[..16]
        .iter()
        .flat_map(|&byte| {
            [
                char::from(b'a' + (byte >> 4)),
                char::from(b'a' + (byte & 15)),
            ]
        })
        .collect();
    Ok(Some(id))
}

fn summary(id: Uuid, extension: &Extension, observed: Option<&Value>) -> Value {
    observed.map_or_else(
        || json!({"extensionId":id,"source":extension.source.summary["reference"],"state":"not_observed"}),
        |entry| json!({"extensionId":id,"source":extension.source.summary["reference"],"state":"installed","name":entry["name"],"version":entry["version"],"enabled":entry["enabled"]}),
    )
}

impl Worker {
    async fn extension_registry(
        &mut self,
        id: Uuid,
        deadline: Instant,
    ) -> Result<Vec<Value>, Error> {
        let mut result = self
            .call_read(id, None, "Extensions.getExtensions", json!({}), deadline)
            .await?;
        match result["extensions"].take() {
            Value::Array(items) => Ok(items),
            _ => Err("EXTENSION_REGISTRY_UNAVAILABLE".into()),
        }
    }

    async fn extension_load(
        &mut self,
        id: Uuid,
        source: &Path,
        deadline: Instant,
    ) -> Result<String, Error> {
        let result = self
            .call(
                id,
                None,
                "Extensions.loadUnpacked",
                json!({"path":source}),
                deadline,
            )
            .await?;
        Ok(extension_id(&result["id"])?.into())
    }

    async fn extension_install(
        &mut self,
        id: Uuid,
        directory: Reference,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if self.extensions.installed.len() >= 16 {
            return Err("EXTENSION_CAPACITY".into());
        }
        self.transfer_scope(directory)?;
        let mut source = self
            .transfers
            .directories
            .get(&directory)
            .ok_or("DIRECTORY_FIXTURE_REQUIRED")?
            .clone();
        crate::browser::directory::verify(source.clone()).await?;
        source.path = std::fs::canonicalize(&source.path).map_err(|_| "DIRECTORY_CHANGED")?;
        let manifest =
            crate::artifacts::read_private_file(&source.path.join("manifest.json"), 64 * 1024)
                .map_err(|_| "EXTENSION_MANIFEST_UNAVAILABLE")?;
        let manifest: Value =
            serde_json::from_slice(&manifest).map_err(|_| "EXTENSION_MANIFEST_INVALID")?;
        if !manifest.is_object() {
            return Err("EXTENSION_MANIFEST_INVALID".into());
        }
        let declared_id = manifest_id(&manifest)?;
        let before = self.extension_registry(id, deadline).await?;
        if extension_at(&before, &source.path).is_some()
            || self
                .extensions
                .installed
                .values()
                .any(|extension| extension.source.path == source.path)
        {
            return Err("EXTENSION_SOURCE_ALREADY_IN_USE".into());
        }
        if declared_id
            .as_ref()
            .is_some_and(|id| before.iter().any(|entry| entry["id"] == *id))
        {
            return Err("EXTENSION_ID_ALREADY_IN_USE".into());
        }
        let extension_id = Uuid::new_v4();
        let path = source.path.clone();
        self.extensions.installed.insert(
            extension_id,
            Extension {
                source,
                native: None,
                has_action: ["action", "browser_action", "page_action"]
                    .iter()
                    .any(|field| manifest[*field].is_object()),
            },
        );
        let native = match self.extension_load(id, &path, deadline).await {
            Ok(native) => native,
            Err(mut error) => {
                if matches!(
                    error.code,
                    "BROWSER_METHOD_FAILED" | "NOT_DISPATCHED" | "CANCELLED"
                ) {
                    self.extensions.installed.remove(&extension_id);
                } else {
                    error.partial = Some(
                        json!({"extensionId":extension_id,"replayed":false,"cleanupPending":true}),
                    );
                }
                return Err(error);
            }
        };
        self.extensions
            .installed
            .get_mut(&extension_id)
            .expect("Retained extension load")
            .native = Some(native.clone());
        if declared_id.is_some_and(|expected| expected != native) {
            let mut error = Error::from("EXTENSION_IDENTITY_CHANGED");
            error.partial = Some(json!({"extensionId":extension_id,"cleanupPending":true}));
            return Err(error);
        }
        let observed = self.extension_registry(id, deadline).await?;
        let item = extension_at(&observed, &path).filter(|entry| entry["id"] == native);
        Ok(
            json!({"extension":summary(extension_id,&self.extensions.installed[&extension_id],item)}),
        )
    }

    async fn extension_list(
        &mut self,
        id: Uuid,
        cursor: Option<&str>,
        limit: Option<u64>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let observed = self.extension_registry(id, deadline).await?;
        let mut items = Vec::new();
        for (id, extension) in &mut self.extensions.installed {
            let item = extension_at(&observed, &extension.source.path);
            if extension.native.is_none()
                && let Some(native) = item.and_then(|row| extension_id(&row["id"]).ok())
            {
                extension.native = Some(native.into());
            }
            items.push((
                *id,
                summary(
                    *id,
                    extension,
                    item.filter(|item| item["id"].as_str() == extension.native.as_deref()),
                ),
            ));
        }
        self.page_profile_apps("extensions", cursor, limit, items)
    }

    pub(super) async fn extension_operation(
        &mut self,
        id: Uuid,
        action: Action,
        deadline: Instant,
    ) -> Result<Value, Error> {
        if !self.managed_apps {
            return Err("NATIVE_EXTENSION_OWNED_PROFILE_REQUIRED".into());
        }
        if !self.extension_profile {
            return Err("EXTENSION_REGULAR_CONTEXT_REQUIRED".into());
        }
        let context = self
            .persistent_context
            .ok_or("EXTENSION_REGULAR_CONTEXT_REQUIRED")?;
        if let Action::TriggerAction { page, .. } = &action
            && self.index.retained_page_context(*page) != Some(context)
        {
            return Err("EXTENSION_REGULAR_CONTEXT_REQUIRED".into());
        }
        let extension_id = match action {
            Action::Install { directory } => {
                return self.extension_install(id, directory, deadline).await;
            }
            Action::List { cursor, limit } => {
                return self
                    .extension_list(id, cursor.as_deref(), limit, deadline)
                    .await;
            }
            Action::Reload { extension_id }
            | Action::TriggerAction { extension_id, .. }
            | Action::Uninstall { extension_id } => extension_id,
        };
        let extension = self
            .extensions
            .installed
            .get(&extension_id)
            .ok_or("EXTENSION_REFERENCE_UNKNOWN")?
            .clone();
        let native = extension
            .native
            .as_deref()
            .ok_or("EXTENSION_INSTALL_OUTCOME_UNKNOWN")?;
        let registry = self.extension_registry(id, deadline).await?;
        let item = extension_at(&registry, &extension.source.path);
        if item.is_some_and(|entry| entry["id"] != native) {
            return Err("EXTENSION_IDENTITY_CHANGED".into());
        }
        if item.is_none() && !matches!(action, Action::Uninstall { .. }) {
            return Err("EXTENSION_NOT_INSTALLED".into());
        }
        match action {
            Action::Reload { .. } => {
                crate::browser::directory::verify(extension.source.clone()).await?;
                if self
                    .extension_load(id, &extension.source.path, deadline)
                    .await?
                    != native
                {
                    return Err("EXTENSION_IDENTITY_CHANGED".into());
                }
                Ok(json!({"extensionId":extension_id,"reloaded":true}))
            }
            Action::TriggerAction { page, .. } => {
                if !extension.has_action || !item.is_some_and(|row| row["enabled"] == true) {
                    return Err("EXTENSION_ACTION_UNAVAILABLE".into());
                }
                let tab = self.app_tab_for_page(id, page, deadline).await?;
                self.call(
                    id,
                    None,
                    "Target.activateTarget",
                    json!({"targetId":tab}),
                    deadline,
                )
                .await?;
                self.call(
                    id,
                    None,
                    "Extensions.triggerAction",
                    json!({"id":native,"targetId":tab}),
                    deadline,
                )
                .await?;
                Ok(json!({"extensionId":extension_id,"page":page,"dispatched":true}))
            }
            Action::Uninstall { .. } => {
                if item.is_some() {
                    self.call(
                        id,
                        None,
                        "Extensions.uninstall",
                        json!({"id":native}),
                        deadline,
                    )
                    .await?;
                }
                self.extension_workers_released(id, native, deadline)
                    .await?;
                self.extensions.installed.remove(&extension_id);
                Ok(json!({"extensionId":extension_id,"removed":true}))
            }
            _ => unreachable!("Extension mutation selected above"),
        }
    }

    async fn extension_workers_released(
        &mut self,
        id: Uuid,
        extension: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        let prefix = format!("chrome-extension://{extension}/");
        loop {
            let targets = self
                .call_read(
                    id,
                    None,
                    "Target.getTargets",
                    json!({"filter":[{}]}),
                    deadline,
                )
                .await?;
            let remaining = targets["targetInfos"]
                .as_array()
                .ok_or("BACKEND_PROTOCOL")?
                .iter()
                .any(|target| {
                    matches!(
                        target["type"].as_str(),
                        Some("background_page" | "service_worker" | "worker")
                    ) && target["url"]
                        .as_str()
                        .is_some_and(|url| url.starts_with(&prefix))
                });
            if !remaining {
                return Ok(());
            }
            let wake = Instant::now() + Duration::from_millis(25);
            if wake >= deadline {
                return Err("EXTENSION_CLEANUP_PENDING".into());
            }
            tokio::time::sleep_until(wake).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_extension_manifest_key_resolves_to_its_chromium_id() {
        assert_eq!(manifest_id(&json!({})).unwrap(), None);
        assert_eq!(
            manifest_id(&json!({"key":"YWJj"})).unwrap().as_deref(),
            Some("lkhibglpipabmpokebebeanofnkocccd")
        );
        assert_eq!(
            manifest_id(&json!({"key":"invalid!"})).unwrap_err().code,
            "EXTENSION_KEY_INVALID"
        );
    }
}

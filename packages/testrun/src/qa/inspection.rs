//! Application inventory, selected-device clipboard reads and bounded accessibility evidence.
use super::arguments::{only_fields, parse, session_ref};
use super::{ReadContext, Server, id};
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::time::Duration;

impl ReadContext {
    pub(super) async fn observe(&self, name: &str, args: Value) -> Result<Value> {
        if name == "app_list" {
            only_fields(
                &args,
                &[
                    "runId",
                    "goalId",
                    "sessionId",
                    "all",
                    "name",
                    "cursor",
                    "limit",
                ],
            )?;
        } else {
            only_fields(&args, &["runId", "goalId", "sessionId"])?;
        }
        let search: Option<crate::ui::TextMatch> = match args.get("name") {
            Some(name) if !name.is_null() => Some(parse(name.clone())?),
            _ => None,
        };
        if let Some(search) = &search {
            search.matches_observed("", false)?;
        }
        let target = self.live_target().await?;
        if target.kind == "android" && name != "app_list" {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Android clipboard observation adapter is unavailable",
            ));
        }
        if target.kind == "device" && name != "app_list" {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Physical clipboard observation is unavailable",
            ));
        }
        if name == "app_list" {
            let inventory = if target.kind == "android" {
                crate::android::Android::default()
                    .apps(&target.platform_id)
                    .await?
            } else {
                crate::applications::inventory(&target.identity.target_id).await?
            };
            let (mut apps, hidden) = select_apps(&inventory, args["all"] == true, search.as_ref())?;
            apps.sort_by_cached_key(|app| {
                crate::pagination::identity_key(app["appId"].as_str().expect("app ID"))
            });
            let total = apps.len();
            let page = crate::pagination::collect(
                "app_list",
                &(&target.identity.target_id, &args["all"], &args["name"]),
                &args,
                apps.into_iter().map(|app| {
                    (
                        crate::pagination::identity_key(app["appId"].as_str().expect("app ID")),
                        app,
                    )
                }),
                Ok,
                "app_list with limit 1",
            )?;
            self.verified("app_list", json!({}));
            let mut result = json!({"apps":page.items,"total":total,"nextCursor":page.next_cursor});
            if hidden > 0 {
                result["hiddenNotLaunchable"] = json!(hidden);
            }
            for field in ["unavailableFields", "namesUnavailable"] {
                if let Some(value) = inventory.get(field) {
                    result[field] = value.clone();
                }
            }
            return Ok(result);
        }
        let output = crate::simulator::device_set::run(
            &["pbpaste", &target.platform_id],
            Duration::from_secs(15),
            1024 * 1024,
        )
        .await?;
        Ok(
            json!({"text":String::from_utf8(output.bytes).map_err(|_| Error::new("PLATFORM_PROTOCOL","Clipboard is not UTF-8"))?}),
        )
    }

    pub(super) async fn accessibility(&self, name: &str, args: Value) -> Result<Value> {
        only_fields(
            &args,
            &[
                "runId",
                "goalId",
                "sessionId",
                "scope",
                "query",
                "limit",
                "cursor",
                "allowPartial",
            ],
        )?;
        session_ref(&args)?;
        if name == "ui_find" {
            let query: crate::ui::Query = parse(args["query"].clone())?;
            query.validate()?;
        }
        let target = self.live_target().await?;
        if target.kind != "device" && args["scope"]["kind"] != "display" {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Native accessibility currently supports the main display's frontmost app scope",
            ));
        }
        let mut snapshot = self.accessibility_snapshot(&target, &args).await?;
        if target.kind == "simulator" {
            self.incarnation().await?;
        }
        crate::ui::normalize_roles(&mut snapshot["root"]);
        snapshot["sessionId"] = args["sessionId"].clone();
        snapshot["targetId"] = json!(target.identity.target_id);
        self.verified(
            "accessibility",
            json!({"partial":snapshot["quality"]!="complete"}),
        );
        snapshot["frameUnit"] = json!(if target.kind == "android" {
            "pixels"
        } else {
            "points"
        });
        snapshot["display"] = self.display_extent(&target, &snapshot).await?;
        if snapshot["quality"] != "complete"
            && args.get("allowPartial").and_then(Value::as_bool) != Some(true)
        {
            return Err(Error::new(
                "PARTIAL_UI_TREE",
                "Host translation covers the frontmost app only; explicitly allow partial observations",
            ));
        }
        let snapshot_id = snapshot
            .get("snapshotId")
            .and_then(Value::as_str)
            .map_or_else(id, str::to_owned);
        let path = self.evidence.join(format!("ui-{snapshot_id}.json"));
        let bytes = serde_json::to_vec(&snapshot)
            .map_err(|_| Error::new("INTERNAL", "Cannot encode accessibility artifact"))?;
        crate::artifacts::write(&path, &bytes)?;
        let data = if name == "ui_find" {
            let query: crate::ui::Query = parse(
                args.get("query")
                    .cloned()
                    .ok_or_else(|| Error::invalid("query is required"))?,
            )?;
            let mut found = crate::pagination::ui::find(
                &snapshot["root"],
                &query,
                &args,
                &json!([
                    target.identity.target_id,
                    args["scope"],
                    args["allowPartial"]
                ]),
            )?;
            crate::ui::add_centers(&mut found["matches"], &snapshot["display"]);
            if found["matches"].as_array().is_some_and(Vec::is_empty)
                && scrollable(&snapshot["root"])
            {
                found["hint"] = json!(
                    "No on-screen match. Lists may expose only nearby rows; swipe the list, then search again."
                );
            }
            found
        } else {
            crate::pagination::ui::snapshot(
                &snapshot["root"],
                &args,
                &json!([
                    target.identity.target_id,
                    args["scope"],
                    args["allowPartial"]
                ]),
            )?
        };
        Ok(
            json!({"snapshotId":snapshot_id,"target":target.identity,"quality":snapshot["quality"],"frameUnit":snapshot["frameUnit"],"display":snapshot["display"],"coverage":snapshot.get("coverage").unwrap_or(&json!("app_exposed_tree")),"artifactPath":path,"nextCursor":data["nextCursor"],"data":data}),
        )
    }

    /// The display that point input normalizes against, in the frames' unit,
    /// or null when this observation cannot establish it.
    async fn display_extent(
        &self,
        target: &crate::targets::Target,
        snapshot: &Value,
    ) -> Result<Value> {
        let rectangle = |frame: &Value| json!({"x":frame["x"],"y":frame["y"],"width":frame["width"],"height":frame["height"]});
        Ok(match target.kind.as_str() {
            // Geometry enriches the tree; secure-screen capture refusal must
            // not discard accessible nodes. Null also suppresses tap centers.
            "android" => crate::android::Android::default()
                .dimensions(&target.platform_id)
                .await
                .map(|(width, height)| json!({"x":0,"y":0,"width":width,"height":height}))
                .unwrap_or(Value::Null),
            // The runner reports SpringBoard's frame; an older runner reports none.
            "device" => snapshot
                .get("display")
                .filter(|display| display["width"].is_number())
                .map_or(Value::Null, rectangle),
            // Simulator element taps already require a full-display root.
            _ => {
                let frame = &snapshot["root"]["frame"];
                if frame["x"].as_f64() == Some(0.0) && frame["y"].as_f64() == Some(0.0) {
                    rectangle(frame)
                } else {
                    Value::Null
                }
            }
        })
    }

    pub(super) async fn accessibility_snapshot(
        &self,
        target: &crate::targets::Target,
        args: &Value,
    ) -> Result<Value> {
        match target.kind.as_str() {
            "android" => {
                let adapter = crate::android::Android::default();
                let mut snapshot = adapter.snapshot(&target.platform_id).await?;
                snapshot["foregroundApp"] =
                    json!(adapter.focused_package(&target.platform_id).await?);
                Ok(snapshot)
            }
            "device" => Ok(self
                .runner
                .as_ref()
                .map_err(Clone::clone)?
                .call(
                    "ui_snapshot",
                    json!({"scope":crate::mobile::runner_scope(&args["scope"])?}),
                    None,
                    Duration::from_secs(5),
                )
                .await?
                .data),
            _ => crate::native::snapshot(&target.platform_id).await,
        }
    }
}

/// Whether the tree holds a scrolling container, whose off-screen rows may
/// not be built yet.
fn scrollable(node: &Value) -> bool {
    node["role"] == "scroll_view"
        || node["states"]["scrollable"] == true
        || node["children"]
            .as_array()
            .is_some_and(|children| children.iter().any(scrollable))
}

/// The apps an `app_list` call shows. Without `all`, an app that reports it
/// cannot be opened is hidden; an app that reports nothing either way stays.
/// A name search matches the display name or the app ID. Returns the
/// selection and how many apps were hidden as not launchable.
fn select_apps(
    inventory: &Value,
    all: bool,
    search: Option<&crate::ui::TextMatch>,
) -> Result<(Vec<Value>, usize)> {
    let apps = inventory["apps"]
        .as_array()
        .ok_or_else(|| Error::new("PLATFORM_PROTOCOL", "Invalid app inventory"))?;
    let mut hidden = 0;
    let mut selected = Vec::new();
    for app in apps {
        if let Some(search) = search {
            let text = |field: &str| app[field].as_str().unwrap_or_default();
            if search.matches_observed(text("name"), false)? != Some(true)
                && search.matches_observed(text("appId"), false)? != Some(true)
            {
                continue;
            }
        }
        if !all && app["launchable"] == false {
            hidden += 1;
            continue;
        }
        selected.push(app.clone());
    }
    Ok((selected, hidden))
}

impl Server {
    pub(super) async fn observe(&mut self, name: &str, args: Value) -> Result<Value> {
        self.read_context(&args)?.observe(name, args).await
    }
    pub(super) async fn accessibility(&mut self, name: &str, args: Value) -> Result<Value> {
        self.read_context(&args)?.accessibility(name, args).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrolling_containers_are_found_at_any_depth() {
        let list = json!({"role":"application","children":[{"role":"element","children":[{"role":"scroll_view","children":[]}]}]});
        assert!(scrollable(&list));
        let android = json!({"role":"element","states":{"scrollable":true}});
        assert!(scrollable(&android));
        assert!(!scrollable(
            &json!({"role":"application","children":[{"role":"button"}]})
        ));
    }
    #[test]
    fn apps_list_hides_unopenable_apps_and_finds_home_screen_names() {
        let inventory = json!({"apps":[
            {"appId":"com.example.acme.game","name":"Acme Quest","launchable":true},
            {"appId":"com.example.acme.service","name":"Acme Service","launchable":false},
            {"appId":"com.example.notes","name":"Notes"}
        ]});
        let ids = |apps: Vec<Value>| {
            apps.iter()
                .map(|app| app["appId"].clone())
                .collect::<Vec<_>>()
        };
        let (apps, hidden) = select_apps(&inventory, false, None).unwrap();
        assert_eq!(hidden, 1);
        // An app that reports nothing about launching, as iOS does, stays listed.
        assert_eq!(
            ids(apps),
            [json!("com.example.acme.game"), json!("com.example.notes")]
        );
        let (apps, hidden) = select_apps(&inventory, true, None).unwrap();
        assert_eq!((apps.len(), hidden), (3, 0));
        // The search reads the name a person sees, then the ID.
        let quest: crate::ui::TextMatch =
            serde_json::from_value(json!({"text":"acme quest","mode":"substring"})).unwrap();
        let (apps, _) = select_apps(&inventory, false, Some(&quest)).unwrap();
        assert_eq!(ids(apps), [json!("com.example.acme.game")]);
        let id: crate::ui::TextMatch =
            serde_json::from_value(json!({"text":"notes","mode":"substring"})).unwrap();
        assert_eq!(
            select_apps(&inventory, false, Some(&id)).unwrap().0.len(),
            1
        );
        // A search for an app that cannot be opened finds it only with all.
        let service: crate::ui::TextMatch =
            serde_json::from_value(json!({"text":"Acme Service","mode":"exact"})).unwrap();
        assert_eq!(
            select_apps(&inventory, false, Some(&service)).unwrap(),
            (vec![], 1)
        );
        assert_eq!(
            select_apps(&inventory, true, Some(&service))
                .unwrap()
                .0
                .len(),
            1
        );
    }
}

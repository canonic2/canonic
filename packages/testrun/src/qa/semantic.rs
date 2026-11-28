//! Managed bounded semantic reads, freshness refusal and independently admitted waits.
use super::{ReadContext, Server, arguments::only_fields};
use crate::{
    error::{Error, Result},
    ui::semantic::{Condition, Target, parse_target, parse_wait},
};
use serde_json::{Value, json};
use std::time::Duration;

impl Server {
    pub(super) async fn resolve_semantic_input(&self, name: &str, args: Value) -> Result<Value> {
        if name == "type_text" && args["replace"] == true {
            let context = self.read_context(&args)?;
            if context.target.kind == "android" {
                return context.resolve_text(args).await;
            }
        }
        if matches!(name, "tap" | "long_press") && args.get("element").is_some() {
            let context = self.read_context(&args)?;
            crate::mobile::validate_backend_options(&context.target.kind, None, name, &args)?;
            context.resolve_pointer(args).await
        } else {
            Ok(args)
        }
    }
}

impl ReadContext {
    async fn resolve_text(&self, mut args: Value) -> Result<Value> {
        let target = parse_target(&args["element"])?;
        let fresh = self.semantic_snapshot(&args).await?;
        let element = self.resolve(&target, &fresh, &args)?;
        if !matches!(
            element["role"].as_str(),
            Some("text_field" | "secure_text_field")
        ) || element["states"]["focused"] != true
            || element["states"]["enabled"] != true
        {
            return Err(Error::new(
                "FOCUS_NOT_PROVEN",
                "Replacement requires the uniquely resolved field to be focused and enabled",
            ));
        }
        let index = element["elementId"]
            .as_str()
            .and_then(|reference| reference.strip_prefix("node-"))
            .and_then(|index| index.parse::<u32>().ok())
            .filter(|index| *index > 0)
            .ok_or_else(|| Error::new("STALE_ELEMENT", "Native field identity is unavailable"))?;
        let digest = fresh["nativeDigest"]
            .as_str()
            .ok_or_else(|| Error::new("STALE_ELEMENT", "Native tree proof is unavailable"))?;
        args["textProof"] = json!({"index":index,"digest":digest});
        Ok(args)
    }
    pub(super) async fn resolve_pointer(&self, mut args: Value) -> Result<Value> {
        crate::ui::semantic::validate_pointer(&args)?;
        let target = parse_target(&args["element"])?;
        let fresh = self.semantic_snapshot(&args).await?;
        let element = self.resolve(&target, &fresh, &args)?;
        let geometry = args["display"]["geometryVersion"].as_str().unwrap_or("");
        let (width, height) = geometry
            .split_once('x')
            .and_then(|(w, h)| Some((w.parse::<f64>().ok()?, h.parse::<f64>().ok()?)))
            .ok_or_else(|| Error::invalid("A current display geometry is required"))?;
        let viewport = if self.target.kind == "simulator" {
            let frame = &fresh["root"]["frame"];
            let w = frame["width"].as_f64().unwrap_or(0.0);
            let h = frame["height"].as_f64().unwrap_or(0.0);
            if frame["x"].as_f64() != Some(0.0)
                || frame["y"].as_f64() != Some(0.0)
                || w <= 0.0
                || h <= 0.0
                || (w / h - width / height).abs() > 0.001
            {
                return Err(Error::new(
                    "GEOMETRY_CHANGED",
                    "The accessibility root does not establish a full-display coordinate mapping",
                ));
            }
            (w, h)
        } else {
            (width, height)
        };
        let frame = &element["frame"];
        let field = |name: &str| {
            frame[name]
                .as_f64()
                .ok_or_else(|| Error::new("ELEMENT_NOT_ACTIONABLE", "Element has no usable frame"))
        };
        let (x, y, w, h) = (field("x")?, field("y")?, field("width")?, field("height")?);
        if w <= 0.0 || h <= 0.0 {
            return Err(Error::new(
                "ELEMENT_NOT_ACTIONABLE",
                "Element frame is empty",
            ));
        }
        let point = crate::ui::Point {
            x: (x + w / 2.0) / viewport.0,
            y: (y + h / 2.0) / viewport.1,
        };
        point.validate()?;
        args.as_object_mut().unwrap().remove("element");
        args.as_object_mut().unwrap().remove("allowPartial");
        args["point"] = json!({"x":point.x,"y":point.y});
        Ok(args)
    }
    pub(super) async fn semantic(&self, name: &str, args: Value) -> Result<Value> {
        only_fields(
            &args,
            &[
                "runId",
                "goalId",
                "sessionId",
                "element",
                "conditions",
                "timeoutMs",
                "allowPartial",
            ],
        )?;
        if self.target.kind == "device" {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Managed semantic reads and waits support simulator and Android only",
            ));
        }
        if name == "ui_read" {
            let target = parse_target(&args["element"])?;
            let snapshot = self.semantic_snapshot(&args).await?;
            let element = self.resolve(&target, &snapshot, &args)?;
            let snapshot_id = super::id();
            let path = self.evidence.join(format!("ui-{snapshot_id}.json"));
            crate::artifacts::write(
                &path,
                &serde_json::to_vec(&snapshot)
                    .map_err(|_| Error::new("INTERNAL", "Cannot encode snapshot"))?,
            )?;
            return Ok(
                json!({"snapshotId":snapshot_id,"element":element,"quality":snapshot["quality"],"coverage":snapshot["coverage"],"truncated":snapshot["truncated"],"frameUnit":if self.target.kind=="android"{"pixels"}else{"points"},"artifactPath":path}),
            );
        }
        let (conditions, ms) = parse_wait(&self.target.kind, &args)?;
        if ms == 0 {
            let (satisfied, evidence) =
                tokio::time::timeout(Duration::from_secs(30), self.conditions(&conditions, &args))
                    .await
                    .map_err(|_| {
                        Error::new(
                            "TIMEOUT",
                            "Single condition observation exceeded 30 seconds",
                        )
                    })??;
            return Ok(
                json!({"state":if satisfied{"satisfied"}else{"unsatisfied"},"evidence":evidence,"attempts":1}),
            );
        }
        crate::app_lifecycle::poll(Duration::from_millis(ms), || {
            self.conditions(&conditions, &args)
        })
        .await
    }

    async fn conditions(&self, conditions: &[Condition], args: &Value) -> Result<(bool, Value)> {
        let snapshot = if conditions
            .iter()
            .any(|c| !matches!(c, Condition::Foreground { .. }))
        {
            Some(self.semantic_snapshot(args).await)
        } else {
            None
        };
        let mut evidence = Vec::new();
        let mut matched = None;
        for (index, condition) in conditions.iter().enumerate() {
            let result = match condition {
                Condition::Foreground { bundle_id } => self.foreground(bundle_id).await,
                _ => match snapshot.as_ref().unwrap() {
                    Ok(snapshot) => condition.evaluate(snapshot),
                    Err(error) => Err(error.clone()),
                },
            };
            let (satisfied, observed) = match result {
                Ok(observed) => observed,
                Err(error) => (false, json!({"error":error})),
            };
            if satisfied && matched.is_none() {
                matched = Some(index);
            }
            evidence.push(json!({"index":index,"satisfied":satisfied,"observed":observed}));
        }
        Ok((
            matched.is_some(),
            json!({"matchedIndex":matched,"conditions":evidence}),
        ))
    }

    async fn foreground(&self, bundle: &str) -> Result<(bool, Value)> {
        let target = self.live_target().await?;
        if target.kind == "android" {
            let focused = crate::android::Android::default()
                .focused_package(&target.platform_id)
                .await?;
            return Ok((
                focused.as_deref() == Some(bundle),
                json!({"focusedPackage":focused,"requestedApp":bundle}),
            ));
        }
        let snapshot = self.accessibility_snapshot(&target, &json!({})).await?;
        let output = crate::simulator::device_set::run(
            &["spawn", &target.platform_id, "launchctl", "list"],
            Duration::from_secs(5),
            1024 * 1024,
        )
        .await?;
        let pid = snapshot["pid"].as_i64();
        let matches = String::from_utf8_lossy(&output.bytes).lines().any(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            fields.len() == 3
                && fields[0].parse::<i64>().ok() == pid
                && pid.is_some()
                && fields[2].starts_with(&format!("UIKitApplication:{bundle}["))
        });
        Ok((
            matches,
            json!({"requestedApp":bundle,"frontmostMatches":matches,"coverage":"frontmost_app"}),
        ))
    }

    pub(super) async fn semantic_snapshot(&self, args: &Value) -> Result<Value> {
        let target = self.live_target().await?;
        if target.kind == "device" {
            return Err(Error::new(
                "CAPABILITY_UNAVAILABLE",
                "Physical semantic resolution is unavailable",
            ));
        }
        let mut snapshot = self.accessibility_snapshot(&target, args).await?;
        if target.kind == "simulator" {
            self.incarnation().await?;
        }
        crate::ui::normalize_roles(&mut snapshot["root"]);
        snapshot["sessionId"] = args["sessionId"].clone();
        snapshot["targetId"] = json!(target.identity.target_id);
        if snapshot["quality"] != "complete" && args["allowPartial"] != true {
            return Err(Error::new(
                "PARTIAL_UI_TREE",
                "Explicit allowPartial is required for the frontmost-app tree",
            ));
        }
        Ok(snapshot)
    }

    pub(super) fn resolve(&self, target: &Target, fresh: &Value, args: &Value) -> Result<Value> {
        match target {
            Target::Query { query } => crate::ui::semantic::unique(query, fresh),
            Target::Reference {
                snapshot_id,
                element_ref,
            } => {
                if uuid::Uuid::parse_str(snapshot_id).is_err() {
                    return Err(Error::invalid("Invalid snapshotId"));
                }
                let path = self.evidence.join(format!("ui-{snapshot_id}.json"));
                let old: Value = crate::managed::storage::read(&path)?.ok_or_else(|| {
                    Error::new("STALE_ELEMENT", "Snapshot is unavailable in this run")
                })?;
                if old["sessionId"] != args["sessionId"] || old["targetId"] != fresh["targetId"] {
                    return Err(Error::new(
                        "STALE_ELEMENT",
                        "Snapshot belongs to another session or target",
                    ));
                }
                crate::ui::semantic::revalidate(&old, fresh, element_ref)
            }
        }
    }
}

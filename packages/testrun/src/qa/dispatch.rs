//! Route public QA operations to their owning application workflows.
use super::Server;
use super::arguments::{Empty, parse};
use crate::error::{Error, Result};
use serde_json::{Value, json};

impl Server {
    pub async fn call(&mut self, name: &str, args: Value) -> Result<Value> {
        if crate::builds::config_handles(name) {
            return crate::builds::config(name, &args);
        }
        if crate::builds::handles(name) || crate::diagnostics::handles(name) {
            return self.managed.call(name, args).await;
        }
        if crate::documentation::handles(name) {
            return crate::documentation::call(name, &args, None).await;
        }
        if name == "cancel" || crate::browser::api::selected(name, &args) {
            return self.managed.call(name, args).await;
        }
        if matches!(name, "browser_list" | "browser_doctor") {
            return crate::browser::runtime::inspect(name, args);
        }
        if crate::browser::surface::handles(name) {
            return self.managed.call(name, args).await;
        }
        if matches!(
            name,
            "control_status" | "control_acquire" | "control_release"
        ) || crate::playback::operation(name)
            || crate::network::operation(name)
            || crate::managed::selected_operation(name) && args.get("sessionId").is_none()
        {
            return self.managed.call(name, args).await;
        }
        if matches!(
            name,
            "connect"
                | "act"
                | "observe"
                | "status"
                | "disconnect"
                | "app_install"
                | "app_uninstall"
        ) {
            return crate::device_setup::managed_call(&mut self.managed, name, args).await;
        }
        match name {
            "doctor" => {
                super::arguments::only_fields(&args, &["targetId"])?;
                let target = super::arguments::field(&args, "targetId")?;
                if target.starts_with("android:") {
                    crate::android::Android::default().doctor(target).await
                } else {
                    crate::device_setup::doctor(target).await
                }
            }
            "host_info" => {
                parse::<Empty>(args)?;
                Ok(
                    json!({"os":std::env::consts::OS,"architecture":std::env::consts::ARCH,"adapterProtocolVersion":1}),
                )
            }
            // Preparation changes the device, so the manager runs it in the
            // device's queue.
            "developer_mode_reveal" | "developer_support_install" => {
                super::arguments::only_fields(&args, &["targetId"])?;
                super::arguments::field(&args, "targetId")?;
                self.managed.call(name, args).await
            }
            "target_list" => self.list_targets(parse(args)?).await,
            "run.begin" => self.begin(parse(args)?),
            "goal.begin" => self.begin_goal(parse(args)?),
            "run.end" => self.end(parse(args)?).await,
            "session_open" => self.open(parse(args)?).await,
            "session_status" => self.status(parse(args)?).await,
            "session_close" => self.close(parse(args)?).await,
            "command_status" => self.command_status(args),
            "batch" => self.batch(args).await,
            "app_launch" | "app_terminate" | "app_restart" | "open_url" | "tap" | "long_press"
            | "swipe" | "key" | "type_text" => self.mutate(name, args).await,
            "app_list" | "clipboard_read" => self.observe(name, args).await,
            "screenshot" => self.screenshot(args).await,
            "ui_snapshot" | "ui_find" => self.accessibility(name, args).await,
            "ui_read" | "wait" => self.read_context(&args)?.semantic(name, args).await,
            "preview_start" => self.stream_start(args).await,
            "preview_stop" => self.stream_stop(args).await,
            "preview_status" => self.stream_status(args),
            "recording_start" => self.recording_start(args).await,
            "recording_stop" | "recording_status" => self.recording_result(name, args).await,
            _ => Err(Error::new("TOOL_NOT_FOUND", "Tool is not registered")),
        }
    }
}

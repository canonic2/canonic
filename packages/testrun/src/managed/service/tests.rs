//! Manager behavior against fixture device actors: one pool and FIFO queue
//! per device for every client, target resolution, release and its
//! automatic retry, idle release, quarantine, upgrades and exit.
use super::*;
use crate::managed::{host, identity::Identity, tests::identity};
#[cfg(unix)]
use std::path::Path;

struct Fixture {
    root: tempfile::TempDir,
    service: Arc<Service>,
}

#[cfg(unix)]
#[tokio::test]
async fn project_workflow_routes_retained_tasks_logs_and_replay_without_device_claims() {
    let fixture = Fixture::new();
    let agent = fixture.agent("project-agent");
    let project = fixture.root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let configuration = json!({"version":1,"builds":{"ios-dev":{"builder":"custom","platform":"ios","target_kind":"device","custom":{"command":["/bin/false"]},"artifact":{"path":"dist/Acme.app"},"development":{"app_id":"com.example.acme","prepare":{"command":["/usr/bin/printf","prepared"]}}}}});
    std::fs::write(project.join("testrun.yml"), configuration.to_string()).unwrap();
    let args = json!({"project":project,"profile":"ios-dev","task":"prepare","submissionId":"project-prepare-1"});
    let started = fixture
        .mcp(&agent, "project_start", args.clone())
        .await
        .unwrap();
    let id = started["operationId"].as_str().unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let status = fixture
                .mcp(&agent, "build_status", json!({"operationId":id}))
                .await
                .unwrap();
            if status["settled"] == true {
                assert_eq!(status["state"], "completed", "{status}");
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let log = fixture
        .mcp(&agent, "build_log", json!({"operationId":id}))
        .await
        .unwrap();
    assert_eq!(log["text"], "prepared");
    let status = fixture
        .mcp(
            &agent,
            "project_status",
            json!({"project":project,"profile":"ios-dev"}),
        )
        .await
        .unwrap();
    assert_eq!(status["checks"]["installedApp"]["state"], "not_checked");
    assert_eq!(status["recentOperations"][0]["operationId"], id);
    std::fs::write(project.join("testrun.yml"), "invalid configuration").unwrap();
    assert_eq!(
        fixture.mcp(&agent, "project_start", args).await.unwrap()["operationId"],
        id
    );
    assert!(
        fixture
            .service
            .registry()
            .pool()
            .is_none_or(|pool| pool.targets.is_empty())
    );
}

impl Fixture {
    fn new() -> Self {
        Self::with(PROTOCOL)
    }
    fn with(protocol: u32) -> Self {
        let root = crate::managed::tests::root();
        let service = Service::fixture(
            root.path().join("state"),
            protocol,
            Duration::from_millis(300),
        );
        Self { root, service }
    }
    #[cfg(unix)]
    fn idle(idle: Duration) -> Self {
        let root = crate::managed::tests::root();
        let service = Service::fixture_idle(root.path().join("state"), idle);
        Self { root, service }
    }
    /// An agent's identity: its host label, in a workspace of its own.
    fn agent(&self, label: &str) -> Identity {
        identity(label, &self.root.path().join(label))
    }
    #[cfg(unix)]
    fn directory(&self) -> PathBuf {
        super::super::selection::pool_directory(&self.service.root, super::super::identity::POOL)
            .unwrap()
    }
    #[cfg(unix)]
    fn target_directory(&self, target: &str) -> PathBuf {
        super::super::selection::target_directory(&self.directory(), target).unwrap()
    }
    async fn call(
        &self,
        identity: &Identity,
        kind: &str,
        name: &str,
        args: Value,
    ) -> Result<Value> {
        self.request(
            identity,
            kind,
            name,
            args,
            &uuid::Uuid::new_v4().to_string(),
        )
        .await
    }
    async fn request(
        &self,
        identity: &Identity,
        kind: &str,
        name: &str,
        args: Value,
        request_id: &str,
    ) -> Result<Value> {
        self.service
            .request(message(identity, kind, name, &args, Some(request_id)))
            .await
    }
    #[cfg(unix)]
    async fn cli(&self, identity: &Identity, name: &str, args: Value) -> Result<Value> {
        self.call(identity, "cli", name, args).await
    }
    async fn mcp(&self, identity: &Identity, name: &str, args: Value) -> Result<Value> {
        self.call(identity, "mcp", name, args).await
    }
    /// A request running in the background, as a concurrent client sends it.
    #[cfg(unix)]
    fn spawn(
        &self,
        identity: &Identity,
        kind: &str,
        name: &str,
        args: Value,
        request_id: &str,
    ) -> tokio::task::JoinHandle<Result<Value>> {
        let service = self.service.clone();
        let message = message(identity, kind, name, &args, Some(request_id));
        tokio::spawn(async move { service.request(message).await })
    }
    /// A control extension, as the host routes it from the control lane.
    #[cfg(unix)]
    async fn control(&self, kind: &str, fields: Value) -> Result<Value> {
        if kind == "task_end" {
            return Ok(self.service.task_end(&fields));
        }
        self.service.control(kind, &fields, None).await
    }
    fn pooled(&self, target: &str) -> bool {
        self.service
            .registry()
            .pool()
            .is_some_and(|pool| pool.holds(target))
    }
    async fn until(&self, condition: impl Fn(&Self) -> bool) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while !condition(self) {
            assert!(
                tokio::time::Instant::now() < deadline,
                "condition not reached"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
    /// The owner directory of `target`'s device actor.
    #[cfg(unix)]
    fn owner_directory(&self, target: &str) -> PathBuf {
        let selected: Value = storage::read(&self.target_directory(target).join("selected.json"))
            .unwrap()
            .unwrap();
        PathBuf::from(selected["directory"].as_str().unwrap())
    }
}

fn message(
    identity: &Identity,
    kind: &str,
    name: &str,
    args: &Value,
    request_id: Option<&str>,
) -> Value {
    json!({
        "protocol":PROTOCOL,"control":CONTROL,"kind":"call","requestId":request_id,
        "identity":identity,"client":{"kind":kind,"connection":(kind == "mcp").then_some("acme-mcp")},
        "name":name,"args":args,
    })
}

fn target() -> String {
    format!("android:Acme-{}", uuid::Uuid::new_v4().simple())
}
#[cfg(unix)]
fn tap() -> Value {
    json!({"actions":[{"action":"tap","point":{"x":0.5,"y":0.5}}]})
}
#[cfg(unix)]
fn on(device: &str, mut args: Value) -> Value {
    args["targetId"] = json!(device);
    args
}
#[cfg(unix)]
fn dispatches(directory: &Path) -> Vec<String> {
    std::fs::read_to_string(directory.join("dispatches.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}
#[cfg(unix)]
fn lease_held(device: &str) -> bool {
    crate::lease::ControlLease::inspect(&format!("managed:{device}"))
        .unwrap()
        .is_some_and(|(held, _)| held)
}

#[cfg(unix)]
#[tokio::test]
async fn mcp_disconnect_keeps_its_execution_budget_after_waiting_for_the_queue() {
    let fixture = Fixture::new();
    let agent = fixture.agent("acme-disconnect");
    let device = target();
    fixture
        .mcp(&agent, "connect", on(&device, json!({})))
        .await
        .unwrap();
    let queue = fixture.service.queue(Some(&device));
    let predecessor = queue.clone().turn(None).await.unwrap();
    let disconnect = fixture.spawn(
        &agent,
        "mcp",
        "disconnect",
        on(&device, json!({})),
        "acme-mcp-disconnect",
    );
    fixture.until(|_| queue.waiting() == 1).await;

    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(46)).await;
    tokio::time::resume();
    assert_eq!(queue.waiting(), 1);
    assert!(!disconnect.is_finished());
    assert!(fixture.pooled(&device));

    drop(predecessor);
    let released = tokio::time::timeout(Duration::from_secs(10), disconnect)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(released["closed"], true);
    assert!(!fixture.pooled(&device));
}

#[cfg(unix)]
#[tokio::test]
async fn agents_in_different_workspaces_share_one_device_with_fifo_input() {
    let fixture = Fixture::new();
    let (first, second, third) = (
        fixture.agent("acme-first"),
        fixture.agent("acme-second"),
        fixture.agent("acme-third"),
    );
    let device = target();
    // Every agent connects the same device; none is refused for another's use.
    for agent in [&first, &second, &third] {
        let connected = fixture
            .mcp(agent, "connect", json!({"targetId":device}))
            .await
            .unwrap();
        assert_eq!(connected["state"], "ready", "{connected}");
    }
    let directory = fixture.directory();
    storage::write(&directory.join("hold-batch"), &true).unwrap();
    let queue = fixture.service.queue(Some(&device));
    let first_act = fixture.spawn(&first, "mcp", "act", tap(), "acme-1");
    fixture
        .until(|_| dispatches(&directory) == ["acme-1"])
        .await;
    let second_act = fixture.spawn(&second, "cli", "act", tap(), "acme-2");
    fixture.until(|_| queue.waiting() == 1).await;
    let third_act = fixture.spawn(&third, "mcp", "act", tap(), "acme-3");
    fixture.until(|_| queue.waiting() == 2).await;
    // Reads from any agent run while input is queued.
    let observed = tokio::time::timeout(
        Duration::from_secs(5),
        fixture.cli(&second, "observe", json!({})),
    )
    .await
    .expect("A read must not wait behind queued input")
    .unwrap();
    assert_eq!(observed["state"], "ready", "{observed}");
    std::fs::remove_file(directory.join("hold-batch")).unwrap();
    for act in [first_act, second_act, third_act] {
        assert_eq!(act.await.unwrap().unwrap()["state"], "ready");
    }
    assert_eq!(dispatches(&directory), ["acme-1", "acme-2", "acme-3"]);
    fixture.cli(&third, "disconnect", json!({})).await.unwrap();
    assert!(!fixture.pooled(&device));
}

#[cfg(unix)]
#[tokio::test]
async fn a_failed_connect_is_not_the_implied_target_of_later_input() {
    let fixture = Fixture::new();
    let acme = fixture.agent("acme-failed-connect");
    storage::write(&fixture.directory().join("fail-startup"), &true).unwrap();
    let failed = fixture
        .cli(&acme, "connect", json!({"targetId":target()}))
        .await
        .unwrap_err();
    assert_eq!(failed.code, "DEVICE_LOCKED", "{failed:?}");
    let refused = fixture.cli(&acme, "act", tap()).await.unwrap_err();
    assert_eq!(refused.code, "NOT_CONNECTED", "{refused:?}");
    assert!(
        !fixture.directory().join("dispatches.log").exists(),
        "No input may reach a device whose connect failed"
    );
}

#[tokio::test]
async fn pruning_keeps_the_queue_of_a_running_request() {
    let fixture = Fixture::new();
    let device = target();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let first = fixture.service.queue(Some(&device));
    // A running request keeps only its turn, as the managed client does.
    let running = first.clone().turn(Some(deadline)).await.unwrap();
    drop(first);
    fixture.service.prune();
    let next = fixture.service.queue(Some(&device));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), next.clone().turn(None))
            .await
            .is_err(),
        "The next request must wait for the running one, not get a new queue"
    );
    drop(running);
    assert_eq!(next.waiting(), 0);
    assert!(next.turn(Some(deadline)).await.is_some());
    fixture.service.prune();
    assert_eq!(fixture.service.tracked().0, 0);
}

#[cfg(unix)]
#[tokio::test]
async fn android_observe_connects_only_the_named_target() {
    let fixture = Fixture::new();
    let (agent, other) = (fixture.agent("acme-agent"), fixture.agent("acme-other"));
    let device = target();
    for (name, args) in [
        ("observe", json!({})),
        ("observe", json!({"targetId":"android:avd:Acme-QA"})),
        (
            "observe",
            json!({"targetId":"device:Acme-iPhone","profile":"evidence"}),
        ),
        (
            "ui_snapshot",
            json!({"targetId":"device:Acme-iPhone","scope":{"kind":"system"}}),
        ),
        (
            "ui_read",
            on(
                &device,
                json!({"element":{"snapshotId":"s","elementId":"e"}}),
            ),
        ),
        (
            "ui_snapshot",
            on(
                &device,
                json!({"scope":{"kind":"display"},"cursor":"earlier-page"}),
            ),
        ),
        (
            "volume_get",
            on(
                &device,
                json!({"destination":{"kind":"media_session","id":"earlier-session"}}),
            ),
        ),
        ("status", on(&device, json!({}))),
        ("act", on(&device, tap())),
        (
            "control_acquire",
            on(&device, json!({"owner":"agent","expectedVersion":1})),
        ),
    ] {
        let refused = fixture.cli(&agent, name, args).await.unwrap_err();
        assert_eq!(refused.code, "NOT_CONNECTED", "{name}");
        let details = refused.details.unwrap();
        assert_eq!(details["nextAction"], "connect");
        assert_eq!(details["admitted"], false);
    }
    assert!(fixture.service.targets().is_empty());
    assert_eq!(fixture.service.supervisor.alive(), 0);
    let observed = fixture
        .cli(
            &other,
            "observe",
            on(&device, json!({"profile":"evidence"})),
        )
        .await
        .unwrap();
    assert_eq!(observed["source"], "fixture", "{observed}");
    assert_eq!(observed["profile"], "evidence", "{observed}");
    assert_eq!(fixture.service.targets(), std::slice::from_ref(&device));
    assert!(lease_held(&device));
    // Connected by another agent, it serves this one too, in its raw shape.
    let observed = fixture
        .cli(&agent, "observe", on(&device, json!({})))
        .await
        .unwrap();
    assert_eq!(observed["source"], "fixture", "{observed}");
    let acted = fixture
        .cli(&agent, "act", on(&device, tap()))
        .await
        .unwrap();
    assert_eq!(acted["status"], "completed", "{acted}");
    fixture
        .cli(&agent, "disconnect", json!({"targetId":device}))
        .await
        .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn fresh_reads_connect_supported_mobile_targets() {
    let fixture = Fixture::new();
    let agent = fixture.agent("acme-mobile-reads");
    let phone = "device:Acme-iPhone";
    let simulator = "simulator:Acme-Simulator";
    let android = target();
    for (device, name, args) in [
        (phone, "observe", json!({"profile":"fast"})),
        (phone, "app_list", json!({})),
        (simulator, "observe", json!({"profile":"evidence"})),
        (simulator, "clipboard_read", json!({})),
        (
            simulator,
            "ui_snapshot",
            json!({"scope":{"kind":"display"},"allowPartial":true}),
        ),
        (
            &android,
            "ui_find",
            json!({"scope":{"kind":"display"},"query":{"role":"button"}}),
        ),
        (
            &android,
            "ui_read",
            json!({"element":{"query":{"role":"button"}}}),
        ),
        (
            &android,
            "wait",
            json!({"conditions":[{"kind":"foreground","appId":"com.example.acme"}],"timeoutMs":0}),
        ),
        (&android, "media_status", json!({})),
        (
            &android,
            "volume_get",
            json!({"destination":{"kind":"music_stream"}}),
        ),
    ] {
        assert!(!fixture.pooled(device));
        let result = fixture.cli(&agent, name, on(device, args)).await.unwrap();
        if name == "observe" {
            assert_eq!(result["source"], "fixture", "{result}");
        } else {
            assert_eq!(result["tool"], name, "{result}");
        }
        assert_eq!(fixture.service.targets(), [device]);
        let owner: Value = storage::read(&fixture.owner_directory(device).join("owner.json"))
            .unwrap()
            .unwrap();
        assert_eq!(owner["boot"], false);
        assert_eq!(owner["setup"], false);
        if device == phone {
            assert_eq!(owner["controlPath"], "hid");
        }
        fixture
            .cli(&agent, "disconnect", on(device, json!({})))
            .await
            .unwrap();
        assert!(!lease_held(device));
    }
    assert!(dispatches(&fixture.directory()).is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn android_observe_uses_the_connect_capture_and_reports_connection_failure() {
    let fixture = Fixture::new();
    let agent = fixture.agent("acme-android-observe");
    let device = target();
    let args = on(&device, json!({}));
    let fail_start = fixture.directory().join("fail-startup");
    storage::write(&fail_start, &true).unwrap();
    let error = fixture
        .cli(&agent, "observe", args.clone())
        .await
        .unwrap_err();
    assert_eq!(error.code, "DEVICE_LOCKED", "{error:?}");
    assert!(!fixture.pooled(&device));
    assert!(!lease_held(&device));

    std::fs::remove_file(fail_start).unwrap();
    let observed = fixture.cli(&agent, "observe", args.clone()).await.unwrap();
    assert_eq!(observed["source"], "fixture", "{observed}");
    assert!(observed["profile"].is_null(), "{observed}");
    fixture.cli(&agent, "disconnect", args).await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn queued_automatic_read_keeps_the_explicit_xctest_connection() {
    let fixture = Fixture::new();
    let agent = fixture.agent("acme-queued-connect");
    let device = "device:Acme-iPhone";
    let queue = fixture.service.queue(Some(device));
    let predecessor = queue.clone().turn(None).await.unwrap();
    let connect = fixture.spawn(
        &agent,
        "mcp",
        "connect",
        on(
            device,
            json!({"controlPath":"xctest","appId":"com.example.acme"}),
        ),
        "explicit-xctest",
    );
    fixture.until(|_| queue.waiting() == 1).await;
    let read = fixture.spawn(
        &agent,
        "cli",
        "observe",
        on(device, json!({"profile":"fast"})),
        "automatic-read",
    );
    fixture.until(|_| queue.waiting() == 2).await;
    drop(predecessor);

    assert_eq!(connect.await.unwrap().unwrap()["state"], "ready");
    assert_eq!(read.await.unwrap().unwrap()["source"], "fixture");
    let owner: Value = storage::read(&fixture.owner_directory(device).join("owner.json"))
        .unwrap()
        .unwrap();
    assert_eq!(owner["controlPath"], "xctest");
    assert_eq!(owner["bundleId"], "com.example.acme");
    fixture
        .cli(&agent, "disconnect", on(device, json!({})))
        .await
        .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn invalid_wait_cannot_open_a_device_connection() {
    let fixture = Fixture::new();
    let agent = fixture.agent("acme-invalid-wait");
    let device = target();
    let error = fixture
        .cli(
            &agent,
            "wait",
            on(&device, json!({"conditions":[{"kind":"foreground"}]})),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "INVALID_ARGUMENT");
    assert!(!fixture.pooled(&device));
    assert_eq!(fixture.service.supervisor.alive(), 0);
    assert!(
        !fixture
            .target_directory(&device)
            .join("selected.json")
            .exists()
    );
}

#[cfg(unix)]
#[tokio::test]
async fn slow_auto_connect_keeps_the_wait_and_media_read_budgets() {
    let fixture = Fixture::new();
    let agent = fixture.agent("acme-slow-start");
    for (name, args) in [
        (
            "wait",
            json!({"timeoutMs":30000,"conditions":[{"kind":"foreground","appId":"com.example.acme"}]}),
        ),
        ("media_status", json!({})),
    ] {
        let device = target();
        let hold = fixture.directory().join("hold-startup");
        storage::write(&hold, &true).unwrap();
        let call = fixture.spawn(&agent, "cli", name, on(&device, args), name);
        let selected = fixture.target_directory(&device).join("selected.json");
        fixture.until(|_| selected.exists()).await;
        // Pass both read budgets while the connection is still starting.
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(36)).await;
        std::fs::remove_file(hold).unwrap();
        tokio::time::resume();

        let result = tokio::time::timeout(Duration::from_secs(5), call)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(result["tool"], name);
        fixture
            .cli(&agent, "disconnect", on(&device, json!({})))
            .await
            .unwrap();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn calls_without_a_target_use_the_only_connected_one_and_name_one_among_several() {
    let fixture = Fixture::new();
    let acme = fixture.agent("acme-targets");
    let (first, second) = (target(), target());
    fixture
        .mcp(&acme, "connect", json!({"targetId":first}))
        .await
        .unwrap();
    assert_eq!(
        fixture.mcp(&acme, "act", tap()).await.unwrap()["state"],
        "ready"
    );
    // A connect that fails without starting the device leaves nothing pooled,
    // so calls without targetId still reach the one connected device.
    let busy = target();
    {
        let _external = crate::lease::ControlLease::acquire(&format!("managed:{busy}")).unwrap();
        let refused = fixture
            .mcp(&acme, "connect", json!({"targetId":busy}))
            .await
            .unwrap_err();
        assert_eq!(refused.code, "DEVICE_IN_USE");
    }
    assert!(!fixture.pooled(&busy));
    assert_eq!(
        fixture.mcp(&acme, "act", tap()).await.unwrap()["state"],
        "ready"
    );
    fixture
        .mcp(&acme, "connect", json!({"targetId":second}))
        .await
        .unwrap();
    assert_eq!(fixture.service.devices().len(), 2);
    let refused = fixture.mcp(&acme, "act", tap()).await.unwrap_err();
    assert_eq!(refused.code, "TARGET_REQUIRED");
    let details = refused.details.unwrap();
    assert_eq!(details["admitted"], false);
    assert_eq!(details["targets"].as_array().unwrap().len(), 2);
    // Named calls reach their own device and records.
    fixture.mcp(&acme, "act", on(&second, tap())).await.unwrap();
    assert_eq!(dispatches(&fixture.target_directory(&first)).len(), 2);
    assert_eq!(dispatches(&fixture.target_directory(&second)).len(), 1);
    let status = fixture.mcp(&acme, "status", json!({})).await.unwrap();
    assert_eq!(status["targets"].as_array().unwrap().len(), 2, "{status}");
    let status = fixture
        .mcp(&acme, "status", json!({"targetId":first}))
        .await
        .unwrap();
    assert_eq!(status["targetId"], first, "{status}");
    let unknown = fixture
        .mcp(&acme, "status", json!({"targetId":target()}))
        .await
        .unwrap_err();
    assert_eq!(unknown.code, "NOT_CONNECTED");
    // A disconnect of a target that is not connected has nothing to release.
    let nothing = fixture
        .mcp(&acme, "disconnect", json!({"targetId":target()}))
        .await
        .unwrap();
    assert_eq!(nothing["closed"], true, "{nothing}");
    // Disconnect without a target needs one among several.
    let refused = fixture
        .mcp(&acme, "disconnect", json!({}))
        .await
        .unwrap_err();
    assert_eq!(refused.code, "TARGET_REQUIRED");
    let closed = fixture
        .mcp(&acme, "disconnect", json!({"targetId":first}))
        .await
        .unwrap();
    assert_eq!(closed["closed"], true, "{closed}");
    assert!(!fixture.pooled(&first));
    assert!(!lease_held(&first));
    assert_eq!(
        fixture.mcp(&acme, "act", tap()).await.unwrap()["state"],
        "ready"
    );
    fixture.mcp(&acme, "disconnect", json!({})).await.unwrap();
    let none = fixture.mcp(&acme, "act", tap()).await.unwrap_err();
    assert_eq!(none.code, "NOT_CONNECTED", "{none:?}");
    assert!(fixture.service.registry().is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn browsers_are_targets_beside_devices_and_close_one_at_a_time() {
    let fixture = Fixture::new();
    let acme = fixture.agent("acme-browsers");
    let device = target();
    fixture
        .mcp(&acme, "connect", json!({"targetId":device}))
        .await
        .unwrap();
    let pool = fixture.service.registry().pool().unwrap().id.clone();
    let browsers: Vec<String> = (0..2)
        .map(|_| {
            let connection = fixture.service.browsers.fixture_preparing(&pool);
            let target = crate::browser::runtime::target(connection);
            fixture.service.claim(&target).unwrap();
            target
        })
        .collect();
    // Browsers never make a device call ambiguous.
    assert_eq!(
        fixture.mcp(&acme, "act", tap()).await.unwrap()["state"],
        "ready"
    );
    let all = fixture
        .mcp(&acme, "browser_status", json!({}))
        .await
        .unwrap();
    assert_eq!(all["browsers"].as_array().unwrap().len(), 2, "{all}");
    let refused = fixture
        .mcp(&acme, "browser_disconnect", json!({}))
        .await
        .unwrap_err();
    assert_eq!(refused.code, "TARGET_REQUIRED");
    let closed = fixture
        .mcp(&acme, "browser_disconnect", json!({"targetId":browsers[0]}))
        .await
        .unwrap();
    assert_eq!(closed["targetId"], browsers[0], "{closed}");
    assert!(!fixture.pooled(&browsers[0]));
    let (_, directory) = fixture.service.browser_identity().unwrap();
    let path = directory.join("browser status.json");
    let record: Value = super::super::storage::read(&path).unwrap().unwrap();
    assert_eq!(
        record["browsers"].as_array().unwrap().len(),
        1,
        "A successful close must retire the durable browser entry: {record}"
    );
    assert_eq!(record["browsers"][0]["targetId"], browsers[1]);
    // Plain disconnect naming a browser target closes that browser too.
    fixture
        .mcp(&acme, "disconnect", json!({"targetId":browsers[1]}))
        .await
        .unwrap();
    assert!(fixture.service.browsers.connections(&pool).is_empty());
    let record: Value = super::super::storage::read(&path).unwrap().unwrap();
    assert_eq!(
        record["browsers"],
        json!([]),
        "The final close must persist an empty browser list"
    );
    fixture
        .mcp(&acme, "browser_disconnect", json!({}))
        .await
        .unwrap();
    assert_eq!(fixture.service.targets(), std::slice::from_ref(&device));
    fixture.mcp(&acme, "disconnect", json!({})).await.unwrap();
    assert!(fixture.service.registry().is_empty());
}

async fn assert_compact_browser_status(
    fixture: &Fixture,
    agent: &Identity,
    target: &str,
    [pending, next_pending, latest]: [uuid::Uuid; 3],
    total: usize,
) {
    let compact_args = json!({"targetId":target,"limit":1});
    let (shared_status, shared_args) =
        crate::browser::api::translate("status", compact_args.clone()).unwrap();
    for (name, args) in [
        ("browser_status", compact_args),
        (&shared_status, shared_args),
    ] {
        let compact = fixture.mcp(agent, name, args).await.unwrap();
        assert_eq!(compact["testrunVersion"], env!("CARGO_PKG_VERSION"));
        assert_eq!(compact["browser"]["phase"], "ready");
        assert_eq!(
            compact["browser"]["operations"].as_array().unwrap().len(),
            1
        );
        assert_eq!(compact["browser"]["operationsTotal"], total);
        assert_eq!(compact["browser"]["operations"][0]["id"], json!(pending));
        assert_eq!(compact["browser"]["detailsOmitted"], true);
        assert!(compact["browser"].get("resources").is_none());
    }
    let default_status = fixture
        .mcp(agent, "browser_status", json!({"targetId":target}))
        .await
        .unwrap();
    let default_browser = &default_status["browser"];
    assert_eq!(default_browser["detailsOmitted"], true);
    assert_eq!(default_browser["operations"].as_array().unwrap().len(), 3);
    assert_eq!(default_browser["operations"][0]["id"], json!(pending));
    assert_eq!(default_browser["operations"][1]["id"], json!(next_pending));
    assert_eq!(default_browser["operations"][2]["id"], json!(latest));
    assert!(default_status.to_string().len() < 2048);
    let listed = fixture
        .mcp(agent, "browser_status", json!({"limit":1}))
        .await
        .unwrap();
    assert_eq!(listed["browsers"].as_array().unwrap().len(), 1);
    assert_eq!(
        listed["browsers"][0]["browser"]["operations"],
        default_browser["operations"]
    );
    let error = fixture
        .mcp(
            agent,
            "browser_status",
            json!({"targetId":target,"limit":17}),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "invalid_arguments");
    let waited = fixture
        .mcp(
            agent,
            "browser_status",
            json!({"targetId":target,"waitMs":1}),
        )
        .await
        .unwrap();
    assert_eq!(waited["waitedOperationId"], json!(pending));
    assert_eq!(waited["waitExpired"], true);
}

#[tokio::test]
async fn browser_status_returns_one_result_even_when_history_exceeds_the_reply_limit() {
    use crate::browser::{
        contract::{Engine, LaunchOptions, Mode},
        lifecycle::{Outcome, Phase, State},
    };
    use canonic_process_kit::frame::encode_json;
    use uuid::Uuid;

    let fixture = Fixture::new();
    let agent = fixture.agent("acme-browser-status");
    let pool = fixture
        .service
        .open_then(|registry| Ok(registry.pool().unwrap().id.clone()))
        .unwrap();
    let mut state = State::new(
        Uuid::parse_str(&pool).unwrap(),
        Uuid::new_v4(),
        &LaunchOptions {
            engine: Engine::Chrome,
            mode: Mode::Headless,
            context: Default::default(),
            media_fixtures: Default::default(),
            proxy: None,
        },
    );
    state.phase = Phase::Ready;
    let budget = super::super::wire::MAX_REPLY_FRAME;
    let events: Vec<_> = (1..=100)
        .map(|sequence| json!({"kind":"console","sequence":sequence,"data":{"text":"x".repeat(2048)}}))
        .collect();
    let full_page = json!({"events":events});
    let latest_page =
        json!({"events":[{"kind":"request_failed","sequence":101}],"nextCursor":null});
    let latest = Uuid::new_v4();
    let full_pages = budget / serde_json::to_vec(&full_page).unwrap().len() + 1;
    for (id, result) in (0..full_pages)
        .map(|_| (Uuid::new_v4(), &full_page))
        .chain(std::iter::once((latest, &latest_page)))
    {
        state.begin(id, "diagnostics_read".into()).unwrap();
        let operation = state.operations.get_mut(&id).unwrap();
        operation.settled = true;
        operation.outcome = Outcome::Succeeded;
        operation.result = Some(result.clone());
    }
    let pending = Uuid::from_u128(u128::MAX);
    let next_pending = Uuid::from_u128(1);
    state.begin(pending, "page_wait".into()).unwrap();
    state.begin(next_pending, "page_wait".into()).unwrap();
    assert!(matches!(
        encode_json(&state.operations, budget),
        Err(canonic_process_kit::Error::FrameTooLarge)
    ));

    let artifacts = fixture.root.path().join("browser");
    crate::artifacts::directory(&artifacts).unwrap();
    let connection = fixture
        .service
        .browsers
        .fixture_diagnostic(&pool, state, &artifacts);
    let target = crate::browser::runtime::target(connection);
    fixture.service.claim(&target).unwrap();
    assert_compact_browser_status(
        &fixture,
        &agent,
        &target,
        [pending, next_pending, latest],
        full_pages + 3,
    )
    .await;
    assert_browser_status_replies(
        &fixture,
        &agent,
        &target,
        &pool,
        latest,
        latest_page,
        full_pages + 3,
    )
    .await;
}

async fn assert_browser_status_replies(
    fixture: &Fixture,
    agent: &Identity,
    target: &str,
    pool: &str,
    latest: uuid::Uuid,
    latest_page: Value,
    total: usize,
) {
    use canonic_process_kit::{
        frame::encode_json,
        protocol::{CONTROL_VERSION, Reply},
    };
    let budget = super::super::wire::MAX_REPLY_FRAME;
    let (poll, poll_args) =
        crate::browser::api::translate("status", json!({"targetId":target,"actionId":latest}))
            .unwrap();
    for (name, args, expected_result) in [
        (
            "browser_status",
            json!({"targetId":target,"includeDetails":true}),
            Value::Null,
        ),
        (poll.as_str(), poll_args, latest_page),
    ] {
        let data = fixture.mcp(agent, name, args).await.unwrap();
        assert_eq!(data["targetId"], target);
        if expected_result.is_null() {
            let summaries = data["browser"]["operations"].as_array().unwrap();
            assert_eq!(summaries.len(), total);
            assert!(summaries.iter().all(|summary| summary["result"].is_null()));
            assert!(summaries.iter().any(|summary| {
                summary["id"] == json!(latest)
                    && summary["outcome"] == "succeeded"
                    && summary["settled"] == true
            }));
        } else {
            assert!(data.get("browser").is_none());
            assert_eq!(data["operation"]["id"], json!(latest));
            assert!(data.to_string().len() < 1024);
        }
        assert_eq!(data["operation"]["result"], expected_result);
        encode_json(
            &Reply {
                control_version: CONTROL_VERSION,
                manager_id: pool.to_owned(),
                request_id: uuid::Uuid::new_v4().to_string(),
                accepted: None,
                code: None,
                data,
            },
            budget,
        )
        .unwrap();
    }
}

#[tokio::test]
async fn abandoned_disconnect_retires_a_lost_browser_and_its_fixture() {
    use crate::browser::{cdp::session::lost_with_fixture, runtime};

    let fixture = Fixture::new();
    let agent = fixture.agent("acme-lost-browser");
    let pool = fixture
        .service
        .open_then(|registry| Ok(registry.pool().unwrap().id.clone()))
        .unwrap();
    let (_, directory) = fixture.service.browser_identity().unwrap();
    let (connection, session, admitted, cleanup) = lost_with_fixture(false).await;
    let target = runtime::target(connection);
    fixture.service.claim(&target).unwrap();
    fixture
        .service
        .browsers
        .fixture_chrome(&pool, connection, session);
    let status = fixture
        .mcp(
            &agent,
            "browser_status",
            json!({"targetId":target,"includeDetails":true}),
        )
        .await
        .unwrap();
    assert_eq!(status["browser"]["phase"], "lost");
    assert_eq!(status["nextAction"], "browser_disconnect");
    assert_eq!(
        status["browser"]["transfers"]["fixtures"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    fixture.service.record_browsers(&pool, &directory).unwrap();

    let disconnect = message(
        &agent,
        "mcp",
        "browser_disconnect",
        &json!({"targetId":target}),
        Some("lost-browser-close"),
    );
    let mut call = Box::pin(fixture.service.offload(disconnect.clone(), None));
    let proceed = tokio::select! {
        result = &mut call => panic!("Disconnect must still be in progress: {result:?}"),
        admitted = tokio::time::timeout(Duration::from_secs(5), admitted) => admitted.unwrap().unwrap(),
    };
    drop(call);
    let abandoned =
        tokio::time::timeout(Duration::from_secs(5), fixture.service.request(disconnect))
            .await
            .unwrap()
            .unwrap_err();
    assert_eq!(abandoned.code, "OUTCOME_UNKNOWN");
    proceed.send(()).unwrap();
    cleanup.await.unwrap();

    fixture
        .until(|fixture| {
            !fixture.pooled(&target)
                && storage::read::<Value>(&directory.join("browser status.json"))
                    .unwrap()
                    .is_some_and(|record| record["browsers"] == json!([]))
        })
        .await;
    let status = fixture
        .mcp(&agent, "browser_status", json!({}))
        .await
        .unwrap();
    assert_eq!(status["browsers"], json!([]));
}

#[tokio::test]
async fn failed_browser_close_retries_automatically_and_shares_the_receipt() {
    use crate::browser::{cdp::session::lost_with_fixture, runtime};

    let fixture = Fixture::new();
    let agent = fixture.agent("acme-browser-retry");
    let pool = fixture
        .service
        .open_then(|registry| Ok(registry.pool().unwrap().id.clone()))
        .unwrap();
    let (connection, session, admitted, cleanup) = lost_with_fixture(true).await;
    let target = runtime::target(connection);
    fixture.service.claim(&target).unwrap();
    fixture
        .service
        .browsers
        .fixture_chrome(&pool, connection, session);
    let args = json!({"targetId":target});
    let failure = fixture
        .mcp(&agent, "browser_disconnect", args.clone())
        .await
        .unwrap_err();
    assert_eq!(failure.code, "CLEANUP_PENDING");
    assert!(failure.retryable);
    assert_eq!(
        fixture.service.pending_release(&target).unwrap().attempts,
        1
    );
    let status = fixture
        .mcp(&agent, "browser_status", args.clone())
        .await
        .unwrap();
    assert_eq!(status["browser"]["phase"], "cleanup_pending");
    assert_eq!(status["closing"], true);
    assert!(fixture.pooled(&target));

    let service = fixture.service.clone();
    let lifetime = tokio::spawn(async move { service.keep_alive().await });
    let proceed = tokio::time::timeout(Duration::from_secs(5), admitted)
        .await
        .unwrap()
        .unwrap();
    assert!(fixture.service.pending_release(&target).unwrap().running);
    let request = message(
        &agent,
        "mcp",
        "browser_disconnect",
        &args,
        Some("join-browser-retry"),
    );
    let mut joined = Box::pin(fixture.service.dispatch(request));
    assert!(futures_util::poll!(joined.as_mut()).is_pending());
    proceed.send(()).unwrap();
    let closed = joined.await.unwrap();
    assert_eq!(closed["closed"], true);
    assert_eq!(closed["cleanup"]["journalFailure"], "STATE_IO");
    cleanup.await.unwrap();
    assert!(!fixture.pooled(&target));
    assert!(fixture.service.browsers.connections(&pool).is_empty());
    lifetime.abort();
    let _ = lifetime.await;
}

#[tokio::test]
async fn draining_manager_releases_browser_through_target_owner() {
    let fixture = Fixture::new();
    let agent = fixture.agent("acme-browser-drain");
    fixture
        .mcp(&agent, "connect", json!({"targetId":target()}))
        .await
        .unwrap();
    let id = fixture.service.registry().pool().unwrap().id.clone();
    let connection = fixture.service.browsers.fixture_preparing(&id);
    let browser = crate::browser::runtime::target(connection);
    fixture.service.claim(&browser).unwrap();
    let (_, directory) = fixture.service.browser_identity().unwrap();
    let service = fixture.service.clone();
    let lifetime = tokio::spawn(async move { service.keep_alive().await });
    fixture.service.drain();
    fixture.until(|fixture| !fixture.pooled(&browser)).await;
    assert!(fixture.service.browsers.connections(&id).is_empty());
    let record: Option<Value> = storage::read(&directory.join("browser status.json")).unwrap();
    assert!(record.is_none_or(|record| record["browsers"] == json!([])));
    lifetime.abort();
    let _ = lifetime.await;
}

#[tokio::test]
async fn driver_lifecycle_evidence_record_failure_preserves_verified_close() {
    let fixture = Fixture::new();
    let acme = fixture.agent("acme-browser-close-record");
    let device = target();
    fixture
        .mcp(&acme, "connect", json!({"targetId":device}))
        .await
        .unwrap();
    let pool = fixture.service.registry().pool().unwrap().id.clone();
    let connection = fixture.service.browsers.fixture_preparing(&pool);
    let target = crate::browser::runtime::target(connection);
    fixture.service.claim(&target).unwrap();
    fixture
        .mcp(&acme, "browser_status", json!({}))
        .await
        .unwrap();
    let (_, directory) = fixture.service.browser_identity().unwrap();
    let path = directory.join("browser status.json");
    // Status is read-only. A directory at the record path forces the close's
    // subsequent evidence write to fail without changing the verified close.
    assert!(!path.exists());
    std::fs::create_dir(&path).unwrap();
    let closed = fixture
        .mcp(&acme, "disconnect", json!({"targetId":target}))
        .await
        .unwrap();
    assert_eq!(closed["closed"], true);
    assert_eq!(closed["statusRecordError"]["code"], "STATE_IO", "{closed}");
    assert!(!fixture.pooled(&target));
    assert!(fixture.service.browsers.connections(&pool).is_empty());
    assert_eq!(fixture.service.targets(), std::slice::from_ref(&device));
    fixture
        .mcp(&acme, "disconnect", json!({"targetId":device}))
        .await
        .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn work_on_one_target_never_waits_for_another_target() {
    let fixture = Fixture::new();
    let acme = fixture.agent("acme-parallel");
    let (held, free) = (target(), target());
    for device in [&held, &free] {
        fixture
            .cli(&acme, "connect", json!({"targetId":device}))
            .await
            .unwrap();
    }
    let held_directory = fixture.target_directory(&held);
    storage::write(&held_directory.join("hold-batch"), &true).unwrap();
    let blocked = fixture.spawn(&acme, "cli", "act", on(&held, tap()), "acme-held");
    fixture
        .until(|_| dispatches(&held_directory).len() == 1)
        .await;
    let done = tokio::time::timeout(
        Duration::from_secs(5),
        fixture.cli(&acme, "act", on(&free, tap())),
    )
    .await
    .expect("A held target must not delay another target")
    .unwrap();
    assert_eq!(done["status"], "completed", "{done}");
    std::fs::remove_file(held_directory.join("hold-batch")).unwrap();
    assert_eq!(blocked.await.unwrap().unwrap()["status"], "completed");
    for device in [&held, &free] {
        fixture
            .cli(&acme, "disconnect", json!({"targetId":device}))
            .await
            .unwrap();
        assert!(!lease_held(device));
    }
    fixture
        .until(|fixture| fixture.service.tracked() == (0, 0))
        .await;
}

#[cfg(unix)]
#[tokio::test]
async fn a_disconnect_waits_behind_queued_input_and_releases_for_every_agent() {
    let fixture = Fixture::new();
    let (user, leaver) = (fixture.agent("acme-user"), fixture.agent("acme-leaver"));
    let device = target();
    fixture
        .cli(&user, "connect", json!({"targetId":device}))
        .await
        .unwrap();
    let directory = fixture.directory();
    storage::write(&directory.join("hold-batch"), &true).unwrap();
    let act = fixture.spawn(&user, "cli", "act", tap(), "acme-before-disconnect");
    fixture.until(|_| dispatches(&directory).len() == 1).await;
    let disconnect = fixture.spawn(&leaver, "mcp", "disconnect", json!({}), "acme-leave");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !disconnect.is_finished(),
        "The disconnect must wait its turn"
    );
    assert!(fixture.pooled(&device));
    std::fs::remove_file(directory.join("hold-batch")).unwrap();
    assert_eq!(act.await.unwrap().unwrap()["state"], "ready");
    assert_eq!(disconnect.await.unwrap().unwrap()["closed"], true);
    assert!(!fixture.pooled(&device));
    let refused = fixture.cli(&user, "act", tap()).await.unwrap_err();
    assert_eq!(refused.code, "NOT_CONNECTED");
}

#[cfg(unix)]
#[tokio::test]
async fn client_departure_and_host_task_end_release_no_device() {
    let fixture = Fixture::new();
    let (leaving, staying) = (fixture.agent("thread-1"), fixture.agent("thread-2"));
    let device = target();
    fixture
        .mcp(&leaving, "connect", json!({"targetId":device}))
        .await
        .unwrap();
    // A client abandons its queued request: only that request ends.
    let directory = fixture.directory();
    storage::write(&directory.join("hold-batch"), &true).unwrap();
    let running = fixture.spawn(&staying, "cli", "act", tap(), "acme-running");
    fixture.until(|_| dispatches(&directory).len() == 1).await;
    let abandoned = fixture.spawn(&leaving, "mcp", "act", tap(), "acme-abandoned");
    let queue = fixture.service.queue(Some(&device));
    fixture.until(|_| queue.waiting() == 1).await;
    abandoned.abort();
    let ended = fixture
        .control("task_end", json!({"hostLabel":"thread-1"}))
        .await
        .unwrap();
    assert_eq!(ended["released"], json!([]), "{ended}");
    std::fs::remove_file(directory.join("hold-batch")).unwrap();
    assert_eq!(running.await.unwrap().unwrap()["state"], "ready");
    // The abandoned input never reached the device, and the device stays.
    assert_eq!(dispatches(&directory), ["acme-running"]);
    assert!(fixture.pooled(&device));
    assert_eq!(
        fixture.cli(&staying, "act", tap()).await.unwrap()["state"],
        "ready"
    );
    fixture
        .cli(&staying, "disconnect", json!({}))
        .await
        .unwrap();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn an_idle_device_is_released_and_use_from_any_agent_renews_it() {
    let fixture = Fixture::idle(Duration::from_millis(600));
    let (first, second) = (fixture.agent("acme-first"), fixture.agent("acme-second"));
    let device = target();
    fixture
        .cli(&first, "connect", json!({"targetId":device}))
        .await
        .unwrap();
    let (running, _stop) = host::fixture(fixture.service.clone()).await;
    // Another agent's use keeps the device past one idle period.
    for _ in 0..4 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        fixture.cli(&second, "observe", json!({})).await.unwrap();
    }
    assert!(fixture.pooled(&device));
    // Without use it is released, recordings finalized, and the manager exits.
    fixture.until(|fixture| !fixture.pooled(&device)).await;
    assert!(!lease_held(&device));
    assert!(fixture.directory().join("recordings-finalized").exists());
    let exited = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(exited["reason"], "empty");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_release_is_retried_automatically_and_holds_no_other_device() {
    let fixture = Fixture::new();
    let (owner, other) = (fixture.agent("acme-owner"), fixture.agent("acme-other"));
    let (locked, ready) = (target(), target());
    for device in [&locked, &ready] {
        fixture
            .cli(&owner, "connect", json!({"targetId":device}))
            .await
            .unwrap();
    }
    let actor = fixture.owner_directory(&locked);
    storage::write(&actor.join("fail-cleanup"), &true).unwrap();
    let failed = fixture
        .cli(&owner, "disconnect", json!({"targetId":locked}))
        .await
        .unwrap_err();
    assert_eq!(failed.code, "CLEANUP_FAILED", "{failed:?}");
    assert!(failed.retryable);
    let details = failed.details.unwrap();
    assert_eq!(details["retrying"], true, "{details}");
    assert!(details["nextAction"].is_string());
    assert!(lease_held(&locked), "The failed device keeps its resources");
    // The other device keeps working for every agent.
    assert_eq!(
        fixture.cli(&other, "act", on(&ready, tap())).await.unwrap()["status"],
        "completed"
    );
    // Status and blockers report the pending release and its retry.
    let status = fixture
        .cli(&other, "status", json!({"targetId":locked}))
        .await
        .unwrap();
    assert_eq!(status["state"], "releasing", "{status}");
    assert_eq!(status["release"]["retrying"], true);
    let blockers = fixture.control("blockers", json!({})).await.unwrap();
    assert_eq!(blockers["blockers"][0]["kind"], "release", "{blockers}");
    assert_eq!(blockers["blockers"][0]["targetId"], locked);
    // A connect retries the release first and reports the blocker.
    let refused = fixture
        .cli(&other, "connect", json!({"targetId":locked}))
        .await
        .unwrap_err();
    assert_eq!(refused.code, "CLEANUP_FAILED", "{refused:?}");
    assert_eq!(refused.details.unwrap()["retrying"], true);
    // Once the cause clears, the manager's own retry finishes the release.
    let (running, _stop) = host::fixture(fixture.service.clone()).await;
    std::fs::remove_file(actor.join("fail-cleanup")).unwrap();
    fixture.until(|fixture| !fixture.pooled(&locked)).await;
    assert!(!lease_held(&locked));
    assert!(fixture.pooled(&ready));
    assert_eq!(
        fixture
            .cli(&other, "connect", json!({"targetId":locked}))
            .await
            .unwrap()["state"],
        "ready"
    );
    for device in [&locked, &ready] {
        fixture
            .cli(&other, "disconnect", json!({"targetId":device}))
            .await
            .unwrap();
    }
    let exited = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(exited["reason"], "empty");
}

#[cfg(unix)]
#[tokio::test]
async fn recovery_answers_at_once_while_a_release_is_slow() {
    let fixture = Fixture::new();
    let acme = fixture.agent("acme-stuck");
    let device = target();
    fixture
        .cli(&acme, "connect", json!({"targetId":device}))
        .await
        .unwrap();
    let actor = fixture.owner_directory(&device);
    storage::write(&actor.join("fail-cleanup"), &true).unwrap();
    fixture
        .cli(&acme, "disconnect", json!({}))
        .await
        .unwrap_err();
    storage::write(&actor.join("cleanup-delay-ms"), &2000_u64).unwrap();
    let started = std::time::Instant::now();
    let blockers = fixture.control("blockers", json!({})).await.unwrap();
    assert_eq!(blockers["blockers"][0]["expiry"]["unknown"], true);
    let pending = fixture
        .control("retry", json!({"targetId":device}))
        .await
        .unwrap();
    assert_eq!(pending["state"], "pending", "{pending}");
    assert!(started.elapsed() < Duration::from_secs(2));
    let operation = pending["operationId"].as_str().unwrap().to_owned();
    let record = fixture
        .service
        .root
        .join("operations")
        .join(format!("{operation}.json"));
    fixture
        .until(|_| {
            storage::read::<Value>(&record)
                .ok()
                .flatten()
                .is_some_and(|record| record["state"] == "failed")
        })
        .await;
    std::fs::remove_file(actor.join("fail-cleanup")).unwrap();
    storage::write(&actor.join("cleanup-delay-ms"), &20_u64).unwrap();
    let recovered = fixture.control("retry", json!({})).await.unwrap();
    assert_eq!(recovered["state"], "completed", "{recovered}");
    assert!(!fixture.pooled(&device));
}

#[cfg(unix)]
#[tokio::test]
async fn a_release_attempt_that_stops_unfinished_leaves_a_pending_retry() {
    let fixture = Fixture::new();
    let acme = fixture.agent("acme-release");
    let device = target();
    fixture
        .cli(&acme, "connect", json!({"targetId":device}))
        .await
        .unwrap();
    fixture
        .service
        .releases
        .lock()
        .unwrap()
        .insert(device.clone(), tokio::sync::watch::channel(None).1);
    // As when the attempt panics partway: its guard still runs.
    drop(super::lifetime::Releasing {
        service: fixture.service.clone(),
        target: device.clone(),
        finished: false,
    });
    assert!(
        !fixture
            .service
            .releases
            .lock()
            .unwrap()
            .contains_key(&device)
    );
    assert_eq!(
        fixture.service.pending_release(&device).unwrap().attempts,
        1
    );
    fixture.service.release_settled(&device);
    fixture.cli(&acme, "disconnect", json!({})).await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn a_running_recording_is_finalized_when_its_device_is_released() {
    let fixture = Fixture::new();
    let acme = fixture.agent("acme-recording");
    fixture
        .cli(&acme, "connect", json!({"targetId":target()}))
        .await
        .unwrap();
    let closed = fixture.cli(&acme, "disconnect", json!({})).await.unwrap();
    assert_eq!(closed["closed"], true);
    assert_eq!(
        closed["recordings"][0]["recordingId"], "acme-recording",
        "{closed}"
    );
    assert!(fixture.directory().join("recordings-finalized").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn a_lost_response_is_one_request_and_a_refusal_can_be_sent_again() {
    let fixture = Fixture::new();
    let acme = fixture.agent("acme-lost");
    // Refused before admission: nothing is connected and nothing is kept.
    let refused = fixture
        .request(&acme, "mcp", "observe", json!({}), "acme-same")
        .await
        .unwrap_err();
    assert_eq!(refused.code, "NOT_CONNECTED", "{refused:?}");
    fixture
        .cli(&acme, "connect", json!({"targetId":target()}))
        .await
        .unwrap();
    let observed = fixture
        .request(&acme, "mcp", "observe", json!({}), "acme-same")
        .await
        .unwrap();
    assert_eq!(observed["state"], "ready", "{observed}");
    // The same request ID is one request: no repeated input.
    let first = fixture
        .request(&acme, "cli", "act", tap(), "acme-lost")
        .await
        .unwrap();
    let again = fixture
        .request(&acme, "cli", "act", tap(), "acme-lost")
        .await
        .unwrap();
    assert_eq!(first, again);
    assert_eq!(dispatches(&fixture.directory()).len(), 1);
    fixture.cli(&acme, "disconnect", json!({})).await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn a_started_device_transition_keeps_its_queue_for_every_agent() {
    let fixture = Fixture::new();
    let (acme, other) = (
        fixture.agent("acme-transition"),
        fixture.agent("acme-other"),
    );
    let serial = target();
    std::fs::write(fixture.directory().join("transition-target"), &serial).unwrap();
    let started = target();
    fixture
        .cli(&acme, "connect", json!({"targetId":started}))
        .await
        .unwrap();
    let transition: Value =
        storage::read(&fixture.owner_directory(&started).join("transition.json"))
            .unwrap()
            .unwrap();
    assert_eq!(transition["to"], serial);
    // Another agent naming the device's new identity reaches the same device.
    let acted = fixture
        .cli(&other, "act", on(&serial, tap()))
        .await
        .unwrap();
    assert_eq!(acted["status"], "completed", "{acted}");
    assert_eq!(dispatches(&fixture.directory()).len(), 1);
    fixture
        .cli(&other, "disconnect", json!({"targetId":serial}))
        .await
        .unwrap();
    assert!(!fixture.pooled(&serial));
    assert!(!fixture.pooled(&started));
}

#[cfg(unix)]
#[tokio::test]
async fn a_ready_started_device_reconciles_its_alias_after_a_lost_connect_deadline() {
    let fixture = Fixture::new();
    let (acme, other) = (fixture.agent("acme-startup"), fixture.agent("acme-other"));
    let (started, serial) = (target(), target());
    let directory = fixture.directory();
    storage::write(&directory.join("hold-startup"), &true).unwrap();
    std::fs::write(directory.join("transition-target"), &serial).unwrap();
    let service = fixture.service.clone();
    let request = message(
        &acme,
        "cli",
        "connect",
        &json!({"targetId":started}),
        Some("acme-slow-connect"),
    );
    // Run the same dispatcher on this test runtime so its caller deadline can
    // be advanced without changing the separate device actor's clock.
    let connecting = service.dispatch(request);
    tokio::pin!(connecting);
    tokio::select! {
        result = &mut connecting => panic!("Startup completed while held: {result:?}"),
        () = fixture.until(|f| {
            storage::read::<Value>(&f.target_directory(&started).join("selected.json"))
                .unwrap()
                .is_some_and(|selected| {
                    PathBuf::from(selected["directory"].as_str().unwrap())
                        .join("s")
                        .is_dir()
                })
        }) => {}
    }
    // Hold readiness with a condition and advance the actual caller deadline;
    // reproducing a cold-start timeout must not depend on host speed.
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(91)).await;
    assert_eq!(connecting.await.unwrap_err().code, "RECOVERING");
    tokio::time::resume();
    std::fs::remove_file(directory.join("hold-startup")).unwrap();
    fixture
        .until(|f| {
            storage::read::<Value>(&f.owner_directory(&started).join("state.json"))
                .unwrap()
                .is_some_and(|state| state["state"] == "ready")
        })
        .await;
    assert_eq!(
        fixture
            .cli(&acme, "status", json!({"targetId":started}))
            .await
            .unwrap()["state"],
        "ready"
    );
    let reconnected = fixture
        .cli(&acme, "connect", json!({"targetId":started}))
        .await
        .unwrap();
    assert_eq!(reconnected["reused"], true);
    assert_eq!(reconnected["targetId"], serial);
    assert_eq!(
        fixture.service.held_target(&serial).as_deref(),
        Some(started.as_str())
    );
    assert_eq!(
        fixture
            .cli(&other, "act", on(&serial, tap()))
            .await
            .unwrap()["status"],
        "completed"
    );
    assert_eq!(dispatches(&directory).len(), 1);
    // Native emulator startup holds its resolved serial lease as well as the
    // AVD lease. App mutations must reuse this actor, not try one-off leases.
    let emulator_lease = crate::lease::ControlLease::acquire(&serial).unwrap();
    let apk = fixture.root.path().join("Acme.apk");
    std::fs::write(
        &apk,
        b"Acme actor-delegation fixture; not an installable APK",
    )
    .unwrap();
    let installed = fixture
        .cli(&other, "app_install", json!({"targetId":serial,"path":apk}))
        .await
        .unwrap();
    assert_eq!(installed["fixture"], true);
    let uninstalled = fixture
        .cli(
            &other,
            "app_uninstall",
            json!({"targetId":serial,"appId":"com.example.acme"}),
        )
        .await
        .unwrap();
    assert_eq!(uninstalled["fixture"], true);
    fixture
        .cli(&other, "disconnect", json!({"targetId":serial}))
        .await
        .unwrap();
    drop(emulator_lease);
    assert!(!fixture.pooled(&serial));
    assert!(!fixture.pooled(&started));
}

#[cfg(unix)]
#[tokio::test]
async fn a_panicking_actor_is_quarantined_until_recovery_and_other_devices_continue() {
    let fixture = Fixture::new();
    let acme = fixture.agent("acme-panic");
    let (broken, healthy) = (target(), target());
    fixture
        .cli(&acme, "connect", json!({"targetId":broken}))
        .await
        .unwrap();
    storage::write(&fixture.directory().join("panic-actor"), &true).unwrap();
    fixture
        .until(|fixture| fixture.service.supervisor.quarantined(&broken).is_some())
        .await;
    std::fs::remove_file(fixture.directory().join("panic-actor")).unwrap();
    // Its device lease stays with the supervisor; no work is admitted.
    assert!(lease_held(&broken));
    let refused = fixture.cli(&acme, "act", tap()).await.unwrap_err();
    assert_eq!(refused.code, "RELEASE_PENDING", "{refused:?}");
    fixture
        .cli(&acme, "connect", json!({"targetId":healthy}))
        .await
        .unwrap();
    assert_eq!(
        fixture
            .cli(&acme, "act", on(&healthy, tap()))
            .await
            .unwrap()["status"],
        "completed"
    );
    let blockers = fixture.control("blockers", json!({})).await.unwrap();
    assert!(
        blockers["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |blocker| blocker["kind"] == "quarantine" && blocker["cause"] == "Acme actor panic"
            ),
        "{blockers}"
    );
    let recovered = fixture.control("retry", json!({})).await.unwrap();
    assert_eq!(recovered["state"], "completed", "{recovered}");
    fixture
        .until(|fixture| fixture.service.supervisor.quarantined(&broken).is_none())
        .await;
    assert!(!lease_held(&broken));
    assert!(!fixture.pooled(&broken));
    assert_eq!(
        fixture
            .cli(&acme, "act", on(&healthy, tap()))
            .await
            .unwrap()["status"],
        "completed"
    );
    fixture.cli(&acme, "disconnect", json!({})).await.unwrap();
}

#[tokio::test]
async fn a_device_actor_starts_only_while_the_manager_is_open() {
    let fixture = Fixture::new();
    let mut started = false;
    fixture.service.open_then(|_| Ok(())).unwrap();
    fixture
        .service
        .start_if_open(|| {
            started = true;
            Ok(())
        })
        .unwrap();
    assert!(started);
    // An exit decided while a connect waited stops the start itself.
    fixture.service.registry().closing = true;
    let refused = fixture
        .service
        .start_if_open(|| panic!("Started while exiting"))
        .unwrap_err();
    assert_eq!(refused.code, "OWNER_EXITING");
}

#[tokio::test(flavor = "multi_thread")]
async fn polling_does_not_hold_an_empty_manager_but_an_open_attachment_does() {
    let fixture = Fixture::new();
    let (running, _stop) = host::fixture(fixture.service.clone()).await;
    let mut client =
        crate::managed::remote::Client::in_root(&fixture.service.root, fixture.agent("acme-poll"));
    let poll = async {
        loop {
            let _ = client.call("status", json!({})).await;
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    tokio::select! {
        result = running => assert_eq!(result.unwrap().unwrap()["reason"], "empty"),
        () = poll => unreachable!(),
        () = tokio::time::sleep(Duration::from_secs(10)) => panic!("An empty manager must exit"),
    }

    let fixture = Fixture::new();
    let (mut running, _stop) = host::fixture(fixture.service.clone()).await;
    let mut client =
        crate::managed::remote::Client::in_root(&fixture.service.root, fixture.agent("acme-mcp"));
    client.attach_mcp("acme-connection".into());
    client.ensure_attached().await.unwrap();
    // Past the idle grace, an MCP server's attachment still holds the manager.
    let held = tokio::time::timeout(Duration::from_millis(600), &mut running).await;
    assert!(held.is_err());
    client.shutdown().await.unwrap();
    let exited = tokio::time::timeout(Duration::from_secs(10), running).await;
    assert_eq!(exited.unwrap().unwrap().unwrap()["reason"], "empty");
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn an_incompatible_manager_serves_control_retires_when_empty_and_drains_when_busy() {
    let fixture = Fixture::with(PROTOCOL + 1);
    // Held until retirement: the empty manager's idle grace is 200 ms.
    let (running, _stop, held) = host::fixture_held(fixture.service.clone()).await;
    let mut client = crate::managed::remote::Client::in_root(
        &fixture.service.root,
        fixture.agent("acme-upgrade"),
    );
    let refused = client.call("observe", json!({})).await.unwrap_err();
    assert_eq!(refused.code, "OWNER_INCOMPATIBLE");
    assert_eq!(
        refused.details.unwrap()["identity"]["protocol"],
        PROTOCOL + 1
    );
    // The control subset answers whatever the protocol.
    assert_eq!(
        client.control("pool", json!({})).await.unwrap()["live"],
        true
    );
    drop(held);
    assert_eq!(
        client.control("retire", json!({})).await.unwrap()["retired"],
        true
    );
    let exited = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(exited["reason"], "retired");

    let fixture = Fixture::new();
    let (running, _stop, held) = host::fixture_held(fixture.service.clone()).await;
    let busy = fixture.agent("acme-busy");
    let mut client = crate::managed::remote::Client::in_root(&fixture.service.root, busy);
    let device = target();
    client
        .call("connect", json!({"targetId":device}))
        .await
        .unwrap();
    // The connected device holds the manager from here on.
    drop(held);
    // The library's retirement sees the busy hold and reports the pool.
    fixture.until(|fixture| fixture.service.busy_held()).await;
    let retire = client.control("retire", json!({})).await.unwrap();
    assert_eq!(retire["retired"], false);
    assert_eq!(retire["pool"]["targets"][0]["targetId"], device);
    // While a batch runs, the drain refuses new requests and lets the
    // admitted one finish.
    let directory = fixture.directory();
    storage::write(&directory.join("hold-batch"), &true).unwrap();
    let mut acting = client.clone();
    let act = acting.call("act", tap());
    let drain = async {
        fixture.until(|_| dispatches(&directory).len() == 1).await;
        assert_eq!(
            client.control("drain", json!({})).await.unwrap()["draining"],
            true
        );
        let mut late = crate::managed::remote::Client::in_root(
            &fixture.service.root,
            fixture.agent("acme-new"),
        );
        let refused = late
            .call("connect", json!({"targetId":target()}))
            .await
            .unwrap_err();
        assert_eq!(refused.code, "OWNER_DRAINING");
        // Status still answers on the control lane while draining.
        let status = late
            .call("status", json!({"targetId":device}))
            .await
            .unwrap();
        assert!(status["state"].is_string(), "{status}");
        assert_ne!(status["owner"], "absent", "{status}");
        // Once nothing runs, the drain releases the device and retires.
        std::fs::remove_file(directory.join("hold-batch")).unwrap();
    };
    let (acted, ()) = tokio::join!(act, drain);
    assert_eq!(acted.unwrap()["state"], "ready");
    let exited = tokio::time::timeout(Duration::from_secs(10), running)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(exited["reason"], "drained");
    assert!(!lease_held(&device));
}

#[tokio::test(flavor = "multi_thread")]
async fn work_outside_a_request_holds_the_manager_and_exit_closes_admission() {
    let fixture = Fixture::new();
    let (mut running, _stop) = host::fixture(fixture.service.clone()).await;
    // Work started outside any request, such as recovery at startup, holds
    // the manager past its idle grace until it ends.
    fixture
        .until(|fixture| fixture.service.hold().is_some())
        .await;
    let hold = fixture.service.hold().unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(600), &mut running)
            .await
            .is_err()
    );
    drop(hold);
    let exited = tokio::time::timeout(Duration::from_secs(5), running)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(exited["reason"], "empty");
    // Nothing starts after the exit, and a client finds no manager: definite
    // non-admission.
    let refused = fixture
        .service
        .start_if_open(|| panic!("Started after exit"))
        .unwrap_err();
    assert_eq!(refused.code, "OWNER_EXITING");
    let mut late =
        crate::managed::remote::Client::in_root(&fixture.service.root, fixture.agent("acme-late"));
    let absent = late.call("observe", json!({})).await.unwrap_err();
    assert_eq!(absent.code, "OWNER_ABSENT");
}

#[cfg(unix)]
#[tokio::test]
async fn previews_and_recordings_are_limited_across_the_manager() {
    let fixture = Fixture::new();
    let (first, second) = (fixture.agent("acme-media-1"), fixture.agent("acme-media-2"));
    let (left, right) = (target(), target());
    for device in [&left, &right] {
        fixture
            .cli(&first, "connect", json!({"targetId":device}))
            .await
            .unwrap();
    }
    let mut previews = Vec::new();
    for index in 0..16 {
        let (agent, device) = if index % 2 == 0 {
            (&first, &left)
        } else {
            (&second, &right)
        };
        let started = fixture
            .cli(agent, "preview_start", on(device, json!({})))
            .await
            .unwrap();
        previews.push(started["previewId"].as_str().unwrap().to_owned());
    }
    let refused = fixture
        .cli(&first, "preview_start", on(&left, json!({})))
        .await
        .unwrap_err();
    assert_eq!(refused.code, "RESOURCE_LIMIT");
    assert_eq!(refused.details.unwrap()["admitted"], false);
    // A preview stops by its ID alone, from any agent.
    fixture
        .cli(&first, "preview_stop", json!({"previewId":previews[1]}))
        .await
        .unwrap();
    fixture
        .cli(&first, "preview_start", on(&left, json!({})))
        .await
        .unwrap();
    for index in 0..8 {
        let device = if index % 2 == 0 { &left } else { &right };
        let recording = fixture
            .cli(&second, "recording_start", on(device, json!({})))
            .await
            .unwrap();
        let status = fixture
            .cli(
                &first,
                "recording_status",
                json!({"recordingId":recording["recordingId"]}),
            )
            .await
            .unwrap();
        assert_eq!(status["status"], "active");
    }
    let refused = fixture
        .cli(&second, "recording_start", on(&right, json!({})))
        .await
        .unwrap_err();
    assert_eq!(refused.code, "RESOURCE_LIMIT");
    for device in [&left, &right] {
        fixture
            .cli(&first, "disconnect", json!({"targetId":device}))
            .await
            .unwrap();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn device_preparation_queues_on_the_device_for_every_agent() {
    let fixture = Fixture::new();
    let (holder, other) = (fixture.agent("acme-holder"), fixture.agent("acme-other"));
    let device = target();
    fixture
        .cli(&holder, "connect", json!({"targetId":device}))
        .await
        .unwrap();
    // Another agent's preparation waits its turn behind queued input.
    let directory = fixture.directory();
    storage::write(&directory.join("hold-batch"), &true).unwrap();
    let queue = fixture.service.queue(Some(&device));
    let act = fixture.spawn(&holder, "cli", "act", tap(), "acme-act");
    fixture.until(|_| dispatches(&directory).len() == 1).await;
    let preparing = fixture.spawn(
        &other,
        "cli",
        "developer_mode_reveal",
        json!({"targetId":device}),
        "acme-prepare",
    );
    fixture.until(|_| queue.waiting() == 1).await;
    assert!(!preparing.is_finished());
    std::fs::remove_file(directory.join("hold-batch")).unwrap();
    assert_eq!(act.await.unwrap().unwrap()["state"], "ready");
    // This fixture device is no iPhone, so the preparation itself refuses it.
    let prepared = preparing.await.unwrap().unwrap_err();
    assert_ne!(prepared.code, "NOT_CONNECTED");
    // On a device that is not connected it runs once and leaves nothing pooled.
    let free = target();
    let refused = fixture
        .cli(&other, "developer_mode_reveal", json!({"targetId":free}))
        .await
        .unwrap_err();
    assert_ne!(refused.code, "NOT_CONNECTED");
    assert!(!fixture.pooled(&free));
    fixture.cli(&holder, "disconnect", json!({})).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn detached_work_blocks_no_async_worker() {
    let (release, released) = std::sync::mpsc::channel::<()>();
    // Synchronous waiting, as record I/O does, inside detached work.
    let blocked = detached(move || async move {
        released.recv().unwrap();
    });
    // The only async worker still runs other tasks meanwhile.
    let other = tokio::spawn(async { 42 });
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), other)
            .await
            .unwrap()
            .unwrap(),
        42
    );
    release.send(()).unwrap();
    blocked.await.unwrap();
}

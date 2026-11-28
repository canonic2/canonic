//! Real CLI/MCP browser sessions with automatic test-owned private acquisition.
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
};

mod connections;
mod investigation;
mod journeys;

struct Fixture {
    workspace: crate::Workspace,
    cache: std::path::PathBuf,
}
impl Fixture {
    fn command(&self) -> Command {
        let executable = std::env::var_os("TESTRUN_TEST_INSTALLED_BINARY")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_testrun").into());
        assert!(
            executable.is_absolute() && executable.is_file(),
            "Select an existing absolute Testrun binary"
        );
        let mut command = Command::new(executable);
        command.arg("--json");
        command
            .current_dir(self.workspace.path())
            .env("TESTRUN_DATA_ROOT", self.workspace.data_root())
            .env("TESTRUN_BROWSER_ROOT", &self.cache)
            .env_remove("CODEX_THREAD_ID")
            .env_remove("TESTRUN_TASK_ID")
            .kill_on_drop(true);
        command
    }
    async fn cli(&self, args: &[&str]) -> Value {
        self.cli_in(self.workspace.path(), args).await
    }
    async fn cli_in(&self, directory: &std::path::Path, args: &[&str]) -> Value {
        let output = tokio::time::timeout(
            Duration::from_secs(360),
            self.command().current_dir(directory).args(args).output(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    async fn ready_target(&self, target: &str) -> Value {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
        loop {
            let status = self.cli(&["browser", "status", "--target", target]).await;
            assert!(status["failure"].is_null(), "{status}");
            assert_eq!(status["targetId"], target, "{status}");
            match status["browser"]["phase"].as_str() {
                Some("ready") => return status,
                None | Some("starting") => {}
                _ => panic!("Browser did not become ready: {status}"),
            }
            assert!(tokio::time::Instant::now() < deadline, "{status}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    async fn ready(&self) -> Value {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
        loop {
            let status = self.cli(&["browser", "status"]).await;
            assert!(status["failure"].is_null(), "{status}");
            // An untargeted status lists every browser; wait on the only one.
            if let Some([only]) = status["browsers"].as_array().map(Vec::as_slice)
                && let Some(target) = only["targetId"].as_str()
            {
                return self.ready_target(target).await;
            }
            if status["browser"]["phase"] == "ready" {
                return status;
            }
            if !status["browser"]["phase"].is_null() && status["browser"]["phase"] != "starting" {
                let startup = if let Some(id) = status["startupOperationId"].as_str() {
                    self.cli(&[
                        "browser",
                        "status",
                        "--operation-id",
                        id,
                        "--wait-ms",
                        "30000",
                    ])
                    .await
                } else {
                    Value::Null
                };
                panic!("Browser failed readiness: {status}; startup completion: {startup}");
            }
            assert!(tokio::time::Instant::now() < deadline, "{status}");
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}
struct Mcp {
    child: Child,
    input: tokio::process::ChildStdin,
    output: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    id: u64,
}
impl Mcp {
    async fn start(fixture: &Fixture) -> Self {
        let mut child = fixture
            .command()
            .args(["mcp", "serve"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap()).lines();
        let mut mcp = Self {
            child,
            input,
            output,
            id: 0,
        };
        mcp.request("initialize",json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"Acme browser fixture","version":"1"}})).await;
        mcp.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .await;
        mcp
    }
    async fn send(&mut self, value: Value) {
        self.input
            .write_all(format!("{value}\n").as_bytes())
            .await
            .unwrap();
    }
    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let id = self.id;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(360);
        loop {
            let line = tokio::select! {
                line=tokio::time::timeout_at(deadline,self.output.next_line())=>line.expect("MCP response deadline").unwrap().expect("MCP EOF"),
                status=self.child.wait()=>panic!("MCP exited while waiting for {method}: {status:?}"),
            };
            let reply: Value = serde_json::from_str(&line).unwrap();
            if reply["id"] == id {
                return reply;
            }
        }
    }
    async fn tool(&mut self, name: &str, args: Value) -> Value {
        let response = self
            .request("tools/call", json!({"name":name,"arguments":args}))
            .await;
        let result = response["result"]["structuredContent"].clone();
        assert_eq!(result["ok"], true, "{response}");
        result["data"].clone()
    }
    async fn detailed_browser_status(&mut self, target: &str) -> Value {
        self.tool(
            "browser_status",
            json!({"targetId":target,"includeDetails":true}),
        )
        .await
    }
    async fn stop(mut self) {
        drop(self.input);
        assert!(
            tokio::time::timeout(Duration::from_secs(30), self.child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
    }
}

#[tokio::test]
async fn cli_and_mcp_prepare_share_and_release_firefox_and_webkit() {
    let workspace = crate::Workspace::new();
    let cache = workspace.path().join("browser-cache");
    let fixture = Fixture { workspace, cache };
    let listed = fixture.cli(&["browser", "list"]).await;
    assert_eq!(listed["browsers"].as_array().unwrap().len(), 3);
    assert!(!fixture.cache.exists());
    assert!(!fixture.workspace.data_root().join("owner.log").exists());
    for engine in ["firefox", "webkit"] {
        let connected = fixture
            .cli(&["browser", "connect", "--engine", engine, "--downloads"])
            .await;
        assert!(connected["preparationOperationId"].is_string());
        let ready = fixture.ready().await;
        let connection = ready["browser"]["connectionId"].clone();
        let mut mcp = Mcp::start(&fixture).await;
        // MCP joins the session and reaches its only browser, a target of
        // its own, without launching another.
        let listed = mcp.tool("browser_status", json!({})).await;
        let [joined] = listed["browsers"].as_array().unwrap().as_slice() else {
            panic!("Expected one shared browser: {listed}");
        };
        assert_eq!(joined["browser"]["connectionId"], connection);
        assert_eq!(
            joined["targetId"],
            format!("browser:{}", connection.as_str().unwrap())
        );
        page_workflow(
            &fixture,
            &mut mcp,
            ready["browser"]["selectedPage"].clone(),
            ready["browser"]["selectedContext"].clone(),
        )
        .await;
        let request = json!({"request":{"operation":"context_create"}});
        let submitted = mcp.tool("browser_send", request).await;
        let id = submitted["operation"]["id"].as_str().unwrap();
        let completed = fixture
            .cli(&[
                "browser",
                "status",
                "--operation-id",
                id,
                "--wait-ms",
                "30000",
            ])
            .await;
        assert_eq!(
            completed["operation"]["outcome"], "succeeded",
            "{completed}"
        );
        let context = completed["operation"]["result"]["context"].clone();
        let invalid=mcp.request("tools/call",json!({"name":"browser_send","arguments":{"request":{"operation":"context_list","foreign":true}}})).await;
        assert_eq!(invalid["result"]["structuredContent"]["ok"], false);
        // A plain disconnect with no device connected releases nothing:
        // browsers close only through browser_disconnect.
        let closed = fixture.cli(&["disconnect"]).await;
        assert_eq!(closed["closed"], true, "{closed}");
        assert_eq!(
            mcp.tool("browser_status", json!({"targetId":joined["targetId"]}))
                .await["browser"]["connectionId"],
            connection
        );
        let submitted = mcp
            .tool(
                "browser_send",
                json!({"request":{"operation":"context_close","context":context}}),
            )
            .await;
        let outcome = mcp
            .tool(
                "browser_status",
                json!({"operationId":submitted["operation"]["id"],"waitMs":30000}),
            )
            .await;
        assert_eq!(outcome["operation"]["outcome"], "succeeded", "{outcome}");
        let closed = mcp.tool("browser_disconnect", json!({})).await;
        assert_eq!(closed["closed"], true, "{closed}");
        assert_eq!(closed["targetId"], joined["targetId"]);
        let released = mcp.tool("disconnect", json!({})).await;
        assert_eq!(released["closed"], true, "{released}");
        mcp.stop().await;
        let ended = fixture.cli(&["browser", "status"]).await;
        assert_eq!(ended["live"], false, "{ended}");
        assert_eq!(ended["state"], "not_connected", "{ended}");
    }
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        // Explicit disconnect releases the reference; the owner observes the
        // socket closing asynchronously. EOF alone intentionally keeps cooldown.
        let retired = fixture.cli(&["manager", "stop"]).await;
        if retired["retired"] == true {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Owner did not retire: {retired}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn page_workflow(fixture: &Fixture, mcp: &mut Mcp, page: Value, context: Value) {
    async fn settled(mcp: &mut Mcp, request: Value) -> Value {
        let submitted = mcp.tool("browser_send", json!({"request":request})).await;
        mcp.tool(
            "browser_status",
            json!({"operationId":submitted["operation"]["id"],"waitMs":30000}),
        )
        .await["operation"]
            .clone()
    }
    async fn operation(mcp: &mut Mcp, request: Value) -> Value {
        let status = settled(mcp, request).await;
        assert_eq!(status["outcome"], "succeeded", "{status}");
        status["result"].clone()
    }
    operation(
        mcp,
        json!({"operation":"diagnostics_start","context":context}),
    )
    .await;
    operation(mcp,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"set_content","html":"<h1>Acme</h1><label>Name<input></label><button onclick=\"document.querySelector('output').textContent='Saved'\">Save</button><output>Ready</output>"}})).await;
    let evaluation=operation(mcp,json!({"operation":"evaluate","scope":page,"expression":"() => { console.log('Acme private diagnostic'); const value={name:'Acme'}; value.self=value; return value; }","isFunction":true})).await;
    assert_eq!(evaluation["value"]["kind"], "object", "{evaluation}");
    let handle=operation(mcp,json!({"operation":"evaluate","scope":page,"expression":"() => ({name:'Acme'})","isFunction":true,"returnHandle":true})).await;
    let handle = handle["handle"].clone();
    let value = operation(mcp, json!({"operation":"handle_read","handle":handle})).await;
    assert_eq!(value["value"]["kind"], "object", "{value}");
    let properties = operation(
        mcp,
        json!({"operation":"handle_properties","handle":handle,"query":{"limit":1}}),
    )
    .await;
    assert_eq!(properties["properties"]["names"], json!(["name"]));
    let property = operation(
        mcp,
        json!({"operation":"handle_property","handle":handle,"name":"name"}),
    )
    .await;
    let property = property["handle"].clone();
    operation(mcp, json!({"operation":"handle_dispose","handle":handle})).await;
    let value = operation(mcp, json!({"operation":"handle_read","handle":property})).await;
    assert_eq!(value["value"]["value"], "Acme");
    operation(mcp, json!({"operation":"handle_dispose","handle":property})).await;
    operation(mcp,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"() => new Promise(resolve => { const url=URL.createObjectURL(new Blob([\"self.acmeIdentity={name:'Acme worker'};postMessage('ready')\"],{type:'text/javascript'})); globalThis.acmeWorker=new Worker(url); acmeWorker.onmessage=()=>{URL.revokeObjectURL(url);resolve(true);}; })"})).await;
    let workers = operation(mcp, json!({"operation":"worker_list","page":page})).await;
    assert_eq!(workers["workers"].as_array().unwrap().len(), 1);
    let worker = workers["workers"][0]["worker"].clone();
    let retained=operation(mcp,json!({"operation":"evaluate","scope":worker,"isFunction":true,"expression":"() => self.acmeIdentity","returnHandle":true})).await;
    assert_eq!(retained["worker"], worker);
    assert!(retained["frame"].is_null());
    let worker_handle = retained["handle"].clone();
    let read=operation(mcp,json!({"operation":"evaluate","scope":worker,"isFunction":true,"expression":"value => value.name","argument":{"kind":"handle","handle":worker_handle}})).await;
    assert_eq!(read["value"]["value"], "Acme worker");
    operation(
        mcp,
        json!({"operation":"handle_dispose","handle":worker_handle}),
    )
    .await;
    operation(mcp,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"() => globalThis.acmeWorker.terminate()"})).await;
    let diagnostics = operation(
        mcp,
        json!({"operation":"diagnostics_read","context":context,"query":{"kind":"console"}}),
    )
    .await;
    assert!(
        !diagnostics.to_string().contains("Acme private diagnostic"),
        "{diagnostics}"
    );
    let private=operation(mcp,json!({"operation":"diagnostics_read","context":context,"query":{"kind":"console","includeText":true}})).await;
    assert!(
        private.to_string().contains("Acme private diagnostic"),
        "{private}"
    );
    let preview = operation(mcp, json!({"operation":"preview_start","page":page})).await;
    let preview = preview["preview"].clone();
    let recording = operation(mcp, json!({"operation":"recording_start","page":page})).await;
    let recording = recording["recording"].clone();
    operation(mcp,json!({"operation":"context_configure","context":context,"setting":{"kind":"offline","offline":true}})).await;
    let offline=operation(mcp,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"() => navigator.onLine"})).await;
    assert_eq!(offline["value"]["value"], false);
    operation(mcp,json!({"operation":"context_configure","context":context,"setting":{"kind":"offline","offline":false}})).await;
    operation(mcp,json!({"operation":"context_configure","context":context,"setting":{"kind":"permissions_grant","origin":"https://example.com","permissions":["geolocation"]}})).await;
    operation(mcp,json!({"operation":"context_configure","context":context,"setting":{"kind":"geolocation","geolocation":{"latitude":12.5,"longitude":-34.25,"accuracy":5}}})).await;
    operation(mcp,json!({"operation":"context_configure","context":context,"setting":{"kind":"permissions_clear"}})).await;
    operation(mcp,json!({"operation":"context_configure","context":context,"setting":{"kind":"geolocation","geolocation":null}})).await;
    let media=operation(mcp,json!({"operation":"page_emulate_media","page":page,"options":{"colorScheme":"dark","reducedMotion":"reduce"}})).await;
    assert_eq!(media["observed"]["dark"], true);
    assert_eq!(media["observed"]["reducedMotion"], true);
    assert_eq!(media["previewRecovery"]["succeeded"], true);
    operation(mcp,json!({"operation":"context_clock","context":context,"action":{"kind":"set_fixed_time","timeMs":1700000000000_i64}})).await;
    operation(mcp,json!({"operation":"context_clock","context":context,"action":{"kind":"pause_at","timeMs":1700000000000_i64}})).await;
    operation(mcp,json!({"operation":"context_clock","context":context,"action":{"kind":"run_for","durationMs":2000}})).await;
    let clock=operation(mcp,json!({"operation":"evaluate","scope":page,"isFunction":true,"expression":"() => Date.now()"})).await;
    assert_eq!(clock["value"]["value"], 1700000000000_i64);
    let detached = operation(mcp, json!({"operation":"preview_stop","preview":preview})).await;
    assert_eq!(detached["previewActive"], false, "{detached}");
    assert_eq!(detached["recordingConsumers"], 1, "{detached}");
    operation(mcp,json!({"operation":"page_action","page":page,"locator":{"kind":"label","text":"Name","exact":true},"action":{"kind":"fill","value":"Acme"}})).await;
    let observed = operation(
        mcp,
        json!({"operation":"page_read","page":page,"read":"geometry"}),
    )
    .await;
    let native=operation(mcp,json!({"operation":"page_input","page":page,"observation":observed["inputObservation"],"actions":[{"kind":"type","text":" QA"},{"kind":"key_down","key":"Shift"}]})).await;
    assert_eq!(native["held"]["possiblyHeldKeys"], 1);
    let released=operation(mcp,json!({"operation":"page_input","page":page,"actions":[{"kind":"key_up","key":"ShiftLeft"}]})).await;
    assert_eq!(released["held"]["possiblyHeldKeys"], 0);
    operation(mcp, json!({"operation":"input_release"})).await;
    let state=operation(mcp,json!({"operation":"page_read","page":page,"locator":{"kind":"label","text":"Name","exact":true},"read":"state"})).await;
    assert_eq!(state["result"]["value"]["value"], "Acme QA");
    operation(mcp,json!({"operation":"page_action","page":page,"locator":{"kind":"role","role":"button","name":"Save","exact":true},"action":{"kind":"click"}})).await;
    let read=operation(mcp,json!({"operation":"page_read","page":page,"locator":{"kind":"css","selector":"output"},"read":"text"})).await;
    assert_eq!(read["result"]["value"], "Saved");
    let movie = operation(
        mcp,
        json!({"operation":"recording_stop","recording":recording}),
    )
    .await;
    assert!(
        matches!(movie["status"].as_str(), Some("complete" | "partial")),
        "{movie}"
    );
    assert_eq!(movie["verification"]["decodable"], true, "{movie}");
    assert!(std::path::Path::new(movie["artifact"]["path"].as_str().unwrap()).is_file());
    assert_eq!(
        operation(
            mcp,
            json!({"operation":"recording_stop","recording":recording})
        )
        .await,
        movie
    );
    let submitted = mcp
        .tool(
            "browser_send",
            json!({"request":{"operation":"page_screenshot","page":page}}),
        )
        .await;
    let response=mcp.request("tools/call",json!({"name":"browser_status","arguments":{"operationId":submitted["operation"]["id"],"waitMs":30000}})).await;
    assert_eq!(
        response["result"]["structuredContent"]["data"]["operation"]["outcome"], "succeeded",
        "{response}"
    );
    assert!(
        response["result"]["content"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["type"] == "image" && entry["mimeType"] == "image/png")
    );
    let path = fixture.workspace.path().join("Acme.csv");
    operation(
        mcp,
        json!({"operation":"context_clock","context":context,"action":{"kind":"resume"}}),
    )
    .await;
    std::fs::write(&path, b"Synthetic Acme fixture").unwrap();
    let registered = fixture
        .cli(&[
            "browser",
            "fixture-register",
            path.to_str().unwrap(),
            "--mime-type",
            "text/csv",
            "--provenance",
            "synthetic_generated",
            "--cleared-by",
            "Acme QA",
            "--clearance-method",
            "Generated test data",
        ])
        .await;
    let registered = fixture
        .cli(&[
            "browser",
            "status",
            "--operation-id",
            registered["operation"]["id"].as_str().unwrap(),
            "--wait-ms",
            "30000",
        ])
        .await;
    assert_eq!(
        registered["operation"]["outcome"], "succeeded",
        "{registered}"
    );
    let file = registered["operation"]["result"]["fixture"]["reference"].clone();
    operation(mcp,json!({"operation":"page_navigate","page":page,"navigation":{"kind":"set_content","html":"<input id=file type=file><a id=download download=Acme.txt>Download</a><script>document.querySelector('a').href=URL.createObjectURL(new Blob(['Synthetic Acme download']));</script>"}})).await;
    let attached = operation(mcp,json!({"operation":"page_upload","page":page,"locator":{"kind":"css","selector":"#file"},"fixtures":[file]})).await;
    assert_eq!(attached["attached"], true);
    assert_eq!(attached["serverUploadVerified"], false);
    operation(mcp,json!({"operation":"page_action","page":page,"locator":{"kind":"css","selector":"#download"},"action":{"kind":"click"}})).await;
    let target = format!("browser:{}", page["connectionId"].as_str().unwrap());
    let download = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let status = mcp.detailed_browser_status(&target).await;
            if let Some(download) = status["browser"]["transfers"]["downloads"]
                .as_array()
                .and_then(|items| {
                    items
                        .iter()
                        .find(|d| d["state"] == "complete" || d["state"] == "partial_or_failed")
                })
            {
                break download.clone();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(download["state"], "complete", "{download}");
    assert_eq!(
        std::fs::read(download["artifact"]["path"].as_str().unwrap()).unwrap(),
        b"Synthetic Acme download"
    );
    operation(
        mcp,
        json!({"operation":"download_delete","download":download["reference"]}),
    )
    .await;

    // A failed compound action quarantines only its owned page, and both
    // public clients observe the same possible held state without replay.
    let created = operation(mcp, json!({"operation":"page_create","context":context})).await;
    let failed_page = created["page"].clone();
    operation(mcp, json!({"operation":"page_navigate","page":failed_page,"navigation":{"kind":"set_content","html":"<button>Acme duplicate</button><button>Acme duplicate</button>"}})).await;
    let click = json!({"operation":"page_action","page":failed_page,"locator":{"kind":"role","role":"button","name":"Acme duplicate","exact":true},"action":{"kind":"click"}});
    let failed = settled(mcp, click.clone()).await;
    assert_eq!(failed["outcome"], "unknown", "{failed}");
    assert_eq!(failed["result"]["cause"], "AMBIGUOUS_LOCATOR");
    assert_eq!(failed["result"]["cleanup"]["requiresOwnedPageClose"], true);
    let status = fixture
        .cli(&[
            "browser",
            "status",
            "--target",
            &target,
            "--include-details",
        ])
        .await;
    assert!(
        status["browser"]["input"]["pages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["page"] == failed_page
                && entry["state"]["unknownButtonReleases"] == 1)
    );
    let cleanup = operation(mcp, json!({"operation":"input_release"})).await;
    assert!(
        cleanup["pages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["page"] == failed_page
                && entry["cleanup"]["requiresOwnedPageClose"] == true)
    );
    let refused = settled(mcp, click).await;
    assert_eq!(refused["code"], "INPUT_STATE_CONFLICT");
    operation(mcp, json!({"operation":"page_close","page":failed_page})).await;
    let status = fixture
        .cli(&[
            "browser",
            "status",
            "--target",
            &target,
            "--include-details",
        ])
        .await;
    assert!(
        status["browser"]["input"]["pages"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["page"] != failed_page)
    );
    let original = operation(mcp, json!({"operation":"page_read","page":page,"locator":{"kind":"css","selector":"#download"},"read":"count"})).await;
    assert_eq!(original["result"]["value"], 1);
}

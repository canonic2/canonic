//! Strict public diagnostic schemas shared by CLI, MCP and manager admission.
use serde_json::{Value, json};
pub(crate) const NAMES: &[&str] = &[
    "diagnostics_open",
    "diagnostics_start",
    "diagnostics_status",
    "diagnostics_read",
    "diagnostics_stop",
    "diagnostics_close",
    "diagnostics_remove",
    "diagnostics_analyze",
    "diagnostics_analysis_status",
    "diagnostics_analysis_cancel",
    "diagnostics_query",
    "diagnostics_compare",
    "diagnostics_capabilities",
    "diagnostics_plan",
    "diagnostics_prepare",
    "diagnostics_project",
    "diagnostics_export",
];
fn definitions() -> &'static Vec<Value> {
    static DEFINITIONS: std::sync::OnceLock<Vec<Value>> = std::sync::OnceLock::new();
    DEFINITIONS.get_or_init(build)
}
pub(crate) fn tools() -> Vec<Value> {
    definitions()
        .iter()
        .map(|value| definition(value["name"].as_str().unwrap()).unwrap().clone())
        .collect()
}
pub(crate) fn definition(name: &str) -> Option<&'static Value> {
    let base = definitions().iter().find(|value| value["name"] == name)?;
    if name != "diagnostics_start" {
        return Some(base);
    }
    // Ordinary polling must not build an unrelated engine request catalog.
    static START: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    Some(START.get_or_init(|| {
        let mut tool = base.clone();
        let schema = crate::browser::contract::schemas();
        let mut request = schema["request"].clone();
        request.as_object_mut().unwrap().remove("$defs");
        let variants = tool["inputSchema"]["properties"]["source"]["oneOf"]
            .as_array_mut()
            .unwrap();
        let browser = variants
            .iter_mut()
            .find(|v| v["properties"]["kind"]["const"] == "browser_operation")
            .unwrap();
        browser["properties"]["request"] = request;
        tool["inputSchema"]["$defs"] = schema["request"]["$defs"].clone();
        tool
    }))
}
#[expect(
    clippy::too_many_lines,
    reason = "The public diagnostic tool schema is one declarative contract table, including shared references and its paging declarations"
)]
fn build() -> Vec<Value> {
    let reference = json!({"type":"string","minLength":1,"maxLength":64});
    let label = json!({"type":"string","minLength":1,"maxLength":256});
    let submission = json!({"type":"string","minLength":1,"maxLength":128});
    let object = |properties: Value, required: &[&str]| json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    let binding = object(
        json!({"targetId":label,"appId":label,"buildId":label,"imageId":label,"processId":{"type":"integer","minimum":1},"processGeneration":label,"runtimeId":label,"runtimeGeneration":label,"qaRunId":label,"qaSessionId":label,"actionIds":{"type":"array","maxItems":32,"items":label}}),
        &[],
    );
    let mut source = json!({"oneOf":[object(json!({"kind":{"const":"import"},"path":{"type":"string","minLength":1,"maxLength":4096},"format":{"type":"string","minLength":1,"maxLength":128}}),&["kind","path","format"]),object(json!({"kind":{"const":"build_output"},"operationId":reference}),&["kind","operationId"]),object(json!({"kind":{"const":"android"},"targetId":label,"appId":label,"userId":{"type":"integer","minimum":0,"maximum":99999},"mode":{"enum":["snapshot","follow"],"default":"snapshot"},"includeReports":{"type":"boolean","default":false},"systemContext":{"type":"boolean","default":true},"since":{"type":"string","minLength":3,"maxLength":32,"description":"Device epoch-seconds cursor, with 1..9 decimal places"}}),&["kind","targetId","appId","userId"])]});
    source["oneOf"].as_array_mut().unwrap().push(object(json!({"kind":{"const":"marker"},"name":label,"actionId":label,"phase":{"enum":["capture_start","before_action","after_action","fault_applied","fault_removed","capture_stop","cleanup"]}}),&["kind","name","actionId","phase"]));
    source["oneOf"].as_array_mut().unwrap().push(object(json!({"kind":{"const":"android_state"},"targetId":label,"appId":label,"userId":{"type":"integer","minimum":0,"maximum":99999},"mode":{"enum":["snapshot","follow"],"default":"snapshot"},"groups":{"type":"array","minItems":1,"maxItems":6,"items":{"enum":["process","memory","scheduling","power","frames","connectivity"]}},"intervalMs":{"type":"integer","minimum":500,"maximum":60000,"default":1000},"maxProcesses":{"type":"integer","minimum":1,"maximum":8,"default":4},"label":label}),&["kind","targetId","appId","userId"]));
    let groups =
        &mut source["oneOf"].as_array_mut().unwrap().last_mut().unwrap()["properties"]["groups"];
    groups["maxItems"] = json!(8);
    groups["items"]["enum"]
        .as_array_mut()
        .unwrap()
        .extend([json!("media"), json!("device_context")]);
    source["oneOf"].as_array_mut().unwrap().push(object(json!({"kind":{"const":"ios"},"targetId":label,"appId":label,"transport":{"enum":["usb","network"],"default":"usb"},"collectLogs":{"type":"boolean","default":true},"systemContext":{"type":"boolean","default":true},"logSeconds":{"type":"integer","minimum":1,"maximum":30,"default":3},"deviceSet":{"type":"string","minLength":1,"maxLength":4096},"reportDirectory":{"type":"string","minLength":1,"maxLength":4096}}),&["kind","targetId","appId"]));
    source["oneOf"].as_array_mut().unwrap().push(object(json!({"kind":{"const":"browser_operation"},"targetId":label,"operationId":{"type":"string","format":"uuid"},"request":{"type":"object"}}), &["kind","targetId","operationId","request"]));
    source["oneOf"].as_array_mut().unwrap().push(object(json!({"kind":{"const":"react_native"},"endpoint":{"type":"string","minLength":1,"maxLength":2048},"targetId":label,"appId":label,"pageId":label,"logicalDeviceId":label,"mode":{"enum":["discover","logs","cpu","heap_sampling"],"default":"discover"},"network":{"type":"boolean","default":false},"scripts":{"type":"boolean","default":false},"probe":{"type":"boolean","default":false},"allowDebuggerTakeover":{"type":"boolean","default":false},"allowProfilerControl":{"type":"boolean","default":false},"sampleMs":{"type":"integer","minimum":100,"maximum":10000,"default":1000}}),&["kind","endpoint","targetId","appId"]));
    source["oneOf"].as_array_mut().unwrap().push(object(json!({"kind":{"const":"dart"},"endpointFile":{"type":"string","minLength":1,"maxLength":4096},"targetId":label,"appId":label,"runtimeKind":{"enum":["flutter_native","flutter_desktop","dart_cli"]},"mode":{"enum":["discover","observe","cpu","memory","inspect"],"default":"discover"},"isolateId":label,"vmStartTime":{"type":"integer","minimum":0},"isolateStartTime":{"type":"integer","minimum":0},"sampleMs":{"type":"integer","minimum":100,"maximum":10000,"default":1000},"inspector":{"type":"boolean","default":false},"network":{"type":"boolean","default":false},"timeline":{"type":"boolean","default":false},"processMemory":{"type":"boolean","default":false},"includeVmOutput":{"type":"boolean","default":false},"allowUnauthenticated":{"type":"boolean","default":false},"classIds":{"type":"array","maxItems":4,"items":label},"objectIds":{"type":"array","maxItems":4,"items":label},"buildMode":{"enum":["debug","profile","release","unknown"]},"buildId":label,"symbolsBuildId":label}),&["kind","endpointFile","targetId","appId","runtimeKind"]));
    source["oneOf"].as_array_mut().unwrap().push(object(json!({"kind":{"const":"embedded_cdp"},"endpoint":{"type":"string","minLength":1,"maxLength":2048},"targetId":label,"appId":label,"mode":{"enum":["discover","observe"],"default":"discover"},"pageId":label,"expectedUrl":{"type":"string","minLength":1,"maxLength":2048},"allowDebuggerAttachment":{"type":"boolean","default":false},"network":{"type":"boolean","default":false},"sampleMs":{"type":"integer","minimum":100,"maximum":10000,"default":1000}}),&["kind","endpoint","targetId","appId"]));
    source["oneOf"].as_array_mut().unwrap().push(object(json!({"kind":{"const":"host_process"},"targetId":label,"serviceId":label,"hostId":label,"processId":{"type":"integer","minimum":1,"maximum":2147483647},"expectedGeneration":label}),&["kind","targetId","serviceId","hostId","processId"]));
    source["oneOf"].as_array_mut().unwrap().push(object(json!({"kind":{"const":"box_logs"},"targetId":label,"serviceId":label,"project":{"type":"string","minLength":1,"maxLength":4096},"workspace":label,"operationId":label,"stream":{"enum":["stdout","stderr"]}}),&["kind","targetId","serviceId","project","workspace","operationId","stream"]));
    source["oneOf"].as_array_mut().unwrap().push(object(json!({"kind":{"const":"observability_http"},"configFile":{"type":"string","minLength":1,"maxLength":4096},"sourceId":label,"scope":object(json!({"account":label,"project":label,"environment":label,"region":label,"resource":label,"release":label}),&["account","project","environment","region","resource","release"]),"window":object(json!({"from":label,"to":label}),&["from","to"]),"includeText":{"type":"boolean","default":false}}),&["kind","configFile","sourceId","scope","window"]));
    let limits = object(
        json!({"durationMs":{"type":"integer","minimum":1,"maximum":300000},"maxBytes":{"type":"integer","minimum":1,"maximum":8388608},"maxEvents":{"type":"integer","minimum":1,"maximum":65536}}),
        &[],
    );
    source["oneOf"].as_array_mut().unwrap().push(object(json!({"kind":{"const":"native_trace"},"backend":{"enum":["perfetto","simpleperf","xctrace","stack_sample"]},"mode":{"enum":["capture","import"]},"targetId":label,"appId":label,"processId":{"type":"integer","minimum":1},"path":{"type":"string","minLength":1,"maxLength":4096},"toolPath":{"type":"string","minLength":1,"maxLength":4096},"template":{"enum":super::native_trace::XCTRACE_TEMPLATES},"durationMs":{"type":"integer","minimum":100,"maximum":10000,"default":3000},"frequencyHz":{"type":"integer","minimum":1,"maximum":1000,"default":99},"allowSystemTrace":{"type":"boolean","default":false},"allowAttach":{"type":"boolean","default":false}}),&["kind","backend","mode","targetId","appId"]));
    let native = source["oneOf"].as_array_mut().unwrap().last_mut().unwrap();
    use super::native_trace::perfetto::{DEFAULT_BUFFER_KIB, MAX_BUFFER_KIB, MIN_BUFFER_KIB};
    native["properties"]["perfettoBufferKib"] = json!({
        "type":"integer","minimum":MIN_BUFFER_KIB,"maximum":MAX_BUFFER_KIB,"default":DEFAULT_BUFFER_KIB,
        "description":"Perfetto capture only. Central buffer size in KiB."
    });
    native["properties"]["perfettoFillPolicy"] = json!({
        "enum":["discard","ring_buffer"],"default":"discard",
        "description":"Perfetto capture only. Discard new chunks on overflow or overwrite the oldest chunks."
    });
    native["properties"]["waitForPerfettoStart"] = json!({
        "type":"boolean","default":false,
        "description":"Perfetto capture only. Use a device Perfetto with --notify-fd and wait for its start acknowledgement before replying."
    });
    native["properties"]["template"]["description"] = json!(
        "Activity Monitor selects an app PID and backboardd from a single physical iOS trace. Capture requires allowSystemTrace and does not attach to the app."
    );
    for name in ["allowAllocationRecording", "allowHeapInspection"] {
        native["properties"][name] = json!({"type":"boolean","default":false});
    }
    let select = object(json!({"investigationId":reference}), &["investigationId"]);
    let mut stop = select.clone();
    stop["properties"]["collectorId"] = reference.clone();
    let read = object(
        json!({"investigationId":reference,"collectorId":reference,"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":65536}}),
        &["investigationId", "collectorId"],
    );
    let anchor = object(
        json!({"source":{"type":"number"},"reference":{"type":"number"},"uncertainty":{"type":"number","minimum":0}}),
        &["source", "reference", "uncertainty"],
    );
    let clock = object(
        json!({"id":label,"unit":{"enum":["seconds","milliseconds","microseconds","nanoseconds"]},"referenceDomain":label,"maxDriftPpm":{"type":"number","minimum":0,"maximum":1000000},"anchors":{"type":"array","maxItems":32,"items":anchor}}),
        &["id", "unit"],
    );
    let symbol = object(
        json!({"kind":{"enum":["source-map","dsym","elf","mach-o","pdb","r8","proguard","dart"]},"evidenceId":reference,"sha256":{"type":"string","minLength":64,"maxLength":64},"buildId":label,"architecture":label}),
        &["kind", "evidenceId", "sha256", "buildId", "architecture"],
    );
    let mut metadata = object(
        json!({"clocks":{"type":"array","maxItems":32,"items":clock},"symbols":{"type":"array","maxItems":32,"items":symbol},"sampling":label,"heapId":label,"observationComplete":{"type":"boolean"},"sourceLoss":{"type":"array","maxItems":32,"items":label}}),
        &[],
    );
    metadata["properties"]["symbolicate"] = json!({"type":"boolean","default":false,"description":"Explicit retained Mach-O/DWARF UUID/architecture verification and bounded atos on macOS; no downloads or attach"});
    let analysis = object(
        json!({"investigationId":reference,"analysisId":reference}),
        &["investigationId", "analysisId"],
    );
    let projection = object(
        json!({"investigationId":reference,"analysisId":reference,"scope":{"enum":["app","all"],"default":"app"},"kind":{"enum":["event","failure","metric","stack","thread","allocation","retention","span","frame","action","marker","process","screenshot","video","audio"]}}),
        &["investigationId", "analysisId"],
    );
    let mut export = projection.clone();
    // An exported projection is one page of rows selected the same way.
    export["properties"]
        .as_object_mut()
        .expect("export properties")
        .extend(
            crate::pagination::properties(canonic_mcp_kit::page::Limits::STANDARD)
                .as_object()
                .expect("page properties")
                .clone(),
        );
    export["properties"]["path"] = json!({"type":"string","minLength":1,"maxLength":4096});
    export["required"] = json!(["investigationId", "analysisId", "path"]);
    export["properties"]["format"] = json!({"enum":["minimal-v1","native_artifact","investigation-json","investigation-markdown"],"default":"minimal-v1","description":"Report formats require scope all and report options; minimal privacy omits text/identity, explicit private retains them; native_artifact is unredacted"});
    export["properties"]["report"] = super::analysis::investigation::schema(true);
    let mut definitions: Vec<Value> = [
        ("diagnostics_open","Open a retained investigation with caller-declared app/build/QA references; reuse submissionId after a lost reply",object(json!({"submissionId":submission,"question":{"type":"string","minLength":1,"maxLength":1024},"binding":binding}),&["submissionId","question"]),false),
        ("diagnostics_start","Collect an explicitly selected diagnostic source or retain an action marker; reuse submissionId after a lost reply",object(json!({"investigationId":reference,"submissionId":submission,"source":source,"limits":limits}),&["investigationId","submissionId","source"]),false),
        ("diagnostics_status","Inspect retained diagnostic state and recovery without connecting a target or starting a manager",select.clone(),true),
        ("diagnostics_read","Read bounded private collector bytes as base64; output can contain application secrets",read,true),
        ("diagnostics_stop","Stop selected or all collectors and preserve their output; inspect diagnostics_status until settled",stop,false),
        ("diagnostics_close","Close an investigation to new collectors and stop its existing collectors, preserving evidence",select.clone(),false),
        ("diagnostics_remove","Remove only a closed, settled investigation's owned records and evidence, preserving source files and commands",select,false),
        ("diagnostics_analyze","Prepare a retained bounded index in an isolated parser process; reuse submissionId after a lost reply",object(json!({"investigationId":reference,"collectorId":reference,"submissionId":submission,"metadata":metadata}),&["investigationId","collectorId","submissionId"]),false),
        ("diagnostics_analysis_status","Inspect parser outcome and retained evidence provenance without recollecting or starting a manager",analysis.clone(),true),
        ("diagnostics_analysis_cancel","Cancel an owned parser and preserve raw evidence; inspect analysis status until settled",analysis.clone(),false),
        ("diagnostics_query","Read bounded private native or composed evidence and witnessed reports from explicitly selected retained analyses",object(json!({"investigationId":reference,"analysisId":reference,"kind":{"enum":["event","failure","metric","stack","thread","allocation","retention","span","frame","action","marker","process","screenshot","video","audio"]},"clockDomain":label,"from":{"type":"number"},"to":{"type":"number"},"correlationId":label,"offset":{"type":"integer","minimum":0,"maximum":20000},"limit":{"type":"integer","minimum":1,"maximum":200}}),&["investigationId","analysisId"]),true),
        ("diagnostics_compare","Compare compatible retained observations or explain unknown/mismatched identities, clocks and measurement definitions",object(json!({"investigationId":reference,"analysisId":reference,"otherInvestigationId":reference,"otherAnalysisId":reference}),&["investigationId","analysisId","otherInvestigationId","otherAnalysisId"]),true),
        ("diagnostics_capabilities","Inspect registered diagnostic effects, scope, cost and bundled dependencies without reading projects, spawning helpers or downloading",object(json!({}),&[]),true),
        ("diagnostics_plan","Plan bounded reproduction collectors and existing fault-control owners without executing actions or probing readiness",object(json!({"preset":{"enum":["crash","hang","memory_growth","slow_interaction","startup","network","background_media","soak"]},"platform":{"enum":["android","ios","browser","import"]},"targetId":label,"appId":label,"userId":{"type":"integer","minimum":0,"maximum":99999},"durationMs":{"type":"integer","minimum":1000,"maximum":300000,"default":30000},"intervalMs":{"type":"integer","minimum":500,"maximum":60000,"default":1000},"fault":{"enum":["none","offline","delay","response","lifecycle","permission","pressure"],"default":"none"}}),&["preset","platform"]),true),
        ("diagnostics_prepare","Verify the bundled parser through a bounded same-executable round trip without installing dependencies or changing app/device state",object(json!({"component":label,"version":label}),&["component"]),false),
        ("diagnostics_project","Read a bounded numeric projection that omits free text and unselected app scope; numeric/timing data can remain sensitive",projection,true),
        ("diagnostics_export","Write explicit minimal/private JSON or Markdown reports, projections or native artifacts without uploading or replacing different bytes",export,false),
    ].into_iter().map(|(name,description,schema,read)|json!({"name":name,"description":description,"inputSchema":schema,"annotations":{"readOnlyHint":read,"destructiveHint":!read,"idempotentHint":read,"openWorldHint":false}})).collect();
    // Derived reads can expose secrets just as explicit raw evidence reads can.
    for definition in &mut definitions {
        let name = definition["name"].as_str().unwrap().to_owned();
        definition["_meta"] = json!({"testrun/access":super::access::effects(&name)});
        if matches!(name.as_str(), "diagnostics_prepare" | "diagnostics_export") {
            definition["annotations"]["destructiveHint"] = json!(false);
            definition["annotations"]["idempotentHint"] = json!(true);
        }
        if definition["name"] == "diagnostics_analyze" {
            definition["annotations"]["idempotentHint"] = json!(true);
        }
    }
    if let Some(query) = definitions
        .iter_mut()
        .find(|t| t["name"] == "diagnostics_query")
    {
        query["inputSchema"]["properties"]["view"] = json!({"enum":["rows","timeline","call_tree","flamegraph","memory","performance","network_data","scenario","assertions","observability","investigation","report"],"default":"rows"});
        query["inputSchema"]["properties"]["investigation"] =
            super::analysis::investigation::schema(false);
        query["inputSchema"]["properties"]["report"] = super::analysis::investigation::schema(true);
        query["inputSchema"]["properties"]["offset"]["maximum"] = json!(80000);
        query["inputSchema"]["properties"]["assertions"] = super::analysis::assertions::schema();
        query["inputSchema"]["properties"]["scenario"] = object(
            json!({"operation":{"enum":["summary","timeline","lifecycle","media","peripherals","energy","settings","restore"]},"clockDomain":label,"actionId":label}),
            &["operation"],
        );
        query["inputSchema"]["properties"]["networkData"] = object(
            json!({"operation":{"enum":["requests","phases","messages","spans","logs","packets","correlate","data","schema","query_plan"]},"id":label,"identityScope":label,"traceId":{"type":"string","minLength":32,"maxLength":32},"requestId":label,"collection":label,"key":label,"equals":object(json!({"field":label,"value":{"type":["string","number","boolean","null"],"maxLength":4096}}),&["field","value"])}),
            &["operation"],
        );
        query["inputSchema"]["properties"]["performance"] = object(
            json!({"operation":{"enum":["summary","startup","frames","stages","interaction","trees","accessibility","gpu","sizes"]},"minTargetSize":{"type":"number","exclusiveMinimum":0,"maximum":1024,"description":"Optional accessibility target-size candidate threshold in producer logical units; not a platform conformance rule"}}),
            &["operation"],
        );
        query["inputSchema"]["properties"]["memory"] = object(
            json!({"operation":{"enum":["summary","classes","objects","object","edges","retainers","paths","dominators","series"]},"objectId":label,"className":{"type":"string","minLength":1,"maxLength":512},"maxDepth":{"type":"integer","minimum":1,"maximum":64}}),
            &["operation"],
        );
        for field in ["nativeUtid", "nativeUpid"] {
            query["inputSchema"]["properties"][field] = json!({"type":"integer","minimum":0,"description":"Perfetto trace-lifetime ID; process selection includes recorded threads without joining reused OS PIDs"});
        }
    }
    for tool in &mut definitions {
        match tool["name"].as_str().unwrap_or_default() {
            "diagnostics_read" => crate::pagination::declare(tool, crate::pagination::BYTES),
            // The CLI validates against these definitions, so paging is declared
            // here and not only in the MCP catalog.
            "diagnostics_project" => {
                crate::pagination::declare(tool, canonic_mcp_kit::page::Limits::STANDARD);
            }
            "diagnostics_query" => {
                crate::pagination::declare(tool, canonic_mcp_kit::page::Limits::STANDARD);
                tool["inputSchema"]["properties"]["collection"] = json!({"const":"measurement_groups","description":"Page performance measurement groups independently from row records"});
            }
            _ => {}
        }
    }
    definitions
}

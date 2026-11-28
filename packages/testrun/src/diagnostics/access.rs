//! Effects and bundled dependency discovery. No target or provider preparation.
use super::analysis::{model::PARSER, worker};
use crate::error::{Error, Result};
use serde_json::{Value, json};
use tokio::sync::watch;

pub(crate) fn local(name: &str) -> bool {
    matches!(
        name,
        "diagnostics_capabilities" | "diagnostics_prepare" | "diagnostics_plan"
    )
}
pub(crate) fn effects(name: &str) -> Value {
    let effects: &[&str] = match name {
        "diagnostics_capabilities" | "diagnostics_plan" => &["read_builtin_metadata"],
        "diagnostics_prepare" => &["spawn_owned_parser", "read_builtin_metadata"],
        "diagnostics_open" => &["write_private_state"],
        "diagnostics_start" => &[
            "read_selected_source",
            "write_private_evidence",
            "spawn_owned_adb_clients",
            "read_selected_android_package_and_filtered_system_context",
            "may_start_adb_server",
            "optional_connect_selected_apple_diagnostic_services",
            "optional_read_selected_exported_reports",
            "optional_copy_settled_browser_operation_without_native_dispatch",
            "optional_loopback_metro_discovery_and_inspector_attachment",
            "optional_fixed_app_probe_execution",
            "optional_runtime_global_Hermes_profiler_control",
            "optional_explicit_legacy_debugger_takeover",
            "optional_read_private_Dart_VM_Service_URI_file_and_attach_loopback",
            "optional_allocate_and_dispose_Flutter_inspector_object_group",
            "optional_read_live_Dart_instances_retaining_paths_and_VM_wide_timeline",
            "optional_finite_native_system_trace_or_CPU_recording",
            "explicit_native_profiler_attach_and_trusted_offline_converter_execution",
            "explicit_Apple_allocation_recording_or_heap_inspection",
            "optional_read_private_provider_config_and_selected_bearer_file",
            "optional_configured_provider_GET_with_possible_backend_query_cost",
        ],
        "diagnostics_status" | "diagnostics_analysis_status" => {
            &["read_private_metadata", "reconcile_owned_state"]
        }
        "diagnostics_read" | "diagnostics_query" | "diagnostics_compare" => {
            &["read_private_content"]
        }
        "diagnostics_project" => &[
            "read_private_content",
            "omit_unselected_scope",
            "omit_free_text",
        ],
        "diagnostics_export" => &[
            "read_private_content",
            "write_explicit_report_or_unredacted_native_artifact",
        ],
        "diagnostics_analyze" => &[
            "read_private_content",
            "spawn_owned_parser",
            "write_private_index",
            "optional_uuid_verified_apple_symbol_tools",
            "optional_private_temporary_symbol_files",
        ],
        "diagnostics_stop" | "diagnostics_close" | "diagnostics_analysis_cancel" => {
            &["stop_owned_work", "write_private_state"]
        }
        "diagnostics_remove" => &["delete_owned_evidence"],
        _ => &[],
    };
    json!({"effects":effects,"appDataChanges":false,"deviceSettingChanges":false,"attach":name=="diagnostics_start","pause":false,"gc":false,"injection":name=="diagnostics_start","install":false,"remoteTransmission":name=="diagnostics_start","remoteTransmissionMeaning":"possible configured provider selectors/authorization; captured evidence is not uploaded automatically"})
}
pub(crate) fn capabilities() -> Value {
    let operations: Vec<_> = super::surface::NAMES
        .iter()
        .map(|name| json!({"operation":name,"access":effects(name)}))
        .collect();
    let mut value = json!({"version":1,"host":{"os":std::env::consts::OS,"architecture":std::env::consts::ARCH},"operations":operations,
        "sources":[
            {"kind":"import","registration":"implemented","availability":"unknown","prerequisite":"Select a readable regular file of at most 8 MiB","scope":"explicit_file; app isolation unknown","effects":["read_selected_source","write_private_evidence"],"cost":{"class":"lightweight_host_io","appInstrumentation":"none","work":"bounded read/copy, durable flushes and SHA-256","appOverhead":"not_measured"}},
            {"kind":"build_output","registration":"implemented","availability":"unknown","prerequisite":"Select an existing operation in this workspace","scope":"selected retained operation; app isolation unknown","effects":["read_selected_source","write_private_evidence"],"cost":{"class":"lightweight_host_io","appInstrumentation":"none","work":"bounded log polling/copy, durable flushes and SHA-256","appOverhead":"not_measured"}},
            {"kind":"android","registration":"implemented","availability":"unknown","prerequisite":"Preinstalled adb; authorized explicitly selected transport; Android API 26+; exact installed package and Android user","scope":"bound target/app; installed UID; filtered system logs; package/service/optional report reads can include related users/processes","effects":["spawn_owned_adb_clients","may_start_adb_server","read_package_process_logs_and_exit_history","optional_read_observed_report_paths","write_private_evidence"],"cost":{"class":"bounded_native_sampling","appInstrumentation":"none","work":"read-only ADB snapshots; up to 256 cycles, 32 processes, 8 libraries and 4 referenced reports; 4s maximum per command","appOverhead":"not_measured"},"sampling":"snapshot or follow; 500ms minimum between cycles plus command time; circular buffer loss unknown; never clears logs","restrictions":"no root, installs, attach, app launch, settings changes or bugreport generation; inaccessible evidence stays unavailable"}
        ],
        "analysis":{"registration":"implemented","availability":"unknown","prerequisite":"Select finalized evidence with a supported format; optional diagnostics_prepare verifies the bundled parser","formats":["testrun-events-v1","chrome-trace","har","speedscope","android-diagnostics-v1","android-tombstone","android-anr","android-bugreport-text"],"cost":{"class":"deep_host_analysis","appInstrumentation":"none","work":"bounded decompression/parse/model construction in a child; raw/index integrity reads","appOverhead":"not_measured"},"sampling":"producer_defined; imports do not configure a sampler"},
        "dependencies":[{"component":"builtin-parser","version":PARSER,"packaging":"same_executable","availability":"bundled_unprobed","downloads":false,"prepare":{"operation":"diagnostics_prepare","arguments":{"component":"builtin-parser"}},"verification":"explicit bounded same-executable round trip; no dependency process starts during discovery"}],
        "nativeCollectors":{"android":{"registration":"implemented","availability":"unknown","adapter":"android-adb/1","dependency":"externally installed adb; no automatic preparation","historicalExits":"API 30+; permission and OEM dependent","formats":["android-diagnostics-v1","android-tombstone","android-anr","android-bugreport-text"]},"otherPlatforms":{"availability":"unregistered","preparation":null}},
        "privacy":{"raw":"private; may contain secrets","projection":"minimal-v1; free text and unrelated/unknown app scope omitted; numeric data can remain sensitive","rawExport":false,"reportExport":"explicit minimal-v1 JSON; no implicit upload"},
        "retention":{"investigationsPerWorkspace":256,"collectorsPerInvestigation":16,"analysesPerInvestigation":16,"expiry":"explicit removal; no automatic deletion","exports":"caller_owned; investigation removal preserves external reports"},
        "disabled":{"collectorProcessesStarted":0,"targetInstrumentation":"none; capability discovery does not begin collection or stop existing work"},
        "recovery":{"unsupportedDependency":"Select a registered component; do not install tools or change app/device security as fallback"}});
    mobile_capabilities(&mut value);
    value["sources"].as_array_mut().unwrap().push(json!({"kind":"browser_operation","registration":"implemented","availability":"unknown","format":"browser-operation-v1","scope":"exact connected browser and settled operation; original typed request fingerprint verified","prerequisite":"Investigation bound to targetId; live owner retaining operation; original browser_send request","effects":["read_one_retained_browser_result","write_private_evidence"],"cost":{"class":"bounded_host_copy","work":"selected result and request only; no unrelated operation history or file contents","appOverhead":"no new browser instrumentation"},"ownership":"borrowed result copy; never cancels original work or pins native files/models","limits":{"bytes":8388608,"chunkBytes":16384},"historyComplete":false}));
    value["analysis"]["formats"]
        .as_array_mut()
        .unwrap()
        .push(json!("browser-operation-v1"));
    value["browser"] = crate::browser::runtime::diagnostic_capabilities();
    runtime_capabilities(&mut value);
    analysis_capabilities(&mut value);
    service_capabilities(&mut value);
    value["workflows"] = super::guidance::routes();
    value["verification"] = serde_json::from_str(include_str!("verification.json"))
        .expect("Bundled diagnostics verification inventory");
    value["nativeCollectors"]["otherPlatforms"] = json!({"selection":"inspect sources, frameworks, services and observability for each registered route; a platform/framework name alone grants no transport","availability":"source_specific","preparation":null});
    value["privacy"]["reportExport"] = json!(
        "minimal-v1 numeric page or minimal investigation JSON/Markdown; explicit private reports include free text/identity; no implicit upload"
    );
    value
}

fn mobile_capabilities(value: &mut Value) {
    value["sources"].as_array_mut().unwrap().push(json!({"kind":"android_state","registration":"implemented","availability":"unknown","adapter":"android-state-adb/1","prerequisite":"Same selected transport/app/user and API 26+ as Android logs; proc/dumpsys permissions vary; standby/appops API 28+","groups":["process","memory","scheduling","power","frames","connectivity"],"defaultGroups":["process","memory","power"],"effects":["spawn_owned_adb_clients","may_start_adb_server","read_process_package_and_selected_system_context","write_private_evidence"],"scope":"bound app processes plus explicitly selected device context; scheduling dumps can read multiple users before filtering","sampling":{"intervalMs":1000,"minimumIntervalMs":500,"meaning":"delay after work; sequential snapshots, not an exact sample frequency","maxCycles":256,"maxProcesses":8,"defaultMaxProcesses":4,"commandDeadlineMs":4000},"cost":{"class":"bounded_native_state_sampling","appInstrumentation":"none","appOverhead":"not_measured","work":"proc reads, optional dumpsys/getconf and existing network readback; no attach, GC, buffer reset or settings changes"},"unavailable":["live_heap_ownership","allocator_retention","forced_gc","OEM_policy_inference"],"integrations":{"network":"existing network readback and parser; never network_set","media":{"operation":"media_status","targetSelection":"explicit targetId through existing connected playback owner","automaticPreparation":false},"volume":"volume_get through existing owner","audio":"audio_probe remains separately scoped and may change routing; never run by state collection"}}));
    value["sources"].as_array_mut().unwrap().push(json!({"kind":"ios","registration":"implemented","availability":"unknown","adapter":"apple-diagnostics/1","routes":{"simulator":"macOS/Xcode simctl for exact booted UUID/device set; selected simulator log store, installed app plist/signature and birth-checked host process counters","physical":"macOS/CoreDevice inventory plus preinstalled idevicesyslog for explicit trusted UDID and USB/network; never host Unified Logging"},"effects":["read_selected_installed_metadata","optional_connect_device_log_service","read_explicit_host_report_directory","spawn_owned_bounded_clients","write_private_evidence"],"scope":"app processes plus exact bundle-related OS process-name context; unknown birth/device attribution stays unknown; selected reports can be broad","limits":{"commandMs":4000,"logBytes":524288,"reportEntries":256,"reportFiles":4,"reportFileBytes":8388608,"processes":16},"cost":{"appOverhead":"not_measured","sampling":"one bounded sequence; no complete history or native-version certification"},"restrictions":"no boot, launch, unlock, pair, developer support preparation, install, privacy change, crash mover/deletion, sysdiagnose generation or upload","gaps":["physical_entitlements_background_modes_and_resource_profiler_unavailable","native_device_report_transfer_unregistered; import exported reports","simulator_boot_generation_unknown; kernel process births when observed","unknown_service_backlog_redaction_and_loss"],"formats":["apple-diagnostics-v1","apple-ips","apple-crash-text","apple-jetsam","apple-metrickit","apple-sysdiagnose-text"]}));
    value["analysis"]["formats"].as_array_mut().unwrap().extend(
        [
            "apple-diagnostics-v1",
            "apple-ips",
            "apple-crash-text",
            "apple-jetsam",
            "apple-metrickit",
            "apple-sysdiagnose-text",
        ]
        .map(|s| json!(s)),
    );
    value["analysis"]["appleSymbolication"] = json!({"registration":"implemented","availability":"unknown","selection":"diagnostics_analyze metadata.symbolicate:true with 1..8 retained dsym/mach-o files","verification":"retained SHA-256 plus artifact-internal dwarfdump UUID and exact architecture before atos; no source checkout or symbol download","limits":{"frames":256,"wallMs":10000,"commandMs":3000},"effects":["read_retained_symbols","private_temporary_copy","spawn_xcrun_dwarfdump_and_atos"],"unresolved":"missing/mismatched artifact, image identity, address, architecture or tool remains explicit"});
    value["nativeCollectors"]["apple"] = json!({"registration":"implemented","availability":"unknown","adapter":"apple-diagnostics/1","routes":["selected_simulator","selected_physical_USB","selected_physical_network","explicit_exported_report_import"],"preparation":null});
}

fn runtime_capabilities(value: &mut Value) {
    value["sources"].as_array_mut().unwrap().push(json!({"kind":"react_native","registration":"implemented","availability":"unknown","adapter":"react-native-inspector/1","modes":["discover","logs","cpu","heap_sampling"],"prerequisite":"explicit numeric loopback HTTP endpoint/port, native target and proxy app ID; capture additionally exact discovered pageId/logicalDeviceId","scope":"producer-reported logical app/runtime; physical device and installed build association unverified; caller selects Expo Go/development client/bare identity","effects":["loopback_HTTP_discovery","optional_WebSocket_attach","optional_enable_log_network_debugger_domains","optional_fixed_JS_probe","explicit_runtime_global_profiler_control","explicit_legacy_debugger_takeover"],"sampling":{"defaultMs":1000,"minimumMs":100,"maximumMs":10000,"heapIntervalBytes":32768,"heapStackDepth":32},"limits":{"discoveryBytes":1048576,"pages":64,"commandMs":3000,"cleanupCommandMs":2000,"eventPacketEntries":32,"eventEntryBytes":32768,"scriptsTracked":256},"cost":{"class":"bounded_runtime_instrumentation","appOverhead":"not_measured; logs/debugger/network/probe/profilers can perturb development execution"},"cleanup":"normal settlement attempts owned profiler stop once, closes socket and releases lease; runtime changes/hard manager loss can leave profiler state unknown","restrictions":"no automatic launch, forwarding, reload, stepping, pause/resume, GC, probe install, source-map download or arbitrary CDP","gaps":["silent proxy rebinding cannot be authenticated","source maps retained as references only","native network/modules/images may be absent","React commits require explicit optional app-owned Profiler callback","allocation sampling is not a retaining graph","live RN/Expo version matrix unverified"]}));
    value["analysis"]["formats"]
        .as_array_mut()
        .unwrap()
        .push(json!("react-native-diagnostics-v1"));
    value["reactNativeProbe"] = json!({"schemaVersion":1,"format":"commonjs","installation":"explicit app-owned integration only; capabilities never installs or executes this template","source":include_str!("../../probes/react-native.js")});
    value["sources"].as_array_mut().unwrap().push(json!({"kind":"dart","registration":"implemented","availability":"unknown","adapter":"dart-vm-service/1","modes":["discover","observe","cpu","memory","inspect"],"runtimeKinds":["flutter_native","flutter_desktop","dart_cli"],"selection":"explicit private endpointFile containing numeric loopback ws://host:port/TOKEN/ws; exact target/app binding; capture requires discovered isolateId, vmStartTime and isolateStartTime","scope":"producer VM/isolate births plus caller native/desktop/CLI association; native device/build identity unverified","effects":["read_private_service_credential_file","attach_VM_Service_socket","subscribe_socket_owned_events","read_existing_CPU_timeline_memory_and_network_data","optional_live_object_and_retaining_path_queries","optional_Flutter_inspector_group_allocation_and_disposal"],"restrictions":"fixed read RPCs only; no evaluate/invoke/resume/pause/reload/kill/forced GC/reset/global profiler flags/HTTP logging enablement/install/download; Flutter web uses browser evidence","bounds":{"requestMs":3000,"cleanupRequestMs":500,"socketCloseMs":1000,"messageBytes":8388608,"isolateInventory":64,"packetEvents":32,"eventBytes":32768,"objectIds":4,"classIds":4,"instances":16,"retainingPath":32,"sampleMs":[100,1000,10000]},"memory":"Dart heap/capacity, embedder external bytes, class statistics and bounded live retaining paths; full binary heap/dominator decoder unregistered","cost":{"appOverhead":"not_measured","class":"bounded VM service reads; inspector/object/path queries can pause or perturb target execution"},"gaps":["live SDK/build/platform range unverified","service credentials required; no forwarding or authentication disablement","whole VM output/timeline/process memory optional and not app-isolated","native plugin network/GPU/native-failure evidence remains separate","object IDs expire and do not match across snapshots"]}));
    value["analysis"]["formats"]
        .as_array_mut()
        .unwrap()
        .extend([json!("dart-diagnostics-v1"), json!("flutter-app-size-v1")]);
    value["dartProbe"] = json!({"schemaVersion":1,"format":"dart","installation":"optional app-owned bounded postEvent template; reading it does not register, install or execute app code; test it with the app SDK","source":include_str!("../../probes/dart.dart")});
    let dart = value["sources"].as_array_mut().unwrap().last_mut().unwrap();
    dart["bounds"]["automaticInstanceObjects"] = json!(4);
    dart["bounds"]["objectResultBytes"] = json!(32768);
    dart["ownership"] = json!(
        "one Testrun capture per loopback service port; external debugger ownership is not inferred"
    );
    dart["heapScope"] = json!(
        "VM isolateGroup heap; class/instance/path observations can include sibling isolates; not per-isolate/app memory isolation"
    );
}

fn native_trace_capabilities() -> Value {
    json!({
        "kind":"native_trace","adapter":"native-trace/1","registration":"implemented","availability":"unknown",
        "backends":["perfetto","simpleperf","xctrace","stack_sample"],"modes":["capture","import"],
        "effects":["spawn_explicit_trusted_native_tools","private_native_artifact_reads_and_writes","capture_can_start_ADB_and_enable_finite_system_tracing_or_CPU_sampling","explicit_xctrace_or_sample_attach_can_perturb_execution","owned_temporary_file_cleanup","explicit_unredacted_native_artifact_export"],
        "prerequisites":"explicit target/app and native tools; Perfetto and xctrace Activity Monitor system capture require allowSystemTrace; xctrace PID attach and sample require allowAttach; no install, launch, root, settings changes or automatic downloads",
        "bounds":{"durationMs":[100,10000],"frequencyHz":[1,1000],"artifactBytes":4194304,"bundleEntries":512,"sectionBytes":1048576,"rows":20000},
        "scope":"Perfetto is system/user data as recorded; simpleperf is selected primary-user UID/PID; xctrace PID attach uses caller-selected target/PID; sample is selected macOS host PID",
        "perfetto":{
            "bufferKib":[super::native_trace::perfetto::MIN_BUFFER_KIB,super::native_trace::perfetto::MAX_BUFFER_KIB],
            "defaultBufferKib":super::native_trace::perfetto::DEFAULT_BUFFER_KIB,
            "fillPolicies":["discard","ring_buffer"],
            "startAcknowledgement":"waitForPerfettoStart requires device Perfetto --notify-fd support"
        },
        "iosCpu":{
            "template":super::native_trace::ACTIVITY_MONITOR,"target":"physical iOS device:UDID","processId":"selected app PID",
            "captureScope":"all processes; original trace can contain neighboring process data",
            "queryScope":"app PID and backboardd CPU percent metrics on one trace-relative native:xctrace clock",
            "identity":"app/PID association is caller-declared; backboardd is shared system work",
            "traceArchive":"up to 16 MiB of tar input; gzip used when needed to fit 4 MiB retention",
            "interruption":"Instruments Device disconnected ends collection as partial with retained trace and source-reported duration"
        },
        "analysis":{"format":"native-trace-v1","queries":["rows","timeline","call_tree","flamegraph"],"export":"native_artifact requires explicit scope all; xctrace bundle exported as tar or tar.gz, never extracted","symbols":"producer labels/build references remain unresolved without independent native artifact identity"},
        "cleanup":"owned host children reaped, temporary host files removed; finite remote recorder can outlive cancelled native client, remote termination unverified; no global profiler stop",
        "cost":{"class":"deep_native_capture_and_offline_conversion","appOverhead":"not_measured; attach/sampling/tracing can perturb execution; bundle cap applies after native tool finishes writing"},
        "gaps":["live per-template device/simulator acceptance unverified","no off-CPU inference from samples","unavailable tables/loss and finite limits retained explicitly","no SDK/tool downloads or symbol servers"]
    })
}

fn analysis_capabilities(value: &mut Value) {
    value["sources"]
        .as_array_mut()
        .unwrap()
        .push(native_trace_capabilities());
    value["analysis"]["formats"]
        .as_array_mut()
        .unwrap()
        .push(json!("native-trace-v1"));
    value["analysis"]["formats"]
        .as_array_mut()
        .unwrap()
        .extend([
            json!("java-hprof"),
            json!("memory-graph-v1"),
            json!("memory-series-v1"),
        ]);
    value["memory"] = json!({"registration":"implemented","readiness":"source_specific_unknown","routes":{"browser":"existing heap_capture/load/query/compare/release through browser operations and retained browser_operation; native V8 model/identity epoch stays browser-owned","hermes":"react_native heap_sampling with explicit allowProfilerControl; allocation estimates, not a retaining graph","dart":"dart memory/inspect reads: classes, live instances/retaining/inbound paths; VM isolateGroup heap, temporary IDs, no forced GC","jvmArt":"java-hprof import, strict bounded 1.0.1/1.0.2/1.0.3 reader, modeled strong-reference graph and Activity field observations; no Shark/LeakCanary heuristics or automatic dump","apple":"native_trace xctrace Allocations/Leaks; separate recording/inspection opt-ins, only advertised registered memory tables, raw .trace retained; Xcode GUI memory graph is not decoded","native":"producer_native graph imports, existing Android process/resource snapshots and app-owned memory-series-v1 for resource lifecycle counters"},"query":{"view":"memory","operations":["summary","classes","objects","object","retainers","paths","dominators","series"],"bounds":{"objects":10000,"edges":50000,"edgesPerObject":512,"pathDepth":64,"dominatorWork":4000000}},"effects":{"graphImport":"private offline parse; no runtime attach/GC","heapCapture":"separate explicit browser operation or externally captured JVM/ART/Dart/native artifact; may pause/perturb runtime and include secrets","allocationRecording":"explicit runtime/Apple opt-in; sampling and instrumentation affect allocation behavior","forcedGC":"unregistered by memory workflow; capture/runtime internal GC behavior not controlled","pressureOrTrim":"unregistered; no callbacks/settings changes or interventions performed"},"interpretation":"specific leak attribution requires supported retention/ownership evidence plus expected release/lifetime proof; growth, callback delivery or persistent RSS alone never proves a leak","comparison":"class aggregates only under matching declared runtime/process/build/sampling and size definitions; object IDs are snapshot-local","series":"warmup, baseline, action/release/cleanup observations with finalized digest-checked probe references; not an action runner or automated intervention","gaps":["large production HPROF files can exceed shared 8 MiB budget","Dart/Hermes binary heap graphs, Apple memgraph and native malloc/core parsing unregistered","live per-runtime/build/device validation and target overhead unverified"]});
    value["memory"]["query"]["operations"]
        .as_array_mut()
        .unwrap()
        .push(json!("edges"));
    let bounds = value["memory"]["query"]["bounds"].as_object_mut().unwrap();
    bounds.remove("edgesPerObject");
    bounds.insert("edgeChunkEntries".into(), json!(32));
    bounds.insert("objectEdgePreview".into(), json!(16));
    value["analysis"]["formats"]
        .as_array_mut()
        .unwrap()
        .extend([json!("ui-performance-v1"), json!("package-size-v1")]);
    value["performance"] = json!({"registration":"implemented","readiness":"source_specific_unknown","query":{"view":"performance","operations":["summary","startup","frames","stages","interaction","trees","accessibility","gpu","sizes"],"bounds":{"records":4096,"treeNodes":4096,"packageEntries":10000,"page":200}},"routes":{"startup":"ui-performance-v1 producer milestones with cold/warm/hot state evidence, same-clock intervals and uncertainty; no automatic launch/cache reset","frames":"existing Android state frames, Flutter.Frame build/raster/elapsed and imported explicit frame deadlines/presentation; no universal 16ms budget or inferred FPS/dropped count","stages":"existing native/browser/Flutter complete spans and imported named thread intervals; overlapping stages not additive","trees":"existing platform UI/accessibility, Flutter inspector, browser React inspection; ui-performance-v1 imports keep view/accessibility/component identities separate","gpu":"imported explicit producer measurements with tool/device/restrictions; Metal/Vulkan/OpenGL capture automation and per-draw analysis unavailable","evidence":"existing screenshot/video/action evidence; imported interaction references remain producer-declared, unverified links","sizes":"package-size-v1 flat disjoint byte inventory with explicit size basis; existing flutter-app-size-v1 hierarchical model remains separate"},"effects":["private_offline_parse_and_query","no_automatic_input_launch_attach_or_instrumentation"],"gaps":["native Compose/SwiftUI runtime component inspectors unregistered","GPU capture automation and physical device restrictions unverified","live startup/frame/display/screen-reader matrix and target overhead unverified","package bytes alone do not establish startup impact"]});
    value["analysis"]["formats"]
        .as_array_mut()
        .unwrap()
        .extend([
            json!("network-evidence-v1"),
            json!("data-snapshot-v1"),
            json!("pcap"),
        ]);
    value["networkData"] = json!({"registration":"implemented","readiness":"source_specific_unknown","query":{"view":"network_data","operations":["requests","phases","messages","spans","logs","packets","correlate","data","schema","query_plan"]},"routes":{"network":"HAR phases, existing RN CDP lifecycle, browser request/HTTP/HAR/WebSocket owners, Dart HTTP/timeline observations and explicit network-evidence-v1 for streaming/gRPC/service spans/logs","proxy":"existing explicitly selected browser launch/context/HTTP proxy owners; private HAR/evidence capture; close owned context/client for release; no OS/mobile route/trust changes","packets":"bounded classic PCAP Ethernet/raw IPv4/IPv6 header import; no live interface capture, pcapng, reassembly, decryption or app attribution","localData":"existing browser storage export/IndexedDB/cache interfaces plus explicit scalar snapshots for preferences/files/SQLite/Room/Core Data; no live DB open or sandbox bypass","remoteData":"named read-only indexed query exports through existing authenticated HTTP access and selected settled http_evidence_read JSON result or explicit import; no new credential owner or database shell","differences":"matching declared dataset/scope/account/project and target/app; authoritative/derived/cache/index roles and consistency remain producer claims; no repair/flush"},"bounds":{"records":4096,"dataRecords":2000,"collections":32,"indexesPerCollection":4,"packetRecords":4096,"remotePlanRows":2000,"remotePlanScanned":1000000,"remotePlanTimeoutMs":10000,"pageBytes":262144},"effects":["private_offline_parse_and_query","existing_access_capture_and_authentication_owners_remain_explicit","no_automatic_network_replay_SQL_execution_trust_change_or_repair"],"gaps":["remote read-only/plan/cost controls must be enforced by configured access owner before dispatch; imported claims are not authenticated","native mobile proxy routing, CA provisioning, pinning bypass and live packet capture unregistered","live provider/database/app/version/transport range and target overhead unverified"]});
    value["analysis"]["formats"]
        .as_array_mut()
        .unwrap()
        .push(json!("scenario-context-v1"));
    let android = value["sources"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|s| s["kind"] == "android_state")
        .unwrap();
    android["groups"]
        .as_array_mut()
        .unwrap()
        .extend([json!("media"), json!("device_context")]);
    android["contextReads"] = json!({"media":"fixed dumpsys audio/media_session with exact app plus selected user/UID line witnesses; no route change or helper/probe startup","device_context":"device locale/timezone properties and selected-user font_scale/user_rotation/accelerometer_rotation settings get only; no settings put, permission grant or radio operation"});
    value["scenarioContext"] = json!({"registration":"implemented","readiness":"source_specific_unknown","query":{"view":"scenario","operations":["summary","timeline","lifecycle","media","peripherals","energy","settings","restore"]},"routes":{"android":"existing jobs/services/alarms/wakelock/power/thermal/connectivity plus optional media/device_context reads; command/OEM/permission gaps remain explicit","apple":"existing native logs/termination/MetricKit; app-owned AVAudioSession/lifecycle/peripheral markers imported explicitly; no native media-state source inferred","media":"existing media_status/volume and explicit audio_probe keep target/controller/route-change/cleanup contracts; imports keep session/item/position origin separate from audio output and process exit","peripherals":"explicit app/platform video/camera/Bluetooth/location/sensor markers with hardware/effect declarations; no active probe or external-device input","energy":"native device battery/thermal and source-defined power/energy; process attribution requires declared app/process/evidence, never inferred from device charge/percent","settings":"before/applied/current ledger and restore outcome/candidate classification; only existing setting owner can execute a fresh authorized restore"},"effects":["private_offline_parse_and_query","optional_fixed_ADB_readback","no_automatic_active_probe_or_setting_restore"],"bounds":{"records":4096,"page":200,"pageBytes":262144},"gaps":["live hardware/OS/background/peripheral/route/energy acceptance and overhead unverified","locale/timezone/appearance/text size/rotation/permission setters and automatic restoration unregistered by diagnostics","setting receipts and imported identities/effects are declarations, not verified authority","audio samples/position do not prove audibility; thermal/battery observations do not establish app energy or causality"]});
    value["analysis"]["formats"]
        .as_array_mut()
        .unwrap()
        .extend([json!("runtime-checks-v1"), json!("sanitizer-report-text")]);
    value["interactiveDebugging"] = json!({"registration":"implemented_bounded","routes":{"chrome":"page_debugger: owned CDP session/page, pauseId fenced stack/scopes/own variable descriptors, breakpoints, exception stops, pause/resume/step/evaluate and temporary structured logpoints","firefoxWebkit":"context_debugger controls automation calls; not a JavaScript debugger","native":"LLDB, JDWP and DAP transports unregistered","reactNativeFlutter":"existing fixed diagnostic reads/profilers; no interactive debugger attach"},"session":{"identity":"owned browser target/page reference plus sessionId; pauseId changes on every stop","exclusive":"same-page profiler and debugger access conflict; cleanup and status remain available","cleanup":"breakpoint_remove removes a selected owned marker; disable releases the owned debugger; transport loss invalidates runtime access","variables":"own descriptors only, getters skipped, no object expansion or arbitrary objectId; maximum 100 values/6 KiB per page","executionChanges":["pause","step","exception stops","breakpoint conditions","evaluate","logpoint console.debug"]},"buildProfiles":{"field":"diagnostics","verified":false,"effects":"declarations retained with effective profile and artifact; existing explicitly configured builder arguments control instrumentation"},"reports":{"formats":["runtime-checks-v1","sanitizer-report-text"],"query":"rows, kind failure; report IDs are correlations","bounds":{"reports":1024,"stacksPerReport":8,"framesPerStack":64},"tools":["ASan","TSan","UBSan","StrictMode","race/lock/runtime checks"],"absenceProvesCleanRun":false},"gaps":["native thread/variable control, remote adapter launch, release attach and native probe insertion unavailable","normalized report identities and references are producer claims","live debugger/build/runtime-check matrix and overhead unverified"]});
    value["analysis"]["formats"]
        .as_array_mut()
        .unwrap()
        .push(json!("reproduction-v1"));
    value["reproduction"] = json!({"registration":"implemented_bounded","readiness":"not_probed","plan":"diagnostics_plan","presets":["crash","hang","memory_growth","slow_interaction","startup","network","background_media","soak"],"markers":{"source":"marker","clock":"host wall milliseconds; action binding declared, completion unverified"},"repeats":{"owner":"existing bounded Android follow collectors; no action replay","maximumWindowMs":300000,"maximumCycles":256,"cancel":"diagnostics_stop","campaignRunner":"unregistered"},"faults":{"owners":["network_set","context_configure","network_route + callback_resolve","act app lifecycle"],"automaticExecution":false,"nativePressureAndPermissionSetters":"unregistered"},"evidence":{"format":"reproduction-v1","compare":"diagnostics_compare","attempts":1024,"maxDeclaredRunMs":86400000,"identity":"producer declared","statisticalSignificance":false,"tailGuarantees":false},"gaps":["live platform/runtime acceptance and overhead unverified","app-owned fixture/randomness controls must be explicitly exposed","fault receipts/cleanup labels are not verified authority; imported runs do not execute commands"]});
    value["sources"].as_array_mut().unwrap().push(json!({"kind":"marker","registration":"implemented","availability":"unknown","prerequisite":"open investigation with matching binding.actionIds; one of 16 collector slots","format":"testrun-events-v1","scope":"host observation and caller-declared action; no app input/completion proof","effects":["write_private_evidence"],"cost":{"class":"bounded_host_event","work":"fixed buffered event; no external source/process or target access","appInstrumentation":"none","appOverhead":"not_measured; host work can contend"}}));
    value["analysis"]["formats"]
        .as_array_mut()
        .unwrap()
        .extend([json!("test-results-v1"), json!("junit-xml")]);
    value["testing"] = json!({"registration":"implemented_bounded","assertions":{"queryView":"assertions","domains":["ui","api","accessibility","visual","media","performance"],"predicates":["count","equals","metric"],"outcomes":["passed","assertion_failed","automation_failed","fixture_failed","unavailable"],"limits":{"checks":32,"selectedRowsPerCheck":200,"witnesses":8},"scope":"retained observations only; no app code execution or conformance inference"},"runners":{"execution":"existing project_start task check; exact declared argv, deadline, private logs and cancellation; external controller must have exclusive target ownership","families":["XCTest","Swift Testing","Android instrumentation/Espresso/UI Automator","Flutter/Dart","web","package-native","external"],"ingestion":["test-results-v1 normalized records","junit-xml selected dialect"],"cases":1024,"attachments":"private producer references; no automatic file/archive reads or uploads"},"failureCapture":{"owner":"start existing collector/trace/recording before action, stop after observed failure; declared prebuffered/post-failure bounds in report","automaticFailureTrigger":false,"preEventReconstruction":false},"performance":{"method":"fixed metric bounds with exact unit/definition; reproduction-v1 separately reports matched-run sample counts, variation and measurement error","statisticalSignificance":false,"tailGuarantees":false},"gaps":["live runner/device/control matrix unverified","native xcresult archives and runner-specific binary attachment parsing unregistered; explicit normalized export only","automatic failure-triggered capture and pre-event ring buffer unregistered"]});
    value["testing"]["runners"]["execution"] = json!(
        "existing native-profile project_start task check; other project kinds keep their external declared execution owner; release competing target input control before a driver starts"
    );
    value["analysis"]["formats"]
        .as_array_mut()
        .unwrap()
        .extend([
            json!("framework-evidence-v1"),
            json!("runtime-samples-v1"),
            json!("embedded-cdp-v1"),
        ]);
    value["sources"].as_array_mut().unwrap().push(json!({"kind":"embedded_cdp","registration":"implemented_bounded","availability":"unknown","prerequisite":"explicit numeric loopback HTTP endpoint; matching investigation targetId/appId; exact pageId/expectedUrl and allowDebuggerAttachment:true for observation","modes":["discover","observe"],"effects":["read_selected_inspector","attach_borrowed_page","enable_runtime_page_log_observation","optional_network_observation","write_private_evidence"],"notPerformed":["enable debugging endpoint","port scanning/forwarding","input/evaluation/profiler control","native process attachment","close borrowed page"],"limits":{"pages":64,"events":256,"sampleMs":[100,10000]},"format":"embedded-cdp-v1"}));
    value["frameworks"] = json!({"registration":"implemented_bounded","composition":"framework-evidence-v1; at most 8 inline components and 64 explicit witnessed links; existing parsers keep models/units and separate clocks/generations","runtimeExports":"runtime-samples-v1 typed app/exporter snapshots; sampled/evented speedscope; chrome-trace; selected existing native formats","identity":"producer-declared exact build/architecture and component process/runtime generation; imported reports do not authenticate targets or symbols","families":["kotlin_multiplatform","compose_multiplatform","dotnet_maui","capacitor","cordova","ionic","electron","cef","tauri","webview2","wkwebview","hybrid","unity","unreal","custom_engine"],"targetClasses":["phone","tablet","desktop","tv","watch","automotive","spatial","extension","service","unknown"],"capabilityRule":"target class/framework name grants no capture or automation; use actual selected transport/source capability; unsupported transports use exports","gaps":["live SDK/platform/version compatibility unverified","EventPipe/nettrace/JFR/Unity binary/Unreal utrace decoding and automatic converter execution unregistered","WKWebView uses WebKit protocol; embedded CDP is refused","no target input automation, endpoint enablement, managed debugger or native symbol resolver added"]});
    value["frameworks"]["embeddedObservationHost"] = json!({"supported":cfg!(unix),"requirement":"existing cross-process controller lease requires Unix; non-Unix observation returns UNSUPPORTED_HOST; discovery and imports remain separate"});
}

fn service_capabilities(value: &mut Value) {
    value["analysis"]["formats"].as_array_mut().unwrap().extend(
        [
            "service-evidence-v1",
            "host-process-v1",
            "box-log-v1",
            "js-cpu-profile",
            "jfr-json",
            "pprof",
            "stackprof-json",
            "xdebug-cachegrind",
            "windows-minidump",
        ]
        .map(|f| json!(f)),
    );
    value["sources"].as_array_mut().unwrap().extend([
        json!({"kind":"host_process","registration":"implemented_bounded","availability":"unknown","prerequisite":"explicit local process:HOST:PID and matching targetId/appId(serviceId) binding; optional expectedGeneration fences a prior birth","hosts":["macos","linux","windows"],"effects":["read_selected_process_identity_and_resources","spawn_fixed_ps_or_PowerShell_reader_where_needed","write_private_evidence"],"ownership":"borrowed; no census, process launch, profiler attach, environment reads or target signals","format":"host-process-v1","cost":{"class":"single_process_snapshot","toolDeadlineMs":3000,"nativeFileBytes":65536,"targetOverhead":"unmeasured"}}),
        json!({"kind":"box_logs","registration":"implemented_bounded","availability":"unknown","prerequisite":"installed trusted Box, absolute project and exact workspace/operationId/stream, matching targetId/appId(serviceId) binding","effects":["invoke_fixed_Box_logs_read","read_private_retained_output","write_private_evidence"],"lifecycleOwner":"Box; no launch/stop/recover or generic command forwarding","format":"box-log-v1","cost":{"class":"one_retained_tail","toolDeadlineMs":3000,"tailBytes":65536,"encodedOutputBytes":524288}})
    ]);
    value["services"] = json!({"registration":"implemented_bounded","selection":"service-evidence-v1: exact host/OS/transport, process birth, runtime/version, build/architecture, borrowed/external/Box launch owner; no implicit remote access","runtimes":{"node":"js-cpu-profile","bun":"compatible js-cpu-profile export; no V8 identity inferred","deno":"js-cpu-profile","jvm":"jfr-json from explicit jfr print --json","dotnet":"sampled/evented speedscope export","go":"bounded gzip/raw pprof profile.proto","python":"py-spy-compatible speedscope export; no pickle","ruby":"StackProf public JSON; no Marshal","php":"selected Xdebug Cachegrind line-cost dialect","native":"existing native traces, Apple reports, host snapshots and selected minidump metadata"},"workloads":"host, exact container/image/namespace or Kubernetes cluster/namespace/pod UID/container/restart/image; imported claims do not authorize exec/deployment/privileged tracing","gaps":["live runtime/tool/OS/architecture range unverified","no automatic remote/container/Kubernetes transport or credential discovery","ETW/core/minidump unwind, binary JFR/EventPipe, complete heaps and symbol acquisition unregistered","Box retains lifecycle and log ownership; no service supervisor added","process snapshots are observations, not rates, tree census or app-health proof"]});
    value["analysis"]["formats"].as_array_mut().unwrap().extend(
        [
            "observability-evidence-v1",
            "otlp-json",
            "prometheus-json",
            "loki-json",
            "sentry-json",
            "jaeger-json",
            "cloudwatch-json",
            "crashlytics-json",
            "play-vitals-json",
            "deployment-evidence-v1",
        ]
        .map(|f| json!(f)),
    );
    value["sources"].as_array_mut().unwrap().push(json!({"kind":"observability_http","registration":"implemented_bounded","availability":"unknown","hosts":"Unix private credential-file ownership enforcement; imports on other hosts","directReaders":["prometheus","loki","sentry_event"],"effects":["read_explicit_private_configuration_and_optional_bearer_file","one_configured_GET_with_possible_provider_query_cost","write_projected_private_evidence"],"scope":"exact account/project/environment/region/resource/release and UTC window; no default production, redirects, proxy discovery, auth refresh or fallback","bounds":{"windowSeconds":86400,"requestMs":5000,"responseBytes":1048576,"records":2000,"pages":1,"retries":0},"cost":"backend scan/cost unknown; response/time bounds do not bound provider billing; cancellation may outlive client timeout","format":"observability-evidence-v1"}));
    value["observability"] = json!({"queryView":"observability","joins":"exact reported trace/span IDs and metric exemplars; witnessed associations, no causal proof","imports":{"otel":"otlp-json resource spans/logs/metrics; JSON only","tempo":"OTLP JSON export","jaeger":"jaeger-json","cloudwatch":"selected logs, Logs Insights and GetMetricData JSON pages","crashlytics":"selected BigQuery event JSON projection","playVitals":"MetricsRow JSON","apple":"existing apple-ips/apple-metrickit readers","datadog_newRelic_honeycomb":"explicit OTLP JSON exports; native REST readers unregistered","deployment":"deployment-evidence-v1; declared release/image/digests/config/audit observations"},"retention":"private immutable selected evidence and provenance; explicit import can contain customer data; live default omits log text/Sentry payloads and strips named credentials","gaps":["live provider/account acceptance unverified","no local OTLP listener, SDK install, fleet agent, OAuth login or credential vault","no full-incident absence/health inference from zero, partial, sampled or expired results","import scope is a claim, not authenticated IAM/deployment identity","native vendor REST, binary OTLP and GUI acquisition unregistered"]});
}

pub(crate) async fn call(name: &str, args: &Value) -> Result<Value> {
    let definition =
        super::definition(name).ok_or_else(|| Error::invalid("Unknown diagnostic operation"))?;
    crate::schema::validate(&definition["inputSchema"], args)?;
    if name == "diagnostics_capabilities" {
        return Ok(capabilities());
    }
    if name == "diagnostics_plan" {
        return super::reproduction::plan(args);
    }
    if name != "diagnostics_prepare" || args["component"] != "builtin-parser" {
        return Err(Error::new(
            "DIAGNOSTIC_DEPENDENCY_UNAVAILABLE",
            "Select builtin-parser; native dependency preparation is not registered",
        ));
    }
    if args.get("version").is_some_and(|v| v != PARSER) {
        return Err(Error::new(
            "DIAGNOSTIC_DEPENDENCY_VERSION",
            "Use the bundled parser version reported by diagnostics_capabilities",
        ));
    }
    let began = std::time::Instant::now();
    let (_sender, mut cancelled) = watch::channel(None);
    let index = worker::run(
        "chrome-trace",
        br#"{"traceEvents":[{"ph":"X","name":"self-check","ts":0,"dur":1}]}"#,
        &mut cancelled,
    )
    .await?;
    if index.parser != PARSER
        || index.format != "chrome-trace"
        || index.rows.len() != 1
        || index.rows[0].value != Some(1.0)
    {
        return Err(Error::new(
            "DIAGNOSTIC_DEPENDENCY_VERSION",
            "Bundled parser did not satisfy its versioned round-trip contract",
        ));
    }
    Ok(
        json!({"component":"builtin-parser","version":PARSER,"availability":"ready","verification":{"method":"same_executable_bounded_roundtrip","parser":index.parser},"elapsedMs":began.elapsed().as_secs_f64()*1000.0,"downloads":false,"installations":false,"appChanges":false,"deviceChanges":false,"managerStarted":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn diagnostic_access_discovery_preserves_unknown_source_readiness_and_refuses_unregistered_preparation()
     {
        let result = call("diagnostics_capabilities", &json!({})).await.unwrap();
        assert_eq!(result["sources"][0]["availability"], "unknown");
        assert_eq!(result["disabled"]["collectorProcessesStarted"], 0);
        for operation in result["operations"].as_array().unwrap() {
            assert!(
                !operation["access"]["effects"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(
                operation["access"]["remoteTransmission"],
                operation["operation"] == "diagnostics_start"
            );
        }
        assert_eq!(
            call("diagnostics_prepare", &json!({"component":"perfetto"}))
                .await
                .unwrap_err()
                .code,
            "DIAGNOSTIC_DEPENDENCY_UNAVAILABLE"
        );
        assert_eq!(
            call(
                "diagnostics_prepare",
                &json!({"component":"builtin-parser","version":"unverified"})
            )
            .await
            .unwrap_err()
            .code,
            "DIAGNOSTIC_DEPENDENCY_VERSION"
        );
        assert!(
            call(
                "diagnostics_prepare",
                &json!({"component":"builtin-parser","password":"must-not-store"})
            )
            .await
            .is_err()
        );
    }
    #[test]
    fn diagnostic_verification_inventory_keeps_live_gaps_and_source_effects_explicit() {
        let v = capabilities();
        let cells = v["verification"]["cells"].as_array().unwrap();
        let mut ids = std::collections::BTreeSet::new();
        assert!(!cells.is_empty());
        for cell in cells {
            assert!(ids.insert(cell["id"].as_str().unwrap()));
            assert!(!cell["families"].as_array().unwrap().is_empty());
            assert!(!cell["gaps"].as_array().unwrap().is_empty());
            assert_ne!(cell["liveAcceptance"], "verified");
        }
        assert_eq!(
            effects("diagnostics_capabilities")["remoteTransmission"],
            false
        );
        assert_eq!(effects("diagnostics_start")["remoteTransmission"], true);
        assert!(
            effects("diagnostics_start")["effects"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v == "optional_read_private_provider_config_and_selected_bearer_file")
        );
    }
}

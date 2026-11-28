//! Backend-independent browser lifecycle requests, references and capabilities.

pub use super::context_proxy::ContextProxy;
pub use super::har::CaptureContent as HarCaptureContent;
pub use super::har::CaptureMode as HarCaptureMode;
pub use super::har::NotFound as HarNotFound;
pub use super::routing::Header as RouteHeader;
pub use super::routing::Overrides as RouteOverrides;
pub use super::socket_routes::{Destination as WebSocketDestination, Message as WebSocketMessage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

pub const NODE_VERSION: &str = "24.12.0";
pub const NODE_SHA256: &str = "cdd4fee89f17b91fb473a03d50ebbdef4f955740a79f8a9d8382db432198b0b7";
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
pub const MAX_TIMEOUT_MS: u64 = 900_000;
pub const MAX_RESOURCES: usize = 256;
pub const MAX_OPERATIONS: usize = 256;
pub(crate) const MAX_MEDIA_FIXTURE_BYTES: u64 = 256 * 1024 * 1024;

/// Lifecycle availability is separate from CLI/MCP registration and acceptance.
#[expect(
    clippy::too_many_lines,
    reason = "The declarative capability projection keeps registered operations and engine restrictions together for one public response"
)]
pub fn capabilities(engine: Engine, host_supported: bool) -> Value {
    if engine == Engine::Chrome {
        let mut result = json!({"backend":engine.backend(),"engine":engine,"host":"darwin-x64-macos15",
            "browserVersion":super::preparation::CHROME_VERSION,"registered":true,
            "lifecycle":{"available":host_supported,"scope":"owned-isolated-launch","modes":["headless","headed"],"acceptance":"fixture-only"},
            "operations":["context_create","context_list","context_select","context_close","page_create","page_list","page_select","page_close","page_navigate","page_read","page_read_url","page_styles","page_clipboard","page_action","page_wait","frame_enter","page_input","input_release","callback_resolve","events_read","events_wait","fixture_register","directory_register","fixture_list","fixture_verify","page_upload","chooser_resolve","download_list","download_status","download_cancel","download_delete","batch"],
            "pageReading":{"available":host_supported,"maxBytes":8192,"isolatedWorld":true,"documentReferences":true,"snapshotElementReferences":true,"coordinateInputObservations":host_supported,"enteredFrameVisibilityConditions":host_supported,"directUrl":host_supported},
            "inputScope":{"coordinateSpace":"css-viewport","observationTtlMs":10000,"holdExpiryMs":30000,"touchTap":host_supported,"touchContextRequired":true,"viewerControl":host_supported,"viewerScope":"explicit-owned-page","livePreviewInputTransform":"verified-frame-only"},
            "fixtureTransfer":host_supported,
            "mediaFixtures":{"available":host_supported,"scope":"owned-browser-launch","microphoneFormats":["wav"],"cameraFormats":["y4m","mjpeg"],"maxBytesPerFile":MAX_MEDIA_FIXTURE_BYTES},
            "clipboardScope":{"text":host_supported,"origin":"explicit-current-page-origin","scope":"system-text-clipboard","maxBytes":16384,"permissionLifetime":"owned-context"},
            "events":{"available":host_supported,"scope":"owned-context","cursor":"explicit-before-trigger","capacity":512,"metadataOnly":true,"types":super::activity::EVENT_NAMES,"pageless":["service_worker_created"]},
            "creationOptions":["device","acceptDownloads","hasTouch","viewport","deviceScaleFactor","screen","isMobile","userAgent","locale","timezoneId","javaScriptEnabled","bypassCSP","ignoreHTTPSErrors","offline","colorScheme","reducedMotion","forcedColors","contrast","geolocation","permissions","serviceWorkers","baseURL","recordHar","storageState"],
            "navigation":{"available":host_supported,"httpResponseMetadata":true,"sameDocument":true,"history":true,"milestones":["commit","domcontentloaded","load","networkidle"]},
            "sharedApi":{"verbs":["connect","act","observe","status","cancel","disconnect","control_status","control_acquire","control_release"],"explicitBrowserTarget":true,"asynchronous":true,"orderedBatches":host_supported,"viewerControl":host_supported,"maxActions":8},
            "persistent":host_supported,"remoteAttachment":host_supported,"pageInteraction":host_supported,"capture":false,"diagnostics":false,
            "connectionModes":{"profiles":["disposable","retained","borrowed"],"attachment":["loopback-ws","wss"],"adoptIsolatedContext":true,"installedAcceptance":"pending"},
            "preview":{"available":false},"recordingScope":{"available":false},"evaluation":{"available":false}});
        result.as_object_mut().unwrap().extend(
            capture_capabilities(engine, host_supported)
                .as_object()
                .unwrap()
                .clone(),
        );
        result["diagnostics"] = json!(host_supported);
        result["developerTools"] = super::developer_tools::capabilities(host_supported);
        result["webMcpDebug"] = json!({"available":host_supported,"operation":"web_mcp_debug","backend":"native-WebMCP","annotation":"debugging","scopes":["page","frame"],"maxScopes":4,"maxToolsPerScope":32,"maxInvocationsPerScope":16,"maxResultBytes":65536,"asynchronous":true});
        result["webMcp"] = json!({"available":host_supported,"operation":"web_mcp","backend":"native-WebMCP","scopes":["page","frame"],"selection":"current-toolRef","maxScopes":4,"maxToolsPerScope":32,"maxInvocationsPerScope":16,"maxResultBytes":65536,"asynchronous":true});
        result["extensions"] = json!({"available":host_supported,"operation":"browser_extension","scope":"owned-Chrome-for-Testing-regular-profile-context","source":"registered-directory","maxExtensions":16});
        result["pwa"] = json!({"available":host_supported,"operation":"browser_pwa","scope":"owned-Chrome-for-Testing-profile","source":"standard-web-app-manifest","maxApps":8});
        result["framework"] = super::framework::capabilities(host_supported);
        result["css"] = json!({"available":host_supported,"backend":"bundled-DevTools-CSSMatchedStyles","maxRulesPerPage":50,"maxResultBytes":65536,"snapshot":false});
        result["settings"] = json!({"available":host_supported,"context":["offline","geolocation","permissions_grant","permissions_clear","extra_http_headers","http_credentials"],"page":["page_set_extra_http_headers","page_emulate_media","page_emulate_throttling"],"throttling":{"available":host_supported,"scope":"page-and-frames","cpuRate":{"minimum":1,"maximum":20},"networkPresets":["none","offline","slow-3g","fast-3g","slow-4g","fast-4g"]},"mobileEmulation":host_supported,"mediaEmulation":host_supported,"permissionNames":super::settings::permissions(engine),"permissionOrigin":"explicit-ascii-http-origin","scope":"owned-isolated-context","targets":"pages-frames-and-popups; user agent and offline also on service workers","maxHeaders":64,"maxHeaderBytes":16384,"restoration":"replace-setting-or-close-owned-context"});
        result["settings"]["devices"] = json!(super::devices::names());
        result["creationOptions"].as_array_mut().unwrap().extend([
            json!("extraHTTPHeaders"),
            json!("httpCredentials"),
            json!("clientCertificates"),
            json!("proxy"),
        ]);
        result["settings"]["authentication"] = json!({"scope":"owned-pages-child-frames-dedicated-and-service-workers","automatic":host_supported,"manualCallbacksTakePrecedence":true,"workers":host_supported,"dedicatedWorkers":"page-interception","sharedWorkers":false,"serviceWorkers":host_supported,"maxTrackedRequests":512,"observationLoss":"replace-connection"});
        result["operations"].as_array_mut().unwrap().extend([
            json!("context_configure"),
            json!("page_set_extra_http_headers"),
            json!("page_emulate_media"),
            json!("page_emulate_throttling"),
        ]);
        result["performance"] = json!({"available":host_supported,"backend":"bundled-native-DevTools-trace-model","scope":"owned-browser-process","contextIsSelectionOnly":true,"webVitals":"observed-trace-window","fieldData":false,"pageLifetime":false,"maxModels":1,"maxTraceBytes":33554432,"maxEvents":250000,"maxResultBytes":65536,"borrowedFrontend":false});
        result["heap"] = json!({"available":host_supported,"scope":"selected-page-target","format":"v8-heap-snapshot-json","maxBytes":super::heap::MAX_BYTES,"maxLoadedSnapshots":2,"maxProfilers":4,"comparison":"same-owned-page-and-identity-epoch","analysis":"bundled-native-DevTools-heap-worker","privateQueries":true});
        result["heap"]["imports"] = json!({"available":host_supported,"operation":"heap_import","privateRegularFileRequired":true,"maxBytes":super::heap::MAX_BYTES,"comparison":"explicit-caller-declared-unverified-import-group","capturedImportedComparison":false});
        result["heap"]["comparison"] =
            json!("captured-profiler-epoch-or-explicit-unverified-import-group");
        result["networkEvidence"] = json!({"available":host_supported,"operations":["network_read","network_evidence_list","network_evidence_remove"],"maxBodyBytes":1048576,"maxEvidence":32,"bodyFormat":"decoded-http-payload","private":true,"source":"native-CDP-bounded-resource-cache","nativeCacheBytesPerResource":262144,"headerCompleteness":"Network-request-response-events"});
        result["networkEvidence"]["responseDetails"] = json!({"available":host_supported,"option":"responseDetails","private":true,"fields":["serverAddr","securityDetails","timing"],"unknownTiming":"pinned-sdk-sentinels"});
        result["evaluation"] = json!({"available":host_supported,"scopes":["page","frame","worker","handle"],"structuredValues":true,"handleArguments":true,"propertyHandles":host_supported,"propertyEnumeration":host_supported,"workers":host_supported,"maxBytes":1048576,"maxHandles":256,"nativeCallbacks":false});
        result["evaluation"]["workerScope"] = json!({"dedicated":host_supported,"service":host_supported,"shared":host_supported,"contextInventory":"context_worker_list","consoleHistory":host_supported,"startupHistoryBoundary":"new-worker-observed-during-subscription"});
        result["developer"] = json!({"available":host_supported,"nativeFrontend":true,"maxSessions":4,"backend":"owned-native-CDP","nodeRequired":false,"maxEvidence":32,"maxEvidenceBytes":134217728,"audits":{"available":host_supported,"implementation":"bundled-Chrome-Lighthouse","categories":["accessibility","seo","best-practices","agentic-browsing"],"modes":["navigation","snapshot"],"devices":["desktop","mobile"],"formats":["lighthouse-result-json","lighthouse-report-html"],"maxReportBytes":16777216},"profiles":{"available":host_supported,"takeCoverage":"profile_take_coverage","query":"profile_query","queryKinds":["functions"],"preciseCoverageOptions":["javascriptCallCount","javascriptDetailed","reportAnonymousScripts"],"scope":"selected-page-target","navigation":"cumulative","sourceTextsIncluded":false,"formats":["v8-cpu-profile","v8-precise-javascript-coverage","cdp-css-rule-usage","v8-precise-javascript-coverage-delta","cdp-css-rule-usage-delta"],"maxBytesPerReport":8388608},"tracing":{"available":host_supported,"scope":"owned-browser-process","contextIsSelectionOnly":true,"format":"chrome-trace-event-json","playwrightTraceViewer":false,"maxBytes":33554432},"debugger":{"available":host_supported,"scope":"selected-page-target","automationDebugger":false,"pauseIdentityRequired":true,"maxFrames":100,"maxBreakpoints":128}});
        result["developer"]["debugger"]["inspection"] = json!({"sessionIdentity":true,"scopeDescriptors":true,"variables":"own descriptors; getters skipped; no nested expansion","maxVariables":100,"maxVariablePageBytes":6144,"temporaryLogpoints":true,"logpointEffect":"literal structured console.debug condition; execution/timing can change","exclusiveWith":"same-page profiles"});
        result["developer"]["tracing"]["duration"] = json!({"default":"manual","minimumMs":1000,"maximumMs":300000,"finalization":"one-bounded-automatic-attempt-then-explicit-stop","terminalReceiptRetained":true});
        result["developer"]["tracing"]["analysisOnStop"] = json!({"available":host_supported,"options":["analysisDeveloper","analysisPage"],"ownedFrontendRequired":true,"postCaptureFrontendAllocation":true,"captureMetadataPageRequired":true,"ordinaryCapacityRequired":true,"implicitTimedCompletion":false,"fullSdkParity":false});
        result["developer"]["tracing"]["lossProvenance"] = json!({"nativeCompletion":true,"unknownNativeLoss":null,"reasons":["event-gap","metadata-session-lost","native-data-loss","owned-browser-exited","stream-read-failed"],"retainedInReport":true});
        result["developer"]["tracing"]["reload"] = json!({"default":false,"requiresMetadataPage":true,"waitUntil":"load","ignoreCache":"per-reload"});
        result["developer"]["tracing"]["page"] =
            json!({"explicitOwnedPage":true,"contextRequired":true,"changesSelection":false});
        result["developer"]["tracing"]["autoStop"] = json!({"explicitBoolean":true,"trueWindowMs":5000,"startsAfterReload":true,"omitted":"raw-duration-defaults","exclusiveWith":"maxDurationMs"});
        result["diagnosticScope"] = json!({"console":host_supported,"pageErrors":host_supported,"networkMetadata":host_supported,"eventWaits":host_supported,"eventWaitScope":"explicit-cursor-and-owned-context","eventWaitUrl":"exact-redacted-http-url","webSockets":false,"bodies":false,"routes":false,"tracing":false,"sourceLoss":"cursor-expiry","consoleArguments":"released"});
        for operation in [
            "diagnostics_arguments_set",
            "diagnostics_arguments_list",
            "diagnostics_arguments_read",
            "diagnostics_arguments_remove",
            "web_socket_route_add",
            "web_socket_route_add_matcher",
            "web_socket_route_send",
            "web_socket_route_handlers",
            "web_socket_route_close",
            "web_socket_route_list",
            "web_socket_route_remove",
            "binding_add",
            "binding_list",
            "binding_remove",
            "binding_read",
            "page_route_set",
            "network_route",
            "context_route_set",
            "page_init_script_add",
            "page_init_script_list",
            "page_init_script_remove",
            "context_init_script_add",
            "context_init_script_list",
            "context_init_script_remove",
            "developer_open",
            "developer_status",
            "developer_close",
            "developer_evidence_list",
            "developer_evidence_finalize",
            "developer_evidence_remove",
            "page_audit",
            "page_css",
            "developer_tools_discover",
            "developer_tool_call",
            "developer_tool_result_read",
            "developer_tools_release",
            "web_mcp_debug",
            "web_mcp",
            "browser_extension",
            "browser_pwa",
            "framework_prepare",
            "context_framework_prepare",
            "framework_open",
            "framework_query",
            "framework_release",
            "profile_start",
            "heap_capture",
            "performance_load",
            "performance_query",
            "performance_release",
            "heap_stop",
            "heap_load",
            "heap_import",
            "heap_query",
            "heap_compare",
            "heap_release",
            "profile_status",
            "profile_stop",
            "profile_take_coverage",
            "trace_start",
            "trace_status",
            "trace_stop",
            "page_debugger",
            "network_read",
            "network_evidence_list",
            "network_evidence_remove",
            "har_register",
            "har_list",
            "har_finalize",
            "har_remove",
            "page_har_set",
            "evaluate",
            "selector_evaluate",
            "handle_read",
            "handle_owner_frame",
            "handle_content_frame",
            "frame_element",
            "frame_retain",
            "frame_owner_release",
            "page_request_gc",
            "profile_query",
            "handle_property",
            "handle_properties",
            "handle_dispose",
            "worker_list",
            "context_worker_list",
            "diagnostics_start",
            "diagnostics_read",
            "diagnostics_wait",
            "diagnostics_stop",
            "page_screenshot",
            "page_snapshot",
            "page_annotate",
            "page_pdf",
            "capture_compare",
            "page_resize",
            "preview_start",
            "preview_status",
            "preview_stop",
            "recording_start",
            "recording_status",
            "recording_stop",
        ] {
            result["operations"]
                .as_array_mut()
                .unwrap()
                .push(json!(operation));
        }
        result["diagnosticScope"]["webSockets"] = json!(host_supported);
        result["diagnosticScope"]["errorStacks"] = json!(host_supported);
        result["diagnosticScope"]["consoleStacks"] = json!(host_supported);
        result["diagnosticScope"]["stackPrivacy"] = json!("explicit-includeStack");
        result["diagnosticScope"]["exactSequenceLookup"] = json!(host_supported);
        result["diagnosticScope"]["cursorWindows"] = json!(host_supported);
        result["diagnosticScope"]["latestMatch"] = json!(host_supported);
        result["diagnosticScope"]["ignoreRules"] = json!({"available":host_supported,"scope":"explicit-read","maxRules":4,"effect":"annotate-preserve-evidence"});
        result["diagnosticScope"]["requestCorrelation"] = json!({"available":host_supported,"nativeNetworkIdentity":host_supported,"candidates":"explicit-unproven","maxCandidates":4,"causalProof":false});
        result["diagnosticScope"]["readFilters"] = json!([
            "page",
            "worker",
            "kind",
            "levels",
            "resourceTypes",
            "method",
            "status",
            "url",
            "requestId",
            "webSocketId"
        ]);
        result["diagnosticScope"]["consoleArguments"] = json!({"available":host_supported,"default":"released","retention":"explicit-context-opt-in","maxGroups":32,"maxArguments":16,"maxValueBytes":32768,"gettersMayExecute":true});
        result["diagnosticScope"]["bodies"] = json!(host_supported);
        result["diagnosticScope"]["tracing"] = json!(host_supported);
        result["diagnosticScope"]["routes"] = json!(host_supported);
        result["routing"] = json!({"available":host_supported,"operation":"page_route_set","contextOperation":"context_route_set","scopes":["page","context"],"scope":"context-or-page-and-child-frames","crossProcessFrames":host_supported,"serviceWorkers":host_supported,"serviceWorkerScope":"context-manual-and-har-routes","serviceWorkerHar":host_supported,"serviceWorkerAuthentication":host_supported,"matching":"exact-http-url-including-query","maxPatternsPerPage":1,"maxPatternsPerContext":1,"precedence":"matching-page-before-context","responses":["route_continue","route_abort","route_fulfill","route_fulfill_binary","route_fulfill_evidence","route_fulfill_http_evidence"],"fetchOperation":"route_fetch","maxBodyBytes":1048576,"maxInlineBinaryBodyBytes":65536,"maxTextBodyBytes":65536,"maxHeaders":64,"maxHeaderBytes":8192,"bodyEncodings":["utf8","base64","owned-network-evidence","owned-http-evidence"],"callbackExpiryMs":30000,"expiryAction":"abort","authenticationChallenges":false});
        result["routing"]["authenticationChallenges"] = json!(host_supported);
        result["routing"]["handlers"] = json!({"operation":"network_route","maxPerScope":32,"maxPerConnection":256,"matching":["exact","glob","regex"],"globDialect":"pinned-Playwright-ASCII-HTTP(S)","regex":true,"regexDialect":"ASCII-ECMAScript-subset-host-linear","predicates":false,"times":false,"maxPatternsPerScope":32,"order":"newest-first","editWhilePaused":false});
        result["routing"]["responses"]
            .as_array_mut()
            .expect("Route replies")
            .extend([
                json!("route_har"),
                json!("route_continue_with"),
                json!("route_fallback"),
                json!("route_fallback_with"),
            ]);
        result["routing"]["requestOverrides"] = json!({"url":"same-protocol-http","method":true,"headers":"preserve-browser-controlled","bodyBase64MaxBytes":super::routing::MAX_INLINE_BODY});
        result["har"] = super::har::capabilities(host_supported);
        result["routing"]["fulfillStatusRanges"] = json!([[200, 599]]);
        result["routing"]["authenticationScope"] = json!("context-or-page-and-child-frames");
        result["routing"]["authenticationExpiryAction"] = json!("cancel");
        result["routing"]["authenticationResponses"] =
            json!(["auth_credentials", "auth_cancel", "auth_default"]);
        result["initScripts"] = json!({"available":host_supported,"scopes":["page","context"],"scope":"context-or-page-and-child-frames","crossProcessFrames":host_supported,"maxSourceBytes":65536,"maxRegistrations":64,"removable":true,"currentDocumentExecuted":false,"executionOrder":"unspecified"});
        result["bindings"] = json!({"available":host_supported,"scopes":["page","context"],"scope":"context-or-page-and-child-frames","crossProcessFrames":host_supported,"maxRegistrations":64,"maxPendingCalls":64,"maxArgumentBytes":super::bindings::MAX_ARGUMENT_BYTES,"argumentRead":"explicit-private-binding_read","expiryMs":30000,"nativeHostFunctions":false});
        result["httpContexts"] = super::http::capabilities(host_supported);
        result["httpContexts"]["clientCertificates"]["browserInheritance"] = json!(host_supported);
        result["clientCertificateTunnels"] = json!({"available":host_supported,"scope":"owned-context","upstreamTlsVerification":true,"upstreamTlsVerificationScope":"configured-origins","originSelection":"normalized-exact-origin","alpn":"browser-offered-upstream-selected","proxyInheritance":true,"browserTlsOverride":"owned-context-as-pinned-internal-proxy-requirement","otherOrigins":"byte-relay-with-SDK-context-browser-TLS-policy","cleanup":"joined-context-and-connection-sockets","fullSdkParity":false});
        result["httpContexts"]["storageState"]["browserAssociated"] = json!(host_supported);
        result["httpContexts"]["storageState"]["maxVisitedOrigins"] = json!(1024);
        result["httpContexts"]["storageState"]["maxOriginBytes"] = json!(2048);
        result["httpContexts"]["associatedSettings"] =
            json!("explicit-http-options-native-browser-creation-options-restricted");
        result["httpContexts"]["partitionedCookies"] = json!(host_supported);
        result["httpContexts"]["cookiePartitionAncestry"] = json!(host_supported);
        result["httpContexts"]["cookiePartitionIdentityAvailable"] = json!(host_supported);
        result["httpContexts"]["unknownCookiePartitionOptInRequired"] = json!(false);
        for operation in super::http::OPERATIONS {
            result["operations"]
                .as_array_mut()
                .expect("Native operations")
                .push(json!(operation));
        }
        result["webSocketRouting"] = super::socket_routes::capabilities(host_supported);
        result["evaluation"]["selector"] = super::evaluation::selector_capabilities(host_supported);
        result["evaluation"]["retainedFrames"] = retained_frames(host_supported);
        result["evaluation"]["requestGC"] = json!({"available":host_supported,"operation":"page_request_gc","mechanism":"HeapProfiler.collectGarbage"});
        result["evaluation"]["scopes"]
            .as_array_mut()
            .expect("Evaluation scopes")
            .push(json!("frame_owner"));
        result["evaluation"]["elementRelationships"] = json!({"available":host_supported,"ownerFrame":"same-context","contentFrame":"source-page","sourceDocumentFence":true,"maxPendingNativeQueries":32,"embeddingElement":host_supported,"embeddingOperation":"frame_element","embeddingHandleLifetime":"parent-document","fullSdkParity":false});
        result["evaluation"]["handleAllocation"] = json!({"retainedAndPendingShareCapacity":true,"statusField":"handleCapacity","unknownNativeLifetime":"exact-context-destruction-or-session-detachment","documentRotationReleasesUnknown":false,"unknownContinuation":"exact-operation-native-owner-and-healthy-scope","unownedUnknownFencesSession":true});
        result["evaluation"]["handleAllocation"]["cleanupAdmission"] = json!({"highWater":256,"error":"EXECUTION_CLEANUP_PENDING","scope":"new-retained-results","existingHandleReadsAndDisposal":true});
        result["automationDebugger"] = json!({"available":false,"reason":"Playwright-context-automation-debugger-required","javascriptDebuggerOperation":"page_debugger","actionSourceAnnotations":false,"sourceLocationsAvailable":false,"runTo":false,"apiCallLog":false});
        result["browserState"] = super::storage::capabilities(host_supported, true);
        result["credentials"] = super::credentials::capabilities(host_supported, true);
        result["authentication"] = super::authentication::capabilities(host_supported);
        result["storage"] = json!({"available":host_supported,"format":super::storage::native::FORMAT,"formats":[super::storage::native::FORMAT,super::storage::native::CREDENTIALS_FORMAT],"scope":"owned-context-and-connection","maxSnapshots":super::storage::MAX_SNAPSHOTS,"maxBytesPerSnapshot":super::storage::MAX_BYTES,"privateFileImport":host_supported,"importFormats":[super::storage::native::FORMAT,super::storage::native::CREDENTIALS_FORMAT,super::storage::PLAYWRIGHT_FORMAT],"conversion":"validated-playwright-to-native","cookies":host_supported,"partitionedCookies":host_supported,"localStorage":host_supported,"indexedDB":host_supported,"opfs":host_supported,"sessionStorage":false,"virtualCredentials":host_supported,"virtualCredentialRestore":"explicit-installAuthenticator","restoreModes":["replace","merge"],"replace":"pinned-setStorageState-cache-cookies-visited-and-snapshot-origins","merge":"snapshot-cookies-keys-databases-files-upsert","partialOutcomes":"per-type-and-per-origin-progress","ordinaryResults":"private-artifact-metadata-only"});
        result["operations"].as_array_mut().unwrap().extend([
            json!("storage_register"),
            json!("storage_export"),
            json!("storage_apply"),
            json!("storage_forget"),
        ]);
        return result;
    }
    let available = engine.backend() == Backend::Playwright && host_supported;
    let mut result = json!({"backend":engine.backend(),"engine":engine,"host":if engine.backend()==Backend::Playwright{Some("darwin-x64-macos15")}else{None},
        "driverVersion":if engine.backend()==Backend::Playwright{Some(super::DRIVER_VERSION)}else{None},
        "nodeVersion":if engine.backend()==Backend::Playwright{Some(NODE_VERSION)}else{None},
        "browserRevision":engine.revision(),"registered":engine.backend()==Backend::Playwright,
        "lifecycle":{"available":available,"scope":"owned-isolated-launch","modes":["headless","headed"],"acceptance":"initial-host-library"},
        "persistent":available,"remoteAttachment":available,"pageInteraction":available,"capture":available,"diagnostics":available,
        "connectionModes":{"profiles":["retained","borrowed"],"attachment":["loopback-ws","wss"],"adoptIsolatedContext":false,"installedAcceptance":"pending"}});
    let workflows = json!({
        "sharedApi":{"verbs":["connect","act","observe","status","cancel","disconnect","control_status","control_acquire","control_release"],"explicitBrowserTarget":true,"asynchronous":true,"actionId":"operation-uuid","orderedBatches":available,"maxActions":8,"implicitObservation":false,"viewerControl":available},
        "settings":{"available":available,"context":["offline","geolocation","permissions_grant","permissions_clear"],"devices":super::devices::names(),"throttling":{"available":false,"reason":"chrome-only-cdp-emulation"},"creationOptions":["device","viewport","deviceScaleFactor","acceptDownloads","hasTouch","screen","isMobile","userAgent","locale","timezoneId","javaScriptEnabled","bypassCSP","ignoreHTTPSErrors","offline","colorScheme","reducedMotion","forcedColors","contrast","geolocation","permissions","serviceWorkers","baseURL","recordHar","storageState"],"mobileEmulation":available && engine==Engine::Webkit,"mediaEmulation":available,"permissionNames":super::settings::permissions(engine),"scope":"owned-isolated-context","permissionOrigin":"explicit-ascii-http-origin","restoration":"close-owned-context"},
        "storage":{"available":available,"format":"playwright-storage-state-v1","scope":"owned-context-and-connection","maxSnapshots":16,"maxBytesPerSnapshot":1048576,"privateFileImport":available,"cookies":available,"localStorage":available,"indexedDB":available,"opfs":available && engine==Engine::Firefox,"sessionStorage":false,"virtualCredentials":available,"virtualCredentialRestore":"explicit-installAuthenticator","restore":"explicit-destructive-replacement","restoreModes":["replace"],"ordinaryResults":"private-artifact-metadata-only"},
        "credentials":super::credentials::capabilities(available, false),
        "authentication":super::authentication::capabilities(available),
        "clock":{"available":available,"scope":"context-pages-frames-and-future-documents","operations":["install","set_fixed_time","set_system_time","pause_at","run_for","fast_forward","resume"],"maxRunForMs":60000,"maxFastForwardMs":604800000,"hostDeadlinesUnchanged":true,"workers":false,"restoration":"close-owned-context"},
        "diagnosticScope":{"console":available,"pageErrors":available,"networkMetadata":available,"eventWaits":available,"eventWaitScope":"explicit-cursor-and-owned-context","eventWaitUrl":"exact-redacted-http-url","webSockets":false,"bodies":false,"routes":false,"tracing":false},
        "events":{"available":available,"scope":"owned-context","cursor":"explicit-before-trigger","capacity":512,"metadataOnly":true,"types":super::activity::EVENT_NAMES.iter().filter(|name| **name != "service_worker_created").collect::<Vec<_>>(),"downloadMeaning":"allocation-only","historyLoss":"explicit-refusal"}});
    let interaction = json!({
        "clipboardScope":{"text":available,"read":available&&engine==Engine::Webkit,"origin":"explicit-current-page-origin","scope":"system-text-clipboard","maxBytes":16384,"permissionOrActivationRequired":true},
        "pageOperations":["page_read","page_read_url","page_styles","page_clipboard","page_action","page_wait","page_navigate","frame_enter","page_screenshot","page_resize","page_input","input_release"],
        "captureScope":"dedicated-screenshot","coordinateInput":available,"heldInput":available,"inputScope":{"coordinateSpace":"css-viewport","observationTtlMs":10000,"holdExpiryMs":30000,"holdExpiryCleanup":"after-retained-active-input-settles","touchTap":available,"touchContextRequired":true,"multiTouch":false,"viewerControl":available,"viewerScope":"explicit-owned-page","viewerInput":"private-authenticated-ipc-with-fresh-css-viewport-observation","livePreviewInputTransform":"verified-frame-only"},"streaming":available,"recording":available,"fixtureTransfer":available,
        "preview":{"available":available,"access":"read_only","codec":"jpeg","source":"playwright-screencast","acceptance":"initial-host-native","audio":false},
        "recordingScope":{"available":available,"encoding":"testrun-native-h264-from-jpeg","codec":"h264","container":"mp4","audio":false,"maxActivePerPage":1,"maxActivePerConnection":8,"maxDurationMs":600000,"defaultDurationMs":60000,"geometry":"fixed-canvas-aspect-fit-letterbox","captureTimingCalibrated":false},
        "evaluation":{"available":available,"scopes":["page","frame","worker","handle"],"structuredValues":true,"handleArguments":true,"propertyHandles":available,"propertyEnumeration":available,"workers":available,"workerScope":{"dedicated":available,"service":false,"shared":false,"consoleHistory":available},"nativeCallbacks":false,"maxBytes":1048576}});
    result
        .as_object_mut()
        .unwrap()
        .extend(workflows.as_object().unwrap().clone());
    result["settings"]["context"]
        .as_array_mut()
        .unwrap()
        .push(json!("extra_http_headers"));
    result["settings"]["context"]
        .as_array_mut()
        .unwrap()
        .push(json!("http_credentials"));
    result["settings"]["page"] = json!(["page_set_extra_http_headers"]);
    result["settings"]["creationOptions"]
        .as_array_mut()
        .unwrap()
        .push(json!("extraHTTPHeaders"));
    result["settings"]["creationOptions"]
        .as_array_mut()
        .unwrap()
        .push(json!("httpCredentials"));
    result["settings"]["creationOptions"]
        .as_array_mut()
        .unwrap()
        .push(json!("clientCertificates"));
    result["settings"]["creationOptions"]
        .as_array_mut()
        .unwrap()
        .push(json!("proxy"));
    result["networkEvidence"] = json!({"available":available,"operations":["network_read","network_evidence_list","network_evidence_remove"],"maxBodyBytes":1048576,"maxEvidence":32,"bodyFormat":"decoded-http-payload","private":true,"responseBodyPrerequisite":"known-uncompressed-content-length"});
    result["networkEvidence"]["responseDetails"] = json!({"available":available,"option":"responseDetails","private":true,"fields":["serverAddr","securityDetails","timing"],"unknownTiming":"pinned-sdk-sentinels"});
    result["initScripts"] = json!({"available":available,"scopes":["page","context"],"scope":"context-or-page-and-child-frames","maxSourceBytes":65536,"maxRegistrations":64,"removable":true,"currentDocumentExecuted":false,"executionOrder":"unspecified"});
    result["bindings"] = json!({"available":available,"scopes":["page","context"],"maxRegistrations":64,"maxArgumentBytes":super::bindings::MAX_ARGUMENT_BYTES,"argumentRead":"explicit-private-binding_read","expiryMs":30000,"nativeHostFunctions":false});
    result["diagnosticScope"]["webSockets"] = json!(available);
    result["diagnosticScope"]["errorStacks"] = json!(available);
    result["diagnosticScope"]["consoleStacks"] = json!(false);
    result["diagnosticScope"]["stackPrivacy"] = json!("explicit-includeStack");
    result["diagnosticScope"]["exactSequenceLookup"] = json!(available);
    result["diagnosticScope"]["cursorWindows"] = json!(available);
    result["diagnosticScope"]["latestMatch"] = json!(available);
    result["diagnosticScope"]["ignoreRules"] = json!({"available":available,"scope":"explicit-read","maxRules":4,"effect":"annotate-preserve-evidence"});
    result["diagnosticScope"]["requestCorrelation"] = json!({"available":available,"nativeNetworkIdentity":false,"candidates":"explicit-unproven","maxCandidates":4,"causalProof":false});
    result["diagnosticScope"]["readFilters"] = json!([
        "page",
        "worker",
        "kind",
        "levels",
        "resourceTypes",
        "method",
        "status",
        "url",
        "requestId",
        "webSocketId"
    ]);
    result["diagnosticScope"]["consoleArguments"] = json!({"available":available,"default":"released","retention":"explicit-context-opt-in","maxGroups":32,"maxArguments":16,"maxValueBytes":32768,"gettersMayExecute":true});
    result["diagnosticScope"]["bodies"] = json!(available);
    result["diagnosticScope"]["routes"] = json!(available);
    result["diagnosticScope"]["tracing"] = json!(available);
    result["tracing"] = json!({"available":available,"scope":"owned-context","format":"playwright-trace-zip","chunks":true,"groups":true,"maxChunks":4,"maxGroupDepth":32,"maxArchiveBytes":33554432,"maxSourceBytes":67108864,"sourceBudget":"polled-250ms","defaultDurationMs":30000,"maxDurationMs":300000,"sourceTextsIncluded":true,"sourceSelection":"explicit-sourceFiles-snapshot-at-start","maxSourceFiles":16,"maxSourceFileBytes":262144,"maxSelectedSourceBytes":2097152,"automaticCallerStacks":false,"live":true,"liveReaderAvailable":false,"groupLocations":true,"snapshotKinds":["dom","aria","screen"],"namePrefix":"ASCII-alphanumeric-underscore-hyphen-1-128","title":true,"operations":["trace_start","trace_status","trace_stop","trace_chunk_start","trace_chunk_stop","trace_group_start","trace_group_end","developer_evidence_list","developer_evidence_finalize","developer_evidence_remove"]});
    result["routing"] = json!({"available":available,"operation":"page_route_set","contextOperation":"context_route_set","scopes":["page","context"],"scope":"context-or-page-and-child-frames","crossProcessFrames":available,"serviceWorkers":false,"matching":"exact-http-url-including-query","maxPatternsPerPage":1,"maxPatternsPerContext":1,"precedence":"matching-page-before-context","responses":["route_continue","route_abort","route_fulfill","route_fulfill_binary","route_fulfill_evidence","route_fulfill_http_evidence"],"fetchOperation":"route_fetch","maxBodyBytes":1048576,"maxInlineBinaryBodyBytes":65536,"maxTextBodyBytes":65536,"maxHeaders":64,"maxHeaderBytes":8192,"bodyEncodings":["utf8","base64","owned-network-evidence","owned-http-evidence"],"callbackExpiryMs":30000,"expiryAction":"abort","authenticationChallenges":false});
    result["routing"]["handlers"] = json!({"operation":"network_route","maxPerScope":32,"maxPerConnection":256,"matching":["exact","glob","regex"],"globDialect":"pinned-Playwright-ASCII-HTTP(S)","regex":true,"regexDialect":"ASCII-ECMAScript-subset-host-linear","predicates":false,"times":false,"maxPatternsPerScope":32,"order":"newest-first","editWhilePaused":false});
    result["routing"]["fulfillStatusRanges"] = if engine == Engine::Webkit {
        json!([[200, 299], [400, 599]])
    } else {
        json!([[200, 599]])
    };
    result
        .as_object_mut()
        .unwrap()
        .extend(interaction.as_object().unwrap().clone());
    result["evaluation"]["selector"] = super::evaluation::selector_capabilities(available);
    result["evaluation"]["retainedFrames"] = retained_frames(available);
    result["evaluation"]["requestGC"] = json!({"available":available,"operation":"page_request_gc","mechanism":"pinned-driver-requestGC"});
    result["evaluation"]["scopes"]
        .as_array_mut()
        .expect("Evaluation scopes")
        .push(json!("frame_owner"));
    result["evaluation"]["elementRelationships"] = json!({"available":available,"ownerFrame":"same-context","contentFrame":"source-page","sourceDocumentFence":true,"embeddingElement":available,"embeddingOperation":"frame_element","embeddingHandleLifetime":"parent-document","fullSdkParity":false});
    result.as_object_mut().unwrap().extend(
        capture_capabilities(engine, available)
            .as_object()
            .unwrap()
            .clone(),
    );
    result["httpContexts"] = super::http::capabilities(available);
    result["httpContexts"]["storageState"]["browserAssociated"] = json!(available);
    result["httpContexts"]["associatedSettings"] =
        json!("registered-browser-user-agent-and-tls-with-explicit-http-overrides");
    result["httpContexts"]["partitionedCookies"] = json!(available && engine == Engine::Webkit);
    result["httpContexts"]["cookiePartitionAncestry"] = json!(false);
    result["httpContexts"]["cookiePartitionIdentityAvailable"] =
        json!(available && engine == Engine::Webkit);
    result["httpContexts"]["unknownCookiePartitionOptInRequired"] =
        json!(available && engine == Engine::Firefox);
    result["routing"]["responses"]
        .as_array_mut()
        .expect("Route replies")
        .extend([
            json!("route_har"),
            json!("route_continue_with"),
            json!("route_fallback"),
            json!("route_fallback_with"),
        ]);
    result["routing"]["requestOverrides"] = json!({"url":"same-protocol-http","method":true,"headers":"preserve-browser-controlled","bodyBase64MaxBytes":super::routing::MAX_INLINE_BODY});
    result["har"] = super::har::capabilities(available);
    result["developerTools"] = super::developer_tools::capabilities(available);
    result["webMcpDebug"] =
        json!({"available":false,"reason":"native-Chrome-WebMCP-domain-required"});
    result["framework"] = super::framework::capabilities(available);
    result["webSocketRouting"] = super::socket_routes::capabilities(available);
    result["automationDebugger"] = json!({"available":available,"operation":"context_debugger","scope":"owned-context-protocol-automation-calls","javascriptDebugger":false,"controls":["status","pause","resume","next","run_to","enable","calls"],"pauseIdentityRequired":true,"privateDetails":"explicit-includeDetails","actionSourceAnnotations":available,"actionSourceWrapper":"at_source","sourceLocationsAvailable":available,"sourceLocationPolicy":"explicit-page-action-input-metadata","maxSourceFileBytes":1024,"maxSourcePosition":i32::MAX,"runTo":available,"runToFileMatch":"substring","apiCallLog":available,"maxRetainedCalls":32,"maxLogEntriesPerCall":8,"maxStringCharacters":256});
    result["browserState"] = super::storage::capabilities(available, false);
    result
}

fn capture_capabilities(engine: Engine, available: bool) -> Value {
    json!({"capture":available,"captureScope":"dedicated-screenshot","captureOperations":["page_screenshot","page_snapshot","page_annotate","capture_compare"],"comparison":{"available":available,"scope":"immutable-current-connection-evidence","pixelThreshold":"maximum-channel-delta-0-255","changeThreshold":"ratio-0-1","maxCaptures":64,"maxCaptureBytes":16777216,"maxAggregateBytes":268435456},"pdf":{"available":available&&engine==Engine::Chrome,"mode":"headless","maxBytes":16777216},"streaming":available,"recording":available,"preview":{"available":available,"access":"read_only","codec":"jpeg","source":if engine==Engine::Chrome{"chrome-cdp-screencast"}else{"playwright-screencast"},"inputTransform":"verified-frame-only","audio":false},"recordingScope":{"available":available,"encoding":"testrun-native-h264-from-jpeg","codec":"h264","container":"mp4","audio":false,"maxActivePerPage":1,"maxActivePerConnection":8,"maxDurationMs":600000,"defaultDurationMs":60000,"geometry":"fixed-canvas-aspect-fit-letterbox","captureTimingCalibrated":false}})
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    ChromeCdp,
    Playwright,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Chrome,
    Chromium,
    Firefox,
    Webkit,
}

impl Engine {
    pub fn backend(self) -> Backend {
        match self {
            Self::Chrome | Self::Chromium => Backend::ChromeCdp,
            _ => Backend::Playwright,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Chromium => "chromium",
            Self::Firefox => "firefox",
            Self::Webkit => "webkit",
        }
    }
    pub fn revision(self) -> Option<&'static str> {
        match self {
            Self::Firefox => Some("1555"),
            Self::Webkit => Some("2370"),
            _ => None,
        }
    }
    pub(super) fn pinned_version(self) -> Option<&'static str> {
        match self {
            Self::Chrome => Some(super::preparation::CHROME_VERSION),
            Self::Firefox => Some("157.0"),
            Self::Webkit => Some("27.2"),
            Self::Chromium => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Headless,
    Headed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContextOptions {
    pub viewport: Viewport,
    pub device_scale_factor: f64,
    #[serde(default)]
    pub accept_downloads: bool,
    #[serde(default)]
    pub has_touch: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<Viewport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_mobile: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(
        default,
        rename = "extraHTTPHeaders",
        skip_serializing_if = "Option::is_none"
    )]
    pub extra_http_headers: Option<Vec<super::http::Header>>,
    #[serde(
        default,
        rename = "httpCredentials",
        skip_serializing_if = "Option::is_none"
    )]
    pub http_credentials: Option<super::http::Credentials>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_certificates: Option<Vec<super::http::ClientCertificate>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<ContextProxy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java_script_enabled: Option<bool>,
    #[serde(default, rename = "bypassCSP", skip_serializing_if = "Option::is_none")]
    pub bypass_csp: Option<bool>,
    #[serde(
        default,
        rename = "ignoreHTTPSErrors",
        skip_serializing_if = "Option::is_none"
    )]
    pub ignore_https_errors: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offline: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_scheme: Option<super::settings::ColorScheme>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reduced_motion: Option<super::settings::ReducedMotion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forced_colors: Option<super::settings::ForcedColors>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contrast: Option<super::settings::Contrast>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geolocation: Option<super::settings::Geolocation>,
    /// Granted for every origin at creation, as the pinned SDK does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Vec<super::settings::Permission>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_workers: Option<super::settings::ServiceWorkers>,
    /// Testrun-owned resolution base for relative navigations and URL matchers.
    #[serde(default, rename = "baseURL", skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Testrun-owned capture for the context's lifetime, published at `context_close`.
    #[serde(default, rename = "recordHar", skip_serializing_if = "Option::is_none")]
    pub record_har: Option<RecordHar>,
    /// Pinned `newContext({storageState})`: inline state or a caller-bound file,
    /// applied before the context's first page and never retained afterwards.
    /// Boxed so inline state does not enlarge every `Request`.
    #[serde(
        default,
        rename = "storageState",
        skip_serializing_if = "Option::is_none"
    )]
    pub storage_state: Option<Box<super::http::StorageStateInput>>,
}

/// Pinned `recordHar` options without a file path: Testrun publishes a private archive.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordHar {
    #[serde(default)]
    pub content: Option<HarCaptureContent>,
    /// Pinned deprecated alias: `content ?? (omitContent ? "omit" : default)`.
    #[serde(default)]
    pub omit_content: Option<bool>,
    #[serde(default = "record_har_mode")]
    pub mode: HarCaptureMode,
    #[serde(default)]
    pub url_filter: Option<super::routing::Matcher>,
    /// Also write the archive published at `context_close` to this new private file.
    #[serde(default)]
    pub path: Option<String>,
}
fn record_har_mode() -> HarCaptureMode {
    HarCaptureMode::Full
}
impl RecordHar {
    /// Capture options with the export path validated before the capture starts.
    pub(crate) fn options(
        &self,
    ) -> Result<
        (
            HarCaptureContent,
            HarCaptureMode,
            Option<super::har::Export>,
        ),
        super::session::Error,
    > {
        // Pinned default: attach for a .zip path, embed otherwise.
        let content = self.content.unwrap_or(if self.omit_content == Some(true) {
            HarCaptureContent::Omit
        } else if self
            .path
            .as_deref()
            .is_some_and(|path| path.ends_with(".zip"))
        {
            HarCaptureContent::Attach
        } else {
            HarCaptureContent::Embed
        });
        let export = self
            .path
            .as_deref()
            .map(|path| super::har::Export::new(path, None, content))
            .transpose()?;
        Ok((content, self.mode, export))
    }
    /// The capture filter, resolved against the context base like other matchers.
    pub(crate) fn filter(&self, options: &ContextOptions) -> Option<super::routing::Matcher> {
        self.url_filter.clone().map(|mut filter| {
            if let Some(base) = options.base() {
                filter.resolve(&base);
            }
            filter
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

impl Default for ContextOptions {
    fn default() -> Self {
        Self {
            viewport: Viewport {
                width: 1280,
                height: 720,
            },
            device_scale_factor: 1.0,
            accept_downloads: false,
            has_touch: false,
            screen: None,
            is_mobile: None,
            user_agent: None,
            extra_http_headers: None,
            http_credentials: None,
            client_certificates: None,
            proxy: None,
            locale: None,
            timezone_id: None,
            java_script_enabled: None,
            bypass_csp: None,
            ignore_https_errors: None,
            offline: None,
            color_scheme: None,
            reduced_motion: None,
            forced_colors: None,
            contrast: None,
            geolocation: None,
            permissions: None,
            service_workers: None,
            base_url: None,
            record_har: None,
            storage_state: None,
        }
    }
}

impl ContextOptions {
    pub fn validate(&self) -> Result<(), &'static str> {
        if let Some(certificates) = &self.client_certificates {
            super::http::client_certificates::validate(certificates)?;
        }
        if let Some(proxy) = &self.proxy {
            proxy.normalized()?;
        }
        if let Some(credentials) = &self.http_credentials {
            credentials.validate()?;
        }
        if let Some(headers) = &self.extra_http_headers {
            super::http::validate_headers(headers).map_err(|error| error.code)?;
        }
        if !(1..=8192).contains(&self.viewport.width)
            || !(1..=8192).contains(&self.viewport.height)
            || !self.device_scale_factor.is_finite()
            || !(0.5..=4.0).contains(&self.device_scale_factor)
        {
            return Err("INVALID_CONTEXT_OPTIONS");
        }
        if self.screen.is_some_and(|screen| {
            !(1..=8192).contains(&screen.width) || !(1..=8192).contains(&screen.height)
        }) || self.user_agent.as_ref().is_some_and(|value| {
            value.is_empty() || value.chars().count() > 512 || value.chars().any(char::is_control)
        }) || [&self.locale, &self.timezone_id].into_iter().any(|value| {
            value.as_ref().is_some_and(|value| {
                value.is_empty()
                    || value.chars().count() > 128
                    || value.chars().any(char::is_control)
            })
        }) {
            return Err("INVALID_CONTEXT_OPTIONS");
        }
        if let Some(geolocation) = &self.geolocation {
            geolocation.validate()?;
        }
        if let Some(permissions) = &self.permissions {
            super::settings::validate_permissions(permissions, None)?;
        }
        if self.base_url.is_some() && self.base().is_none() {
            return Err("INVALID_BASE_URL");
        }
        if let Some(filter) = self
            .record_har
            .as_ref()
            .and_then(|record| record.filter(self))
        {
            filter.compile()?;
        }
        Ok(())
    }
    /// The validated absolute HTTP(S) base, without credentials or fragment.
    pub(crate) fn base(&self) -> Option<url::Url> {
        let text = self.base_url.as_deref()?;
        if text.len() > 2048 || !text.is_ascii() || text.chars().any(char::is_control) {
            return None;
        }
        let url = url::Url::parse(text).ok()?;
        (matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none())
        .then_some(url)
    }
    pub(crate) fn validate_for(&self, engine: Engine) -> Result<(), &'static str> {
        self.validate()?;
        if let Some(permissions) = &self.permissions {
            super::settings::validate_permissions(permissions, Some(engine))?;
        }
        if engine == Engine::Firefox && self.is_mobile == Some(true) {
            return Err("MOBILE_EMULATION_UNSUPPORTED");
        }
        if engine == Engine::Webkit
            && self
                .proxy
                .as_ref()
                .is_some_and(ContextProxy::password_required)
        {
            return Err("HTTP_PROXY_PASSWORD_REQUIRED");
        }
        Ok(())
    }
    pub(crate) fn wire(&self) -> Value {
        let mut options = serde_json::to_value(self).expect("Typed context options serialize");
        if let Some(credentials) = &self.http_credentials {
            if let Some(credentials) = credentials.wire() {
                options["httpCredentials"] = credentials;
            } else {
                options.as_object_mut().unwrap().remove("httpCredentials");
            }
        }
        options["acceptDownloads"] = json!(if self.accept_downloads {
            "accept"
        } else {
            "deny"
        });
        if !self.has_touch {
            options.as_object_mut().unwrap().remove("hasTouch");
        }
        // Testrun resolves relative URLs itself so both adapters share one rule.
        options.as_object_mut().unwrap().remove("baseURL");
        options.as_object_mut().unwrap().remove("recordHar");
        options
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LaunchOptions {
    pub engine: Engine,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<ContextProxy>,
    #[serde(default, deserialize_with = "super::devices::context_options")]
    pub context: ContextOptions,
    #[serde(default, skip_serializing_if = "MediaFixtures::is_empty")]
    pub media_fixtures: MediaFixtures,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ConnectOptions {
    #[serde(flatten)]
    pub launch: LaunchOptions,
    pub channel: Option<super::preparation::Channel>,
    pub profile: Option<super::profiles::Selection>,
    pub attachment: Option<super::attachment::Attachment>,
}

impl ConnectOptions {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if let Some(attachment) = &self.attachment {
            attachment.validate(self.launch.engine)?;
            if self.profile.is_some()
                || self.channel.is_some()
                || self.launch.proxy.is_some()
                || !self.launch.media_fixtures.is_empty()
            {
                return Err("ATTACHMENT_LAUNCH_OPTIONS_UNSUPPORTED");
            }
            if attachment.context_id.is_some() && self.launch.context != ContextOptions::default() {
                return Err("ATTACHMENT_ADOPTED_CONTEXT_OPTIONS_UNSUPPORTED");
            }
            validate_attachment_context(self.launch.engine, &self.launch.context)?;
        }
        if self.channel.is_some() && self.launch.engine != Engine::Chrome {
            return Err("BROWSER_CHANNEL_UNSUPPORTED");
        }
        if let Some(profile) = &self.profile {
            profile.validate()?;
            if matches!(profile, super::profiles::Selection::Disposable)
                && (!(cfg!(unix) || super::processes::windows_bundle_host())
                    || self.launch.engine != Engine::Chrome
                    || self.channel.is_some())
            {
                return Err("DISPOSABLE_PROFILE_UNSUPPORTED");
            }
            if !self.launch.media_fixtures.is_empty() {
                return Err("PROFILE_MEDIA_FIXTURES_UNSUPPORTED");
            }
            if self.launch.context.storage_state.is_some() {
                return Err("PROFILE_INITIAL_STORAGE_UNSUPPORTED");
            }
            if self.launch.engine == Engine::Chrome
                && (self.launch.context.client_certificates.is_some()
                    || self.launch.context.proxy.is_some())
            {
                return Err("PROFILE_CONTEXT_NETWORK_UNSUPPORTED");
            }
        }
        self.launch.validate()
    }
}

pub(super) fn validate_attachment_context(
    engine: Engine,
    options: &ContextOptions,
) -> Result<(), &'static str> {
    if options.accept_downloads
        || (engine == Engine::Chrome && options.client_certificates.is_some())
    {
        return Err("ATTACHMENT_CONTEXT_HOST_RESOURCES_UNSUPPORTED");
    }
    Ok(())
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MediaFixtures {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub microphone_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_path: Option<String>,
}

impl MediaFixtures {
    pub(crate) fn is_empty(&self) -> bool {
        self.microphone_path.is_none() && self.camera_path.is_none()
    }

    pub(crate) fn paths(&self) -> impl Iterator<Item = &str> {
        [self.microphone_path.as_deref(), self.camera_path.as_deref()]
            .into_iter()
            .flatten()
    }
}

impl LaunchOptions {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.engine == Engine::Chromium {
            return Err("BACKEND_UNAVAILABLE");
        }
        if !self.media_fixtures.is_empty() && self.engine != Engine::Chrome {
            return Err("MEDIA_FIXTURES_UNSUPPORTED");
        }
        if self.media_fixtures.paths().any(|path| {
            !std::path::Path::new(path).is_absolute() || path.len() > 4096 || path.contains('\0')
        }) {
            return Err("MEDIA_FIXTURE_PATH_INVALID");
        }
        if let Some(proxy) = &self.proxy {
            proxy.normalized()?;
        }
        self.context
            .effective_for(self.engine, self.proxy.as_ref())
            .map(|_| ())
    }
    pub(crate) fn wire(&self) -> Value {
        let mut result = json!({"headless":self.mode == Mode::Headless});
        if let Some(proxy) = &self.proxy {
            result["proxy"] = json!(proxy);
        }
        if self.engine == Engine::Firefox && self.mode == Mode::Headed {
            result["firefoxUserPrefs"] =
                json!({"layout.css.devPixelsPerPx":"1.0","browser.uidensity":2});
        }
        if cfg!(windows) && self.engine == Engine::Firefox && self.mode == Mode::Headless {
            result["firefoxUserPrefs"] = json!({
                "gfx.webrender.software":true,
                "gfx.webrender.software.d3d11":false,
                "gfx.webrender.compositor":false,
                "layers.gpu-process.enabled":false
            });
        }
        result
    }
}

/// Issued public identities are independent of driver GUIDs and cannot be
/// accepted without checking the live owner/connection/resource table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reference {
    pub connection_id: Uuid,
    pub id: Uuid,
    pub generation: Uuid,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    HttpRequestInputsSet {
        context: Reference,
        enabled: bool,
    },
    HttpFetchRequest {
        #[serde(rename = "httpContext")]
        http_context: Reference,
        #[serde(rename = "sourceRequestId")]
        source_request_id: Uuid,
        #[serde(default)]
        options: super::http::request_inputs::Overrides,
    },
    HttpRequestInputRemove {
        #[serde(rename = "sourceRequestId")]
        source_request_id: Uuid,
    },
    HttpContextCreate {
        #[serde(default)]
        options: super::http::Options,
    },
    HttpContextList {},
    HttpContextStorageState {
        #[serde(rename = "httpContext")]
        http_context: Reference,
        #[serde(default)]
        options: super::http::StorageStateOptions,
    },
    HttpEvidenceFinalize {
        #[serde(rename = "captureId")]
        capture_id: Uuid,
    },
    HttpContextRemove {
        #[serde(rename = "httpContext")]
        http_context: Reference,
    },
    HttpContextDispose {
        #[serde(rename = "httpContext")]
        http_context: Reference,
        #[serde(default)]
        options: super::http::DisposeOptions,
    },
    HttpContextRead {
        #[serde(rename = "httpContext")]
        http_context: Reference,
        #[serde(default, rename = "includeReason")]
        include_reason: bool,
        #[serde(default = "super::http::response_read_limit", rename = "maxBytes")]
        max_bytes: usize,
    },
    HttpResponseBodyRead {
        #[serde(rename = "requestId")]
        request_id: Uuid,
        format: super::http::BodyFormat,
        #[serde(default = "super::http::response_read_limit", rename = "maxBytes")]
        max_bytes: usize,
    },
    HttpResponseDispose {
        #[serde(rename = "requestId")]
        request_id: Uuid,
    },
    HttpContextCookiesClear {
        #[serde(rename = "httpContext")]
        http_context: Reference,
        #[serde(default)]
        options: super::http::CookieFilter,
    },
    HttpContextCookiesAdd {
        #[serde(rename = "httpContext")]
        http_context: Reference,
        cookies: Vec<Value>,
    },
    HttpContextCookiesRead {
        #[serde(rename = "httpContext")]
        http_context: Reference,
        #[serde(default)]
        urls: super::http::CookieUrls,
    },
    HttpFetch {
        #[serde(rename = "httpContext")]
        http_context: Reference,
        request: super::http::Fetch,
    },
    /// Pinned `route.fetch()`: send a pending route callback's request through an
    /// HTTP client; the callback stays pending for its own reply.
    RouteFetch {
        callback: Reference,
        #[serde(rename = "httpContext")]
        http_context: Reference,
        #[serde(default, rename = "maxRedirects")]
        max_redirects: Option<u32>,
        #[serde(default, rename = "timeoutMs")]
        timeout_ms: Option<u64>,
        #[serde(default, rename = "maxRetries")]
        max_retries: Option<u32>,
        /// Replaces the paused request's URL, method, headers or body for this fetch only.
        #[serde(default)]
        overrides: Option<crate::browser::routing::Overrides>,
    },
    HttpResponseFinalize {
        #[serde(rename = "requestId")]
        request_id: Uuid,
    },
    HttpResponseDiscard {
        #[serde(rename = "requestId")]
        request_id: Uuid,
    },
    HttpResponseRead {
        #[serde(rename = "requestId")]
        request_id: Uuid,
        #[serde(default, rename = "includeHeadersObject")]
        include_headers_object: bool,
        #[serde(rename = "maxBytes", default = "super::http::response_read_limit")]
        max_bytes: usize,
    },
    HttpResponseRemove {
        #[serde(rename = "requestId")]
        request_id: Uuid,
    },
    HttpEvidenceList {},
    HttpEvidenceRead {
        #[serde(rename = "evidenceId")]
        evidence_id: Uuid,
        format: super::http::BodyFormat,
        #[serde(rename = "maxBytes", default = "super::http::response_read_limit")]
        max_bytes: usize,
    },
    HttpEvidenceRemove {
        #[serde(rename = "evidenceId")]
        evidence_id: Uuid,
    },
    WebSocketRouteAdd {
        scope: Reference,
        url: String,
        #[serde(default = "super::socket_routes::sdk_defaults", rename = "sdkDefaults")]
        sdk_defaults: bool,
    },
    WebSocketRouteAddMatcher {
        scope: Reference,
        matcher: super::socket_routes::Matcher,
        #[serde(default = "super::socket_routes::sdk_defaults", rename = "sdkDefaults")]
        sdk_defaults: bool,
    },
    WebSocketRouteList {
        scope: Reference,
    },
    WebSocketRouteSend {
        scope: Reference,
        frame: Reference,
        #[serde(rename = "routeId")]
        route_id: Uuid,
        #[serde(rename = "socketId")]
        socket_id: String,
        to: WebSocketDestination,
        message: WebSocketMessage,
    },
    WebSocketRouteClose {
        scope: Reference,
        frame: Reference,
        #[serde(rename = "routeId")]
        route_id: Uuid,
        #[serde(rename = "socketId")]
        socket_id: String,
        to: WebSocketDestination,
        #[serde(default)]
        options: super::socket_routes::CloseOptions,
    },
    WebSocketRouteHandlers {
        scope: Reference,
        frame: Reference,
        #[serde(rename = "routeId")]
        route_id: Uuid,
        #[serde(rename = "socketId")]
        socket_id: String,
        handlers: super::socket_routes::Handlers,
    },
    WebSocketRouteRemove {
        scope: Reference,
        #[serde(rename = "routeId")]
        route_id: Uuid,
    },
    BindingAdd {
        scope: Reference,
        name: String,
    },
    BindingList {
        scope: Reference,
    },
    BindingRemove {
        scope: Reference,
        #[serde(rename = "bindingId")]
        binding_id: Uuid,
    },
    BindingRead {
        callback: Reference,
    },
    EventsRead {
        context: Reference,
        #[serde(default)]
        cursor: Option<String>,
        #[serde(default = "event_limit")]
        limit: usize,
    },
    EventsWait {
        context: Reference,
        after: u64,
        conditions: Vec<super::activity::Condition>,
    },
    StorageRegister {
        path: String,
    },
    StorageExport {
        context: Reference,
        #[serde(default)]
        options: super::storage::Options,
    },
    StorageApply {
        context: Reference,
        snapshot: Reference,
        mode: super::storage::ApplyMode,
        /// Required for a snapshot carrying virtual credentials: restoring them
        /// installs the virtual authenticator in place of real ones.
        #[serde(default, rename = "installAuthenticator")]
        install_authenticator: bool,
    },
    StorageForget {
        snapshot: Reference,
    },
    /// Pinned `BrowserContext.cookies(urls)`, one keyset page at a time.
    ContextCookiesRead {
        context: Reference,
        #[serde(default)]
        urls: super::http::CookieUrls,
        #[serde(default)]
        cursor: Option<String>,
        #[serde(default)]
        limit: Option<u64>,
    },
    /// Pinned `BrowserContext.addCookies`.
    ContextCookiesAdd {
        context: Reference,
        cookies: Vec<Value>,
    },
    /// Pinned `BrowserContext.clearCookies`, optionally filtered.
    ContextCookiesClear {
        context: Reference,
        #[serde(default)]
        options: super::http::CookieFilter,
    },
    /// Pinned `WebStorage` reads on a page's main frame.
    PageWebStorageRead {
        page: Reference,
        kind: super::storage::web::Kind,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        cursor: Option<String>,
        #[serde(default)]
        limit: Option<u64>,
    },
    /// Pinned `WebStorage` `setItem`/`removeItem`/`clear` on a page's main frame.
    PageWebStorageUpdate {
        page: Reference,
        kind: super::storage::web::Kind,
        update: super::storage::web::Update,
    },
    /// Native Chrome Cache Storage: an origin's caches, one cache's entries,
    /// or one entry's response.
    ContextCacheStorageRead {
        context: Reference,
        origin: String,
        #[serde(default, rename = "cacheName")]
        cache_name: Option<String>,
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        cursor: Option<String>,
        #[serde(default)]
        limit: Option<u64>,
    },
    /// Native Chrome Cache Storage: delete one cache or one of its entries.
    ContextCacheStorageDelete {
        context: Reference,
        origin: String,
        #[serde(rename = "cacheName")]
        cache_name: String,
        #[serde(default)]
        url: Option<String>,
    },
    Batch {
        requests: Vec<Request>,
    },
    ContextClock {
        context: Reference,
        action: super::clock::Action,
    },
    /// Virtual WebAuthn authenticator of an owned context.
    ContextCredentials {
        context: Reference,
        action: super::credentials::Action,
    },
    /// Credential brokers in the private broker directory, without values.
    AuthBrokerList {
        #[serde(default)]
        cursor: Option<String>,
        #[serde(default)]
        limit: Option<u64>,
    },
    /// Trusted sign-in: broker values entered into the journey's origin.
    AuthSignIn {
        page: Reference,
        broker: String,
        account: String,
        journey: super::authentication::Journey,
        #[serde(default, rename = "expectedIdentity")]
        expected_identity: Option<String>,
    },
    /// Read-only sign-in state of the page's document.
    AuthStatus {
        page: Reference,
        journey: super::authentication::Journey,
        #[serde(default, rename = "expectedIdentity")]
        expected_identity: Option<String>,
    },
    ContextConfigure {
        context: Reference,
        setting: super::settings::ContextSetting,
    },
    PageEmulateMedia {
        page: Reference,
        options: super::settings::MediaOptions,
    },
    /// Chrome-only page CPU and network throttling.
    PageEmulateThrottling {
        page: Reference,
        options: super::settings::ThrottlingOptions,
    },
    PageSetExtraHttpHeaders {
        page: Reference,
        headers: Vec<super::http::Header>,
    },
    /// SDK `Page.requestGC()`: ask the page's JavaScript engine to collect garbage.
    PageRequestGc {
        page: Reference,
    },
    /// One page of function timings from a saved `v8-cpu-profile` report.
    ProfileQuery {
        #[serde(rename = "evidenceId")]
        evidence_id: Uuid,
        query: super::profiling::cpu::Query,
    },
    PageInput {
        page: Reference,
        #[serde(default)]
        observation: Option<Reference>,
        actions: Vec<super::input::Action>,
    },
    InputRelease {},
    PageResize {
        page: Reference,
        viewport: Viewport,
    },
    PreviewStart {
        page: Reference,
        #[serde(default)]
        options: super::preview::Options,
    },
    PreviewStatus {
        preview: Reference,
    },
    PreviewStop {
        preview: Reference,
    },
    RecordingStart {
        page: Reference,
        #[serde(default)]
        options: super::recording::Options,
    },
    RecordingStatus {
        recording: Reference,
    },
    RecordingStop {
        recording: Reference,
    },
    DiagnosticsStart {
        context: Reference,
    },
    PageRouteSet {
        page: Reference,
        url: Option<String>,
        #[serde(default, rename = "authenticationChallenges")]
        authentication_challenges: bool,
    },
    NetworkRoute {
        scope: Reference,
        action: super::routing::HandlerAction,
    },
    ContextRouteSet {
        context: Reference,
        url: Option<String>,
        #[serde(default, rename = "authenticationChallenges")]
        authentication_challenges: bool,
    },
    PageHarSet {
        page: Reference,
        #[serde(default, rename = "urlFilter")]
        url_filter: Option<super::routing::Matcher>,
        #[serde(rename = "archiveId")]
        archive_id: Option<Uuid>,
        #[serde(default, rename = "notFound")]
        not_found: HarNotFound,
    },
    ContextHarSet {
        context: Reference,
        #[serde(default, rename = "urlFilter")]
        url_filter: Option<super::routing::Matcher>,
        #[serde(rename = "archiveId")]
        archive_id: Option<Uuid>,
        #[serde(default, rename = "notFound")]
        not_found: HarNotFound,
    },
    PageInitScriptAdd {
        page: Reference,
        source: String,
    },
    PageInitScriptList {
        page: Reference,
    },
    PageInitScriptRemove {
        page: Reference,
        #[serde(rename = "scriptId")]
        script_id: Uuid,
    },
    ContextInitScriptAdd {
        context: Reference,
        source: String,
    },
    ContextInitScriptList {
        context: Reference,
    },
    ContextInitScriptRemove {
        context: Reference,
        #[serde(rename = "scriptId")]
        script_id: Uuid,
    },
    ContextFrameworkPrepare {
        context: Reference,
    },
    NetworkRead {
        context: Reference,
        #[serde(rename = "requestId")]
        request_id: Uuid,
        #[serde(default)]
        options: super::network::Read,
    },
    NetworkEvidenceList {},
    HarRegister {
        path: String,
    },
    HarList {},
    HarCaptureStart {
        context: Reference,
        #[serde(default)]
        page: Option<Reference>,
        #[serde(default, rename = "urlFilter")]
        url_filter: Option<super::routing::Matcher>,
        /// Omitted: `attach` for a re-record (pinned `routeFromHAR` update), otherwise `omit`.
        #[serde(default)]
        content: Option<HarCaptureContent>,
        #[serde(default)]
        mode: HarCaptureMode,
        /// Also write the published archive to this new private `.har`/`.zip` file.
        #[serde(default)]
        path: Option<String>,
        #[serde(default, rename = "resourcesDir")]
        resources_dir: Option<String>,
        #[serde(default, rename = "replacesArchiveId")]
        replaces_archive_id: Option<Uuid>,
    },
    HarCaptureStatus {
        context: Reference,
    },
    HarCaptureStop {
        context: Reference,
    },
    HarFinalize {
        #[serde(rename = "archiveId")]
        archive_id: Uuid,
    },
    HarRemove {
        #[serde(rename = "archiveId")]
        archive_id: Uuid,
    },
    NetworkEvidenceRemove {
        #[serde(rename = "evidenceId")]
        evidence_id: Uuid,
    },
    DeveloperOpen {
        page: Reference,
        #[serde(default)]
        panel: Option<String>,
    },
    DeveloperStatus {
        developer: Reference,
    },
    DeveloperClose {
        developer: Reference,
    },
    PageAudit {
        page: Reference,
        #[serde(default)]
        options: super::audits::Options,
    },
    ProfileStart {
        page: Reference,
        #[serde(default)]
        options: super::profiling::Options,
    },
    ProfileStatus {
        page: Reference,
    },
    ProfileStop {
        page: Reference,
    },
    ProfileTakeCoverage {
        page: Reference,
    },
    HeapCapture {
        page: Reference,
        #[serde(default = "super::heap::default_bytes", rename = "maxBytes")]
        max_bytes: usize,
    },
    HeapStop {
        page: Reference,
    },
    PerformanceLoad {
        developer: Reference,
        #[serde(rename = "evidenceId")]
        evidence_id: Uuid,
    },
    PerformanceQuery {
        developer: Reference,
        #[serde(rename = "analysisId")]
        analysis_id: Uuid,
        query: super::performance::Query,
    },
    PerformanceRelease {
        developer: Reference,
        #[serde(rename = "analysisId")]
        analysis_id: Uuid,
    },
    HeapImport {
        developer: Reference,
        path: String,
        #[serde(default = "super::heap::default_bytes", rename = "maxBytes")]
        max_bytes: usize,
        #[serde(default, rename = "identityGroup")]
        identity_group: Option<Uuid>,
    },
    HeapLoad {
        developer: Reference,
        #[serde(rename = "evidenceId")]
        evidence_id: Uuid,
    },
    HeapQuery {
        developer: Reference,
        #[serde(rename = "snapshotId")]
        snapshot_id: Uuid,
        query: super::heap::Query,
        #[serde(default = "super::heap::default_name_length", rename = "maxNameLength")]
        max_name_length: usize,
    },
    HeapCompare {
        developer: Reference,
        #[serde(rename = "snapshotId")]
        snapshot_id: Uuid,
        #[serde(rename = "baseSnapshotId")]
        base_snapshot_id: Uuid,
        #[serde(default, rename = "classIndex")]
        class_index: Option<usize>,
        #[serde(default = "super::heap::default_name_length", rename = "maxNameLength")]
        max_name_length: usize,
        #[serde(default)]
        offset: usize,
        #[serde(default = "super::heap::default_limit")]
        limit: usize,
    },
    HeapRelease {
        developer: Reference,
        #[serde(rename = "snapshotId")]
        snapshot_id: Uuid,
    },
    DeveloperEvidenceList {},
    DeveloperEvidenceFinalize {
        #[serde(rename = "captureId")]
        capture_id: Uuid,
    },
    PageDebugger {
        page: Reference,
        action: super::debugging::Action,
    },
    ContextDebugger {
        context: Reference,
        action: super::automation_debugger::Action,
    },
    TraceStart {
        context: Reference,
        #[serde(default)]
        options: super::tracing::Options,
    },
    TraceStatus {},
    TraceStop {
        #[serde(default, rename = "filePath", skip_serializing_if = "Option::is_none")]
        file_path: Option<String>,
        #[serde(
            default,
            rename = "analysisDeveloper",
            skip_serializing_if = "Option::is_none"
        )]
        analysis_developer: Option<Reference>,
        #[serde(
            default,
            rename = "analysisPage",
            skip_serializing_if = "Option::is_none"
        )]
        analysis_page: Option<Reference>,
    },
    TraceChunkStart {
        #[serde(default = "super::tracing::default_title")]
        title: String,
        #[serde(default)]
        name: Option<String>,
    },
    TraceChunkStop {},
    TraceGroupStart {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        location: Option<super::automation_debugger::Location>,
    },
    TraceGroupEnd {},
    DeveloperEvidenceRemove {
        #[serde(rename = "evidenceId")]
        evidence_id: Uuid,
    },
    DiagnosticsStop {
        context: Reference,
    },
    DiagnosticsRead {
        context: Reference,
        #[serde(default)]
        query: super::diagnostics::Query,
    },
    DiagnosticsArgumentsSet {
        context: Reference,
        enabled: bool,
    },
    DiagnosticsArgumentsList {
        context: Reference,
    },
    DiagnosticsArgumentsRead {
        context: Reference,
        #[serde(rename = "groupId")]
        group_id: Uuid,
        #[serde(default)]
        query: super::diagnostic_arguments::Read,
    },
    DiagnosticsArgumentsRemove {
        context: Reference,
        #[serde(rename = "groupId")]
        group_id: Uuid,
    },
    DiagnosticsWait {
        context: Reference,
        after: u64,
        conditions: Vec<super::diagnostics::EventCondition>,
    },
    Evaluate {
        scope: Reference,
        expression: String,
        #[serde(default, rename = "isFunction")]
        is_function: Option<bool>,
        #[serde(default)]
        argument: super::evaluation::EvaluationValue,
        #[serde(default, rename = "returnHandle")]
        return_handle: bool,
    },
    SelectorEvaluate {
        scope: Reference,
        options: super::evaluation::SelectorOptions,
    },
    HandleRead {
        handle: Reference,
    },
    HandleOwnerFrame {
        handle: Reference,
    },
    HandleContentFrame {
        handle: Reference,
    },
    FrameElement {
        frame: Reference,
    },
    /// Retain a frame's identity across its document changes (SDK `Frame`).
    FrameRetain {
        frame: Reference,
    },
    FrameOwnerRelease {
        frame: Reference,
    },
    WorkerList {
        page: Reference,
    },
    ContextWorkerList {
        context: Reference,
    },
    HandleProperty {
        handle: Reference,
        name: String,
    },
    HandleProperties {
        handle: Reference,
        #[serde(default)]
        query: super::evaluation::PropertyQuery,
    },
    FixtureRegister {
        fixture: super::transfer::Registration,
    },
    DirectoryRegister {
        name: String,
        entries: Vec<super::directory::Entry>,
    },
    FixtureList {},
    FixtureVerify {
        fixture: Reference,
    },
    PageUpload {
        page: Reference,
        #[serde(default)]
        frame: Option<Reference>,
        locator: super::page::Locator,
        fixtures: Vec<Reference>,
    },
    ChooserResolve {
        chooser: Reference,
        fixtures: Vec<Reference>,
    },
    DownloadList {},
    DownloadStatus {
        download: Reference,
    },
    DownloadCancel {
        download: Reference,
    },
    DownloadDelete {
        download: Reference,
    },
    PageNavigate {
        page: Reference,
        navigation: super::page::Navigation,
    },
    PageRead {
        page: Reference,
        #[serde(default)]
        frame: Option<Reference>,
        #[serde(default)]
        locator: Option<super::page::Locator>,
        read: super::page::Read,
        #[serde(default = "read_limit", rename = "maxBytes")]
        max_bytes: usize,
    },
    PageAction {
        page: Reference,
        #[serde(default)]
        frame: Option<Reference>,
        locator: super::page::Locator,
        action: super::page::Action,
        #[serde(default)]
        force: bool,
        #[serde(default)]
        postcondition: Option<super::page::Condition>,
    },
    PageReadUrl {
        page: Reference,
        url: String,
        #[serde(default = "read_limit", rename = "maxBytes")]
        max_bytes: usize,
    },
    PageClipboard {
        page: Reference,
        origin: String,
        action: super::clipboard::Action,
    },
    FrameworkPrepare {
        page: Reference,
    },
    FrameworkOpen {
        scope: Reference,
    },
    FrameworkQuery {
        registry: Reference,
        query: super::framework::Query,
    },
    FrameworkRelease {
        registry: Reference,
    },
    DeveloperToolsDiscover {
        scope: Reference,
    },
    WebMcpDebug {
        scope: Reference,
        action: super::webmcp_debug::Action,
    },
    DeveloperToolCall {
        registry: Reference,
        #[serde(rename = "toolId")]
        tool_id: String,
        #[serde(default = "tool_parameters")]
        parameters: Value,
        #[serde(default = "tool_handle", rename = "returnHandle")]
        return_handle: bool,
    },
    DeveloperToolsRelease {
        registry: Reference,
    },
    DeveloperToolResultRead {
        handle: Reference,
    },
    PageCss {
        developer: Reference,
        page: Reference,
        #[serde(default)]
        frame: Option<Reference>,
        locator: super::page::Locator,
        #[serde(default)]
        offset: usize,
        #[serde(default = "css_limit")]
        limit: usize,
    },
    PageStyles {
        page: Reference,
        #[serde(default)]
        frame: Option<Reference>,
        locator: super::page::Locator,
        #[serde(default)]
        properties: Vec<String>,
        #[serde(default = "read_limit", rename = "maxBytes")]
        max_bytes: usize,
    },
    PageWait {
        page: Reference,
        #[serde(default)]
        frame: Option<Reference>,
        #[serde(default)]
        locator: Option<super::page::Locator>,
        condition: super::page::Condition,
    },
    FrameEnter {
        page: Reference,
        #[serde(default)]
        frame: Option<Reference>,
        locator: super::page::Locator,
    },
    PageScreenshot {
        page: Reference,
        #[serde(default)]
        capture: super::page::Screenshot,
    },
    CaptureCompare {
        baseline: Uuid,
        actual: Uuid,
        #[serde(default)]
        threshold: u8,
        #[serde(default, rename = "maxChangedRatio")]
        max_changed_ratio: f64,
    },
    PageSnapshot {
        page: Reference,
        #[serde(default)]
        frame: Option<Reference>,
        #[serde(default)]
        locator: Option<super::page::Locator>,
    },
    PageAnnotate {
        page: Reference,
        markers: Vec<super::evidence::Marker>,
    },
    PagePdf {
        page: Reference,
        #[serde(default)]
        options: super::evidence::PdfOptions,
    },
    ContextCreate {
        #[serde(default, deserialize_with = "super::devices::context_options")]
        options: ContextOptions,
    },
    ContextList {},
    ContextSelect {
        context: Reference,
    },
    ContextClose {
        context: Reference,
    },
    PageCreate {
        context: Reference,
        #[serde(default)]
        select: bool,
    },
    PageList {},
    PageSelect {
        page: Reference,
    },
    PageClose {
        page: Reference,
    },
    HandleCreate {
        page: Reference,
        expression: String,
    },
    HandleDispose {
        handle: Reference,
    },
    CallbackResolve {
        callback: Reference,
        response: CallbackResponse,
    },
    WebMcp {
        scope: Reference,
        action: super::webmcp_debug::PageAction,
    },
    BrowserExtension {
        action: super::extensions::Action,
    },
    BrowserPwa {
        action: super::pwa::Action,
    },
}

fn event_limit() -> usize {
    50
}
fn tool_parameters() -> Value {
    json!({})
}
fn tool_handle() -> bool {
    true
}
fn css_limit() -> usize {
    10
}
fn read_limit() -> usize {
    8192
}
/// A present `json` field, including an explicit `null`, as the SDK's `!== undefined`.
fn present_json<'de, D: serde::Deserializer<'de>>(input: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(input).map(Some)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CallbackResponse {
    Dialog {
        accept: bool,
        #[serde(default, rename = "promptText")]
        prompt_text: Option<String>,
    },
    RouteAbort {
        #[serde(default, rename = "errorCode")]
        error_code: Option<super::routing::AbortCode>,
    },
    RouteContinue {},
    RouteFallback {},
    RouteFallbackWith {
        overrides: super::routing::Overrides,
    },
    RouteContinueWith {
        overrides: RouteOverrides,
    },
    /// Pinned `route.fulfill` text form: status defaults to 200; `json` excludes `body`.
    RouteFulfill {
        #[serde(default)]
        status: Option<u16>,
        #[serde(default)]
        body: Option<String>,
        #[serde(default, deserialize_with = "present_json")]
        json: Option<Value>,
        #[serde(default, rename = "contentType")]
        content_type: Option<String>,
        #[serde(default)]
        headers: Vec<RouteHeader>,
        /// Caller file served as the body; relative paths bind to the caller.
        #[serde(default)]
        path: Option<String>,
        /// The file's snapshot, read once by the backend before dispatch.
        #[serde(skip)]
        file: Option<super::routing::FulfillFile>,
    },
    RouteFulfillBinary {
        status: u16,
        #[serde(rename = "bodyBase64")]
        body_base64: String,
        #[serde(default)]
        headers: Vec<RouteHeader>,
    },
    RouteFulfillEvidence {
        status: u16,
        #[serde(rename = "networkEvidenceId")]
        network_evidence_id: Uuid,
        #[serde(default)]
        headers: Vec<RouteHeader>,
    },
    /// Fulfill from an HTTP client response, such as one from `route_fetch`.
    RouteFulfillHttpEvidence {
        /// Defaults to the fetched response's status.
        #[serde(default)]
        status: Option<u16>,
        #[serde(rename = "httpEvidenceId")]
        http_evidence_id: Uuid,
        #[serde(default)]
        headers: Vec<RouteHeader>,
    },
    RouteHar {
        #[serde(rename = "archiveId")]
        archive_id: Uuid,
        #[serde(default, rename = "notFound")]
        not_found: HarNotFound,
    },
    BindingReject {},
    AuthCancel {},
    AuthDefault {},
    AuthCredentials {
        username: String,
        password: String,
    },
    BindingResolve {
        value: Value,
    },
    BindingResolveValue {
        value: super::evaluation::EvaluationValue,
    },
    BindingRejectValue {
        value: super::evaluation::EvaluationValue,
    },
}

/// Canonical input schemas for library composition. Publishing these as CLI/MCP
/// tools is separate from the owned-runtime implementation.
fn context_schema() -> Value {
    let mut context = object(
        json!({"viewport":object(json!({"width":{"type":"integer","minimum":1,"maximum":8192},"height":{"type":"integer","minimum":1,"maximum":8192}}), &["width","height"]),"deviceScaleFactor":{"type":"number","minimum":0.5,"maximum":4},"acceptDownloads":{"type":"boolean","default":false,"description":"Enable downloads for this context before starting them"},"hasTouch":{"type":"boolean","default":false}}),
        &[],
    );
    // A pinned device descriptor supplies the layout fields it names.
    context
        .as_object_mut()
        .expect("Context schema")
        .remove("required");
    context["anyOf"] =
        json!([{"required":["viewport","deviceScaleFactor"]},{"required":["device"]}]);
    context["properties"]["device"] = json!({"type":"string","minLength":1,"maxLength":64,"description":"Device name from capabilities settings.devices; explicit options override it"});
    context["properties"]["screen"] = context["properties"]["viewport"].clone();
    for (field, schema) in [
        (
            "isMobile",
            json!({"type":"boolean","description":"Mobile viewport emulation on Chrome and WebKit; true is refused on Firefox. Touch is selected separately."}),
        ),
        (
            "userAgent",
            json!({"type":"string","minLength":1,"maxLength":512}),
        ),
        ("extraHTTPHeaders", super::http::header_schema()),
        ("httpCredentials", super::http::Credentials::schema()),
        (
            "clientCertificates",
            super::http::client_certificates::schema(),
        ),
        ("proxy", ContextProxy::schema()),
        (
            "locale",
            json!({"type":"string","minLength":1,"maxLength":128}),
        ),
        (
            "timezoneId",
            json!({"type":"string","minLength":1,"maxLength":128}),
        ),
        (
            "javaScriptEnabled",
            json!({"type":"boolean","default":true}),
        ),
        (
            "bypassCSP",
            json!({"type":"boolean","default":false,"description":"Explicitly bypass the owned context's content security policy"}),
        ),
        (
            "ignoreHTTPSErrors",
            json!({"type":"boolean","default":false,"description":"Explicitly ignore TLS certificate errors in the owned context"}),
        ),
        ("offline", json!({"type":"boolean","default":false})),
        (
            "colorScheme",
            json!({"enum":["dark","light","no-preference","no-override"]}),
        ),
        (
            "reducedMotion",
            json!({"enum":["reduce","no-preference","no-override"]}),
        ),
        (
            "forcedColors",
            json!({"enum":["active","none","no-override"]}),
        ),
        (
            "contrast",
            json!({"enum":["more","no-preference","no-override"]}),
        ),
        ("geolocation", super::settings::geolocation_schema()),
        ("permissions", super::settings::permission_schema()),
        ("serviceWorkers", json!({"enum":["allow","block"]})),
        (
            "baseURL",
            json!({"type":"string","minLength":1,"maxLength":2048,"description":"Absolute HTTP(S) base for relative page_navigate URLs, route/HAR/capture URL matchers and WebSocket routes in this context"}),
        ),
        (
            "recordHar",
            json!({"type":"object","properties":{"content":{"enum":["omit","embed","attach"],"default":"omit"},"mode":{"enum":["minimal","full"],"default":"full"},"urlFilter":{"anyOf":[super::routing::Matcher::schema(),{"type":"null"}]},"path":{"type":["string","null"],"maxLength":4096,"description":"New absolute .har or .zip file in a private directory to also receive the archive"}},"additionalProperties":false,"description":"Capture this context's HTTP(S) traffic from creation; context_close publishes the private archive"}),
        ),
        (
            "storageState",
            json!({"anyOf":[{"type":"string","minLength":1,"maxLength":4096},{"type":"object"}]}),
        ),
    ] {
        context["properties"][field] = schema;
    }
    context
}

pub fn schemas() -> Value {
    static SCHEMAS: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
    SCHEMAS.get_or_init(build_schemas).clone()
}
#[expect(
    clippy::too_many_lines,
    reason = "One schema assembly owns the shared definitions and bounded nonrecursive request graph"
)]
fn build_schemas() -> Value {
    let reference_schema = object(
        json!({"connectionId":{"type":"string","format":"uuid"},"id":{"type":"string","format":"uuid"},"generation":{"type":"string","format":"uuid"}}),
        &["connectionId", "id", "generation"],
    );
    let reference = json!({"$ref":"#/$defs/browserReference"});
    let context = context_schema();
    let mut requests = Vec::new();
    requests.extend(super::page::schemas(&reference));
    requests.extend(super::input::schemas(&reference));
    requests.extend(super::settings::schemas(&reference));
    requests.extend(super::storage::schemas(&reference));
    requests.push(super::clock::schema(&reference));
    requests.extend(super::transfer::schemas(&reference));
    requests.extend(super::evaluation::schemas(&reference));
    requests.extend(super::diagnostics::schemas(&reference));
    requests.extend(super::diagnostic_arguments::schemas(&reference));
    requests.extend(super::network::schemas(&reference));
    requests.extend(super::har::schemas(&reference));
    requests.extend(super::scripts::schemas(&reference));
    requests.extend(super::bindings::schemas(&reference));
    requests.extend(super::socket_routes::schemas(&reference));
    requests.extend(super::http::schemas(&reference));
    requests.push(super::routing::schema(&reference));
    requests.push(super::routing::context_schema(&reference));
    requests.push(super::routing::handlers_schema(&reference));
    requests.push(super::audits::schema(&reference));
    requests.extend(super::profiling::schemas(&reference));
    requests.extend(super::heap::schemas(&reference));
    requests.extend(super::performance::schemas(&reference));
    requests.extend(super::developer_tools::schemas(&reference));
    requests.push(super::webmcp_debug::schema(&reference, true));
    requests.extend(super::framework::schemas(&reference));
    requests.extend(super::tracing::schemas(&reference));
    requests.extend(super::automation_debugger::schemas(&reference));
    requests.push(super::debugging::schema(&reference));
    requests.push(object(
        json!({"operation":{"const":"developer_evidence_list"}}),
        &["operation"],
    ));
    requests.push(object(json!({"operation":{"const":"developer_evidence_remove"},"evidenceId":{"type":"string","format":"uuid"}}),&["operation","evidenceId"]));
    requests.push(object(json!({"operation":{"const":"developer_evidence_finalize"},"captureId":{"type":"string","format":"uuid"}}),&["operation","captureId"]));
    requests.push(object(json!({"operation":{"const":"developer_open"},"page":reference,"panel":{"enum":["elements","console","network","sources","resources","timeline","chrome-recorder","heap-profiler","lighthouse","security",null]}}),&["operation","page"]));
    for operation in ["developer_status", "developer_close"] {
        requests.push(object(
            json!({"operation":{"const":operation},"developer":reference}),
            &["operation", "developer"],
        ));
    }
    requests.extend(super::activity::schemas(&reference));
    requests.push(object(
        json!({"operation":{"const":"context_worker_list"},"context":reference}),
        &["operation", "context"],
    ));
    requests.extend(super::preview::schemas(&reference));
    requests.extend(super::recording::schemas(&reference));
    requests.push(object(json!({"operation":{"const":"page_resize"},"page":reference,"viewport":object(json!({"width":{"type":"integer","minimum":1,"maximum":8192},"height":{"type":"integer","minimum":1,"maximum":8192}}),&["width","height"])}),&["operation","page","viewport"]));
    for (operation, field) in [
        ("context_list", None),
        ("page_list", None),
        ("context_select", Some("context")),
        ("context_close", Some("context")),
        ("page_select", Some("page")),
        ("page_close", Some("page")),
        ("handle_dispose", Some("handle")),
    ] {
        let mut properties = json!({"operation":{"const":operation}});
        let mut required = vec!["operation"];
        if let Some(field) = field {
            properties[field] = reference.clone();
            required.push(field);
        }
        requests.push(object(properties, &required));
    }
    requests.push(object(
        json!({"operation":{"const":"context_create"},"options":context.clone()}),
        &["operation"],
    ));
    requests.push(object(json!({"operation":{"const":"page_create"},"context":reference.clone(),"select":{"type":"boolean","default":false}}), &["operation","context"]));
    requests.push(object(json!({"operation":{"const":"handle_create"},"page":reference.clone(),"expression":{"type":"string","minLength":1,"maxLength":8192}}), &["operation","page","expression"]));
    let responses = vec![
        object(
            json!({"kind":{"const":"dialog"},"accept":{"type":"boolean"},"promptText":{"type":["string","null"],"maxLength":8192}}),
            &["kind", "accept"],
        ),
        object(
            json!({"kind":{"const":"route_abort"},"errorCode":{"enum":super::routing::AbortCode::ALL,"default":"failed"}}),
            &["kind"],
        ),
        object(json!({"kind":{"const":"route_continue"}}), &["kind"]),
        object(json!({"kind":{"const":"route_fallback"}}), &["kind"]),
        object(
            json!({"kind":{"const":"route_fallback_with"},"overrides":super::routing::Overrides::schema()}),
            &["kind", "overrides"],
        ),
        object(
            json!({"kind":{"const":"route_continue_with"},"overrides":super::routing::Overrides::schema()}),
            &["kind", "overrides"],
        ),
        object(json!({"kind":{"const":"auth_cancel"}}), &["kind"]),
        object(json!({"kind":{"const":"auth_default"}}), &["kind"]),
        object(
            json!({"kind":{"const":"auth_credentials"},"username":{"type":"string","maxLength":1024},"password":{"type":"string","maxLength":1024}}),
            &["kind", "username", "password"],
        ),
        object(
            json!({"kind":{"const":"route_fulfill"},"status":{"type":"integer","minimum":200,"maximum":599,"default":200},"body":{"type":"string","maxLength":65536},"json":{"description":"JSON body instead of body"},"contentType":{"type":"string","minLength":1,"maxLength":256},"headers":super::routing::headers_schema(),"path":{"type":"string","minLength":1,"maxLength":4096,"description":"Host file body, at most 1 MiB"}}),
            &["kind"],
        ),
        object(
            json!({"kind":{"const":"route_fulfill_binary"},"status":{"type":"integer","minimum":200,"maximum":599},"bodyBase64":{"type":"string","maxLength":super::routing::MAX_INLINE_BODY.div_ceil(3)*4},"headers":super::routing::headers_schema()}),
            &["kind", "status", "bodyBase64"],
        ),
        object(
            json!({"kind":{"const":"route_fulfill_evidence"},"status":{"type":"integer","minimum":200,"maximum":599},"networkEvidenceId":{"type":"string","format":"uuid"},"headers":super::routing::headers_schema()}),
            &["kind", "status", "networkEvidenceId"],
        ),
        object(
            json!({"kind":{"const":"route_fulfill_http_evidence"},"status":{"type":["integer","null"],"minimum":200,"maximum":599,"description":"Defaults to the fetched response's status"},"httpEvidenceId":{"type":"string","format":"uuid"},"headers":super::routing::headers_schema()}),
            &["kind", "httpEvidenceId"],
        ),
        object(
            json!({"kind":{"const":"route_har"},"archiveId":{"type":"string","format":"uuid"},"notFound":{"enum":["abort","continue","fallback"],"default":"abort"}}),
            &["kind", "archiveId"],
        ),
        object(json!({"kind":{"const":"binding_reject"}}), &["kind"]),
        object(
            json!({"kind":{"const":"binding_resolve"},"value":{}}),
            &["kind", "value"],
        ),
        object(
            json!({"kind":{"const":"binding_resolve_value"},"value":{"$ref":"#/$defs/browserValue","description":"Typed binding success value; live handles are refused"}}),
            &["kind", "value"],
        ),
        object(
            json!({"kind":{"const":"binding_reject_value"},"value":{"$ref":"#/$defs/browserValue","description":"Typed thrown binding value; live handles are refused"}}),
            &["kind", "value"],
        ),
    ];
    requests.push(object(json!({"operation":{"const":"callback_resolve"},"callback":reference.clone(),"response":{"oneOf":responses}}), &["operation","callback","response"]));
    requests.push(super::evaluation::selector_schema(&reference));
    // Append new request schemas to preserve existing inventory addresses.
    requests.push(object(json!({"operation":{"const":"page_set_extra_http_headers"},"page":reference,"headers":super::http::header_schema()}), &["operation", "page", "headers"]));
    requests.extend(super::http::storage_state_schemas(&reference));
    requests.extend(super::socket_routes::control_schemas(&reference));
    requests.extend(super::evaluation::frame_relationship_schemas(&reference));
    requests.push(object(
        json!({"operation":{"const":"frame_element"},"frame":reference}),
        &["operation", "frame"],
    ));
    // One bounded level: a batch cannot contain another batch.
    let batch = object(
        json!({"operation":{"const":"batch"},"requests":{"type":"array","minItems":1,"maxItems":8,"items":{"$ref":"#/$defs/browserRequest"}}}),
        &["operation", "requests"],
    );
    let operation = object(
        json!({"id":{"type":"string","format":"uuid"},"outcome":{"enum":["queued","running","succeeded","failed","cancelled","unknown"]},"settled":{"type":"boolean"},"result":{},"code":{"type":["string","null"]},"cancelRequested":{"type":"boolean"}}),
        &[
            "id",
            "outcome",
            "settled",
            "result",
            "code",
            "cancelRequested",
        ],
    );
    let status = status_schema(&reference_schema, &operation);
    let mut definitions = serde_json::Map::new();
    definitions.insert("browserReference".into(), reference_schema);
    definitions.insert("browserLocator".into(), super::page::locator_schema());
    definitions.insert(
        "browserValue".into(),
        super::evaluation::value_schema(&reference),
    );
    requests.extend(super::http::lifetime_schemas(&reference));
    requests.extend(super::http::request_inputs::schemas(&reference));
    // Appended last so existing branch coordinates keep their inventory identity.
    for operation in ["frame_retain", "frame_owner_release"] {
        requests.push(object(
            json!({"operation":{"const":operation},"frame":reference}),
            &["operation", "frame"],
        ));
    }
    requests.push(object(
        json!({"operation":{"const":"page_request_gc"},"page":reference}),
        &["operation", "page"],
    ));
    requests.push(object(
        json!({"operation":{"const":"profile_query"},"evidenceId":{"type":"string","format":"uuid"},"query":super::profiling::cpu::schema()}),
        &["operation", "evidenceId", "query"],
    ));
    // Pinned addScriptTag/addStyleTag, appended last so earlier variant indices
    // stay stable; the frontend reads `path` and submits one handle evaluation.
    let tag_source = json!({"type":"string","minLength":1,"maxLength":8192});
    requests.push(object(json!({"operation":{"const":"page_add_script_tag"},"scope":reference.clone(),"url":tag_source,"path":tag_source,"content":{"type":"string","maxLength":524288},"type":{"type":"string","maxLength":128}}), &["operation","scope"]));
    requests.push(object(json!({"operation":{"const":"page_add_style_tag"},"scope":reference.clone(),"url":tag_source,"path":tag_source,"content":{"type":"string","maxLength":524288}}), &["operation","scope"]));
    requests.push(super::settings::throttling_schema(&reference));
    // Roadmap 5.2 context cookies and page WebStorage, appended last.
    requests.extend(super::storage::cookies::schemas(&reference));
    requests.extend(super::storage::web::schemas(&reference));
    requests.extend(super::storage::cache::schemas(&reference));
    // Roadmap 5.4 credentials and sign-in, appended last so the positional
    // inventory names of earlier requests stay stable.
    requests.push(super::credentials::schema(&reference));
    requests.extend(super::authentication::schemas(&reference));
    requests.push(super::webmcp_debug::schema(&reference, false));
    requests.push(super::extensions::schema(&reference));
    requests.push(super::pwa::schema(&reference));
    definitions.insert("browserRequest".into(), json!({"oneOf":requests}));
    definitions.insert(
        "authJourney".into(),
        super::authentication::journey_schema(),
    );
    // One shared definition instead of a copy in every route, HAR, wait,
    // creation, setting, cookie and paged operation; accepted inputs are unchanged.
    let page_properties = crate::pagination::properties(canonic_mcp_kit::page::Limits::STANDARD);
    for (name, schema) in [
        ("pageCondition", super::page::condition_schema()),
        ("httpRequestBody", super::http::body_schema()),
        ("urlMatcher", super::routing::Matcher::schema()),
        ("viewportCoordinate", super::input::coordinate_schema()),
        ("permissions", super::settings::permission_schema()),
        ("geolocation", super::settings::geolocation_schema()),
        ("httpCredentials", super::http::Credentials::schema()),
        ("cookie", super::http::cookie_admin::cookie_schema()),
        ("cookieFilter", super::http::CookieFilter::schema()),
        ("cookieUrls", super::storage::cookies::urls_schema()),
        ("pageCursor", page_properties["cursor"].clone()),
        ("pageLimit", page_properties["limit"].clone()),
        ("uuid", json!({"type":"string","format":"uuid"})),
        // Booleans defaulting to false and true.
        ("off", json!({"type":"boolean","default":false})),
        ("on", json!({"type":"boolean","default":true})),
        (
            "hostText",
            json!({"type":"string","minLength":1,"maxLength":4096}),
        ),
        (
            "shortText",
            json!({"type":"string","minLength":1,"maxLength":256}),
        ),
        ("text1024", json!({"type":"string","maxLength":1024})),
        // Repeated leaf values only: shared nested shapes would rename the
        // parity inventory rows inside them.
        (
            "epochMs",
            json!({"type":"integer","minimum":-8_640_000_000_000_000_i64,"maximum":8_640_000_000_000_000_i64,"description":"Unix epoch milliseconds within the JavaScript Date range"}),
        ),
        (
            "countOrNull",
            json!({"type":["integer","null"],"minimum":0,"maximum":9_007_199_254_740_991_u64}),
        ),
        (
            "count",
            json!({"type":"integer","minimum":0,"maximum":9_007_199_254_740_991_u64}),
        ),
        (
            "offset",
            json!({"type":"integer","minimum":0,"maximum":100_000,"default":0}),
        ),
        (
            "itemLimit",
            json!({"type":"integer","minimum":1,"maximum":100,"default":50}),
        ),
        (
            "loadState",
            json!({"enum":["commit","domcontentloaded","load","networkidle"],"default":"load"}),
        ),
        (
            "base64url",
            json!({"type":"string","minLength":1,"maxLength":1364,"pattern":"^[A-Za-z0-9_-]+$"}),
        ),
        (
            "textOrNull",
            json!({"type":["string","null"],"maxLength":4096}),
        ),
        (
            "name128",
            json!({"type":"string","minLength":1,"maxLength":128}),
        ),
        (
            "text8192",
            json!({"type":"string","minLength":1,"maxLength":8192}),
        ),
        ("text4096", json!({"type":"string","maxLength":4096})),
        (
            "int32Count",
            json!({"type":"integer","minimum":0,"maximum":2_147_483_647}),
        ),
        (
            "unmatchedRoute",
            json!({"enum":["abort","continue","fallback"],"default":"abort"}),
        ),
        (
            "shortTextOrNull",
            json!({"type":["string","null"],"minLength":1,"maxLength":32}),
        ),
    ] {
        let shared = json!({"$ref":format!("#/$defs/{name}")});
        for (key, value) in &mut definitions {
            // Other tools copy browserReference alone, so it stays self-contained.
            if key != "browserReference" {
                share_definition(value, &schema, &shared);
            }
        }
        definitions.insert(name.into(), schema);
    }
    let launch = object(
        json!({
            "engine":{"enum":["chrome","firefox","webkit"]},
            "mode":{"enum":["headless","headed"],"default":Mode::default(),"description":"Choose headed when you need a visible browser window, such as for user interaction."},
            "proxy":ContextProxy::schema(),
            "context":context,
            "mediaFixtures":object(json!({
                "microphonePath":{"type":"string","minLength":1,"maxLength":4096,"description":"Chrome microphone WAV on the Testrun host"},
                "cameraPath":{"type":"string","minLength":1,"maxLength":4096,"description":"Chrome camera .y4m or .mjpeg on the Testrun host"}
            }), &[]),
            "channel":{"enum":["chrome","msedge"],"description":"Installed Stable channel for engine chrome. Omit to use private Chrome for Testing."},
            "profile":{"oneOf":[
                object(json!({"kind":{"const":"retained"},"name":{"type":"string","pattern":"^[A-Za-z0-9][A-Za-z0-9_-]{0,63}$"}}), &["kind","name"]),
                object(json!({"kind":{"const":"borrowed"},"path":{"type":"string","minLength":1,"maxLength":4096}}), &["kind","path"]),
                object(json!({"kind":{"const":"disposable"}}), &["kind"])
            ]},
            "attachment":object(json!({
                "endpoint":{"type":"string","minLength":1,"maxLength":2048},
                "credentialPath":{"type":"string","minLength":1,"maxLength":4096},
                "caPath":{"type":"string","minLength":1,"maxLength":4096},
                "contextId":{"type":"string","pattern":"^[A-Za-z0-9_-]{1,128}$"}
            }), &["endpoint"])
        }),
        &["engine"],
    );
    json!({"schemaVersion":1,"launch":launch,"request":{"oneOf":[{"$ref":"#/$defs/browserRequest"},batch],"$defs":definitions},"operationResult":operation,"status":status})
}

fn share_definition(value: &mut Value, shared: &Value, reference: &Value) {
    if value == shared {
        *value = reference.clone();
        return;
    }
    match value {
        Value::Array(items) => {
            for item in items {
                share_definition(item, shared, reference);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                share_definition(item, shared, reference);
            }
        }
        _ => {}
    }
}

/// Explicit frame-lifetime authority; document references are never refreshed.
fn retained_frames(available: bool) -> Value {
    json!({"available":available,"retainOperation":"frame_retain","releaseOperation":"frame_owner_release","resourceKind":"frame_owner","lifetime":"frame","execution":"current-document-at-preparation","inFlightDocumentFence":true,"nativeScopeClosedByRelease":false,"globalResourceCapacity":MAX_RESOURCES,"fullSdkParity":false})
}
fn status_schema(reference: &Value, operation: &Value) -> Value {
    let nullable_reference = json!({"anyOf":[reference.clone(),{"type":"null"}]});
    let mut operation_summary = operation.clone();
    operation_summary["properties"]["result"] = json!({"type":"null"});
    let resource = object(
        json!({"reference":reference,"kind":{"enum":["browser","context","page","frame","frame_owner","world","handle","worker","dialog","route","binding"]},"parent":nullable_reference.clone(),"closing":{"type":"boolean"},"policyScope":reference,"handlerId":{"type":["string","null"],"format":"uuid"}}),
        &["reference", "kind", "parent"],
    );
    let status = object(
        json!({"connectionId":{"type":"string","format":"uuid"},"sessionId":{"type":"string","format":"uuid"},"sessionGeneration":{"type":"string","format":"uuid"},"backend":{"enum":["playwright","chrome_cdp"]},"engine":{"enum":["chrome","firefox","webkit"]},"phase":{"enum":["starting","ready","closing","cleanup_pending","lost","closed"]},"identity":{"type":["object","null"]},"selectedContext":nullable_reference.clone(),"selectedPage":nullable_reference,"resources":{"type":"array","maxItems":MAX_RESOURCES,"items":resource},"failure":{"type":["string","null"]},"operations":{"type":"array","maxItems":MAX_OPERATIONS,"items":operation_summary}}),
        &[
            "connectionId",
            "sessionId",
            "sessionGeneration",
            "backend",
            "engine",
            "phase",
            "identity",
            "selectedContext",
            "selectedPage",
            "resources",
            "failure",
            "operations",
        ],
    );
    let mut native_status = status.clone();
    native_status["properties"]["backend"] = json!({"const":"chrome_cdp"});
    native_status["properties"]["engine"] = json!({"const":"chrome"});
    native_status["properties"]["callbacks"] = json!({"type":"array","maxItems":64});
    native_status["properties"]["routeContinuationsPending"] =
        json!({"type":"integer","minimum":0,"maximum":64});
    native_status["properties"]["developers"] =
        json!({"type":"array","maxItems":4,"items":{"type":"object"}});
    native_status["properties"]["profiles"] =
        json!({"type":"array","maxItems":4,"items":{"type":"object"}});
    native_status["properties"]["transfers"] = json!({"type":"object"});
    native_status["properties"]["trace"] = json!({"type":"object"});
    native_status["properties"]["heap"] = json!({"type":"object"});
    native_status["properties"]["performance"] = json!({"type":"object"});
    native_status["properties"]["targetStartup"] = json!({"type":"object","properties":{"automaticAttachment":{"type":"boolean"},"pausedTargets":{"type":"integer","minimum":0,"maximum":512},"setupPending":{"type":"boolean"},"failure":{"type":["string","null"]},"resumeAcknowledgements":{"type":"object","properties":{"pages":{"type":"integer","minimum":0},"frames":{"type":"integer","minimum":0},"workers":{"type":"integer","minimum":0}},"required":["pages","frames","workers"],"additionalProperties":false}},"required":["automaticAttachment","pausedTargets","setupPending","failure","resumeAcknowledgements"],"additionalProperties":false});
    native_status["properties"]["handleCapacity"] = object(
        json!({"limit":{"const":256},"retained":{"type":"integer","minimum":0,"maximum":256},"pending":{"type":"integer","minimum":0,"maximum":256},"uncertain":{"type":"integer","minimum":0,"maximum":256}}),
        &["limit", "retained", "pending", "uncertain"],
    );
    native_status["properties"]["debuggers"] = json!({"type":"array","maxItems":4});
    native_status["properties"]["input"] = super::input::status_schema(reference);
    native_status["properties"]["resources"]["maxItems"] = json!(512);
    let mut status = status;
    status["properties"]["backend"] = json!({"const":"playwright"});
    status["properties"]["trace"] = json!({"type":"object"});
    status["properties"]["automationDebuggers"] =
        json!({"type":"array","maxItems":32,"items":{"type":"object"}});
    status["properties"]["automationDebuggerCleanup"] =
        json!({"type":"array","maxItems":32,"items":{"type":"object"}});
    status["properties"]["engine"] = json!({"enum":["firefox","webkit"]});
    status["properties"]["transfers"] = json!({"type":"object","properties":{"fixtures":{"type":"array","maxItems":64},"choosers":{"type":"array"},"downloads":{"type":"array","maxItems":32},"refused":{"type":"array","maxItems":16}},"required":["fixtures","choosers","downloads","refused"],"additionalProperties":false});
    status["required"]
        .as_array_mut()
        .expect("Status required")
        .push(json!("transfers"));
    status["properties"]["diagnostics"] = json!({"type":"object","properties":{"activeContexts":{"type":"array","maxItems":MAX_RESOURCES},"records":{"type":"integer","minimum":0,"maximum":super::diagnostics::MAX_RECORDS},"bytes":{"type":"integer","minimum":0,"maximum":super::diagnostics::MAX_BYTES},"dropped":{"type":"integer","minimum":0},"oldestCursor":{"type":"integer","minimum":0},"latestCursor":{"type":"integer","minimum":0},"forgottenRequestIdentities":{"type":"integer","minimum":0}},"additionalProperties":false,"required":["activeContexts","records","bytes","dropped","oldestCursor","latestCursor","forgottenRequestIdentities"]});
    status["properties"]["diagnostics"]["properties"]["pendingContexts"] =
        json!({"type":"array","maxItems":MAX_RESOURCES});
    status["properties"]["diagnostics"]["properties"]["forgottenWebSocketIdentities"] =
        json!({"type":"integer","minimum":0});
    status["properties"]["diagnostics"]["required"]
        .as_array_mut()
        .expect("Diagnostic required")
        .push(json!("pendingContexts"));
    status["required"]
        .as_array_mut()
        .expect("Status required")
        .push(json!("diagnostics"));
    native_status["properties"]["diagnostics"] = status["properties"]["diagnostics"].clone();
    status["properties"]["previews"] =
        json!({"type":"array","maxItems":32,"items":{"type":"object"}});
    status["properties"]["input"] = super::input::status_schema(reference);
    status["properties"]["control"] = json!({"type":"object"});
    status["properties"]["storage"] = json!({"type":"object"});
    status["required"]
        .as_array_mut()
        .expect("Status required")
        .push(json!("input"));
    status["properties"]["recordings"] =
        json!({"type":"array","maxItems":32,"items":{"type":"object"}});
    status["required"]
        .as_array_mut()
        .expect("Status required")
        .push(json!("recordings"));
    status["required"]
        .as_array_mut()
        .expect("Status required")
        .push(json!("previews"));
    for field in ["control", "previews", "recordings", "storage"] {
        native_status["properties"][field] = status["properties"][field].clone();
    }
    json!({"oneOf":[status,native_status]})
}

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_owner_discovery_and_request_schemas_preserve_host_gates_and_strict_fields() {
        for engine in [Engine::Chrome, Engine::Firefox, Engine::Webkit] {
            for supported in [false, true] {
                let value = capabilities(engine, supported);
                assert_eq!(
                    value["evaluation"]["retainedFrames"]["available"],
                    supported
                );
                assert_eq!(
                    value["evaluation"]["retainedFrames"]["nativeScopeClosedByRelease"],
                    false
                );
                assert!(
                    value["evaluation"]["scopes"]
                        .as_array()
                        .unwrap()
                        .contains(&json!("frame_owner"))
                );
            }
        }
        let frame = Reference {
            connection_id: Uuid::new_v4(),
            id: Uuid::new_v4(),
            generation: Uuid::new_v4(),
        };
        let schema = schemas()["request"].clone();
        for operation in ["frame_retain", "frame_owner_release"] {
            let value = json!({"operation":operation,"frame":frame});
            let _: Request = serde_json::from_value(value.clone()).unwrap();
            crate::schema::validate(&schema, &value).unwrap();
            let mut invalid = value.clone();
            invalid["frame"]["generation"] = json!("Acme-invalid");
            assert!(crate::schema::validate(&schema, &invalid).is_err());
            let mut invalid = value;
            invalid["closeNativeFrame"] = json!(true);
            assert!(crate::schema::validate(&schema, &invalid).is_err());
        }
    }
    #[test]
    fn frame_element_discovery_survives_final_backend_composition_and_preserves_host_gates() {
        for engine in [Engine::Chrome, Engine::Firefox, Engine::Webkit] {
            for supported in [false, true] {
                let result = capabilities(engine, supported);
                assert_eq!(
                    result["evaluation"]["selector"]["available"], supported,
                    "{engine:?}"
                );
                let relationships = &result["evaluation"]["elementRelationships"];
                assert_eq!(relationships["available"], supported, "{engine:?}");
                assert_eq!(relationships["embeddingElement"], supported, "{engine:?}");
                assert_eq!(relationships["fullSdkParity"], false);
                assert_eq!(relationships["ownerFrame"], "same-context");
                assert_eq!(relationships["contentFrame"], "source-page");
            }
        }
    }

    #[test]
    fn socket_control_discovery_matches_engine_routing_without_claiming_host_support() {
        for host in [false, true] {
            for engine in [Engine::Chrome, Engine::Firefox, Engine::Webkit] {
                let report = capabilities(engine, host);
                assert_eq!(report["webSocketRouting"]["available"], host);
                for operation in [
                    "web_socket_route_add",
                    "web_socket_route_add_matcher",
                    "web_socket_route_list",
                    "web_socket_route_send",
                    "web_socket_route_handlers",
                    "web_socket_route_close",
                    "web_socket_route_remove",
                ] {
                    let nested = report["webSocketRouting"]["operations"].as_array().unwrap();
                    assert_eq!(
                        nested.iter().filter(|value| **value == operation).count(),
                        1,
                        "{engine:?}: {operation}"
                    );
                    if engine == Engine::Chrome {
                        let registered = report["operations"].as_array().unwrap();
                        assert_eq!(
                            registered
                                .iter()
                                .filter(|value| **value == operation)
                                .count(),
                            1,
                            "Chrome discovery omits or duplicates {operation}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn context_proxy_creation_accepts_supported_settings_without_relaxing_native_refusals() {
        let options:ContextOptions=serde_json::from_value(json!({
            "viewport":{"width":640,"height":480},"deviceScaleFactor":1,
            "proxy":{"server":"127.0.0.1:8080/ignored?Acme=1","bypass":" localhost , example.com ","username":"Acme proxy","password":"proxy example"}
        })).expect("Supported pinned context proxy options must be admitted");
        for engine in [Engine::Chrome, Engine::Firefox, Engine::Webkit] {
            options.validate_for(engine).unwrap();
        }
        for field in ["userAgent", "ignoreHTTPSErrors"] {
            let mut value = serde_json::to_value(&options).unwrap();
            value[field] = if field == "userAgent" {
                json!("Acme")
            } else {
                json!(false)
            };
            let options: ContextOptions = serde_json::from_value(value).unwrap();
            options.validate_for(Engine::Chrome).unwrap();
        }
        let mut value = serde_json::to_value(&options).unwrap();
        value["permissions"] = json!(["push"]);
        let options: ContextOptions = serde_json::from_value(value).unwrap();
        assert_eq!(
            options.validate_for(Engine::Chrome),
            Err("PERMISSION_UNSUPPORTED")
        );
        options.validate_for(Engine::Firefox).unwrap();
        assert_eq!(
            options.validate_for(Engine::Webkit),
            Err("PERMISSION_UNSUPPORTED")
        );
    }
    #[test]
    fn http_header_defaults_creation_validates_before_native_allocation() {
        let mut options: ContextOptions = serde_json::from_value(json!({"viewport":{"width":640,"height":480},"deviceScaleFactor":1,"extraHTTPHeaders":[{"name":"X-Acme","value":"initial"}]})).unwrap();
        for engine in [Engine::Chrome, Engine::Firefox, Engine::Webkit] {
            options.validate_for(engine).unwrap();
        }
        super::super::schema::validate(
            "BrowserNewContextParams",
            Some(&options.wire()),
            &super::super::objects::ObjectTable::default(),
        )
        .unwrap();
        options.extra_http_headers.as_mut().unwrap()[0].value = "Acme\r\nInjected: private".into();
        assert_eq!(options.validate(), Err("HTTP_HEADER_INVALID"));
        options.extra_http_headers = Some(vec![]);
        options.user_agent = Some("Acme".into());
        options.validate_for(Engine::Chrome).unwrap();
    }
    #[test]
    fn context_creation_preserves_defaults_and_maps_explicit_native_options() {
        assert_eq!(
            ContextOptions::default().wire(),
            json!({"viewport":{"width":1280,"height":720},"deviceScaleFactor":1.0,"acceptDownloads":"deny"})
        );
        let public = json!({"viewport":{"width":640,"height":480},"deviceScaleFactor":2.0,"acceptDownloads":true,"screen":{"width":800,"height":600},"hasTouch":true,"isMobile":true,"userAgent":"Acme/1.0","locale":"en-US","timezoneId":"UTC","javaScriptEnabled":false,"bypassCSP":true,"ignoreHTTPSErrors":true,"offline":true,"colorScheme":"dark","reducedMotion":"reduce","forcedColors":"active","contrast":"more"});
        let options: ContextOptions = serde_json::from_value(public.clone()).unwrap();
        assert_eq!(
            options.validate_for(Engine::Firefox),
            Err("MOBILE_EMULATION_UNSUPPORTED")
        );
        options.validate_for(Engine::Webkit).unwrap();
        let launch = json!({"engine":"webkit","mode":"headless","context":public});
        crate::schema::validate(&schemas()["launch"], &launch).unwrap();
        let wire = options.wire();
        assert_eq!(wire["acceptDownloads"], "accept");
        assert_eq!(wire["ignoreHTTPSErrors"], true);
        super::super::schema::validate(
            "BrowserNewContextParams",
            Some(&wire),
            &super::super::objects::ObjectTable::default(),
        )
        .unwrap();
        for key in ["bypassCsp", "ignoreHttpsErrors"] {
            let mut invalid = launch["context"].clone();
            invalid[key] = json!(true);
            assert!(serde_json::from_value::<ContextOptions>(invalid).is_err());
        }
        assert!(
            ContextOptions {
                screen: Some(Viewport {
                    width: 0,
                    height: 480
                }),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            ContextOptions {
                locale: Some("en\nUS".into()),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn backend_selection_and_options_refuse_before_allocation() {
        let launch: LaunchOptions =
            serde_json::from_value(json!({"engine":"firefox","mode":"headless"})).unwrap();
        assert!(launch.validate().is_ok());
        assert_eq!(Engine::Chrome.backend(), Backend::ChromeCdp);
        assert_eq!(
            LaunchOptions {
                engine: Engine::Chromium,
                ..launch.clone()
            }
            .validate(),
            Err("BACKEND_UNAVAILABLE")
        );
        assert!(
            serde_json::from_value::<LaunchOptions>(
                json!({"engine":"firefox","mode":"headed","userDataDir":"/private/acme"})
            )
            .is_err()
        );
        assert!(
            ContextOptions {
                device_scale_factor: f64::NAN,
                ..ContextOptions::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            serde_json::from_value::<Request>(json!({"operation":"context_list","guid":"foreign"}))
                .is_err()
        );
    }
}

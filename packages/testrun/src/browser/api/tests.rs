//! Public argument translation and compatibility without browser dependencies.
use super::*;
use crate::browser::contract::{Reference, Request};
use uuid::Uuid;

#[test]
fn browser_screenshot_output_binds_to_the_caller_for_observe_and_batch() {
    let directory = tempfile::tempdir().unwrap();
    let target = Uuid::new_v4();
    let page = Reference {
        connection_id: target,
        id: Uuid::new_v4(),
        generation: Uuid::new_v4(),
    };
    let args = json!({"targetId":format!("browser:{target}"),"page":page,
        "capture":{"outputPath":"evidence/checkout.png"}});
    let (tool, call) = translate("observe", args).unwrap();
    let mut batch = json!({"request":{"operation":"batch","requests":[
        call["request"],{"operation":"page_screenshot","page":page}
    ]}});
    prepare_host_paths_with(&tool, &mut batch, || Ok(directory.path().to_owned())).unwrap();
    assert_eq!(
        batch["request"]["requests"][0]["capture"]["outputPath"],
        json!(directory.path().join("evidence/checkout.png"))
    );
    assert!(batch["request"]["requests"][1].get("capture").is_none());
    let mut invalid = call;
    invalid["request"]["capture"]["outputPath"] = json!("https://example.com/checkout.png");
    let error =
        prepare_host_paths_with(&tool, &mut invalid, || panic!("No working-directory read"))
            .unwrap_err();
    assert_eq!(error.code, "CAPTURE_OUTPUT_PATH_INVALID");
}

#[test]
fn certificate_client_paths_bind_only_selected_material_to_one_caller_directory() {
    let mut args = json!({"request":{"operation":"batch","requests":[
        {"operation":"http_context_create","options":{"clientCertificates":[{"origin":"https://example.com","certPath":"Acme link/../certificate.pem","keyPath":"Acme key.pem"}]}},
        {"operation":"context_create","options":{"clientCertificates":[{"origin":"https://example.com","pfxPath":"Acme archive.pfx"}]}},
        {"operation":"evaluate","argument":{"kind":"json","value":{"clientCertificates":[{"certPath":"application-data"}]}}}
    ]}});
    let application = args["request"]["requests"][2].clone();
    let mut reads = 0;
    prepare_host_paths_with("browser_send", &mut args, || {
        reads += 1;
        Ok("/private/Acme caller".into())
    })
    .unwrap();
    assert_eq!(reads, 1);
    assert_eq!(
        args["request"]["requests"][0]["options"]["clientCertificates"][0]["certPath"],
        "/private/Acme caller/Acme link/../certificate.pem"
    );
    assert_eq!(
        args["request"]["requests"][0]["options"]["clientCertificates"][0]["keyPath"],
        "/private/Acme caller/Acme key.pem"
    );
    assert_eq!(
        args["request"]["requests"][1]["options"]["clientCertificates"][0]["pfxPath"],
        "/private/Acme caller/Acme archive.pfx"
    );
    assert_eq!(args["request"]["requests"][2], application);
}
#[test]
fn certificate_client_paths_preserve_empty_inline_precedence_and_unrelated_arguments() {
    for (name, mut args) in [
        (
            "browser_send",
            json!({"request":{"operation":"http_context_create","options":{"clientCertificates":[{"origin":"https://example.com","cert":"","certPath":"ignored-relative-file","key":"Acme","keyPath":"ignored-relative-key"}]}}}),
        ),
        (
            "browser_status",
            json!({"context":{"clientCertificates":[{"certPath":"application-data"}]}}),
        ),
    ] {
        let before = args.clone();
        prepare_host_paths_with(name, &mut args, || {
            panic!("Unselected files must not read cwd")
        })
        .unwrap();
        assert_eq!(args, before);
    }
    let mut args = json!({"engine":"firefox","context":{"clientCertificates":[{"origin":"https://example.com","pfxPath":"Acme.pfx"}]}});
    prepare_host_paths_with("browser_connect", &mut args, || {
        Ok("/private/Acme caller".into())
    })
    .unwrap();
    assert_eq!(
        args["context"]["clientCertificates"][0]["pfxPath"],
        "/private/Acme caller/Acme.pfx"
    );
}

#[test]
fn record_har_paths_bind_relative_to_the_caller_on_connect_and_context_create() {
    let mut args = json!({"engine":"firefox","context":{"recordHar":{"path":"out/../Acme.har"}}});
    prepare_host_paths_with("browser_connect", &mut args, || {
        Ok("/private/Acme caller".into())
    })
    .unwrap();
    assert_eq!(
        args["context"]["recordHar"]["path"],
        "/private/Acme caller/Acme.har"
    );
    let mut args = json!({"request":{"operation":"context_create","options":{"recordHar":{"path":"Acme.zip","content":"attach"}}}});
    prepare_host_paths_with("browser_send", &mut args, || {
        Ok("/private/Acme caller".into())
    })
    .unwrap();
    assert_eq!(
        args["request"]["options"]["recordHar"]["path"],
        "/private/Acme caller/Acme.zip"
    );
    let mut args = json!({"request":{"operation":"context_create","options":{"recordHar":{"path":"/private/Acme.har"}}}});
    let before = args.clone();
    prepare_host_paths_with("browser_send", &mut args, || {
        panic!("Absolute recordHar path")
    })
    .unwrap();
    assert_eq!(args, before);
}

#[test]
fn route_fulfill_paths_bind_relative_to_the_caller_and_keep_parent_components() {
    let callback = json!({"connectionId":"00000000-0000-4000-8000-000000000000","id":"00000000-0000-4000-8000-000000000001","generation":1});
    let mut args = json!({"request":{"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fulfill","path":"../fixtures/Acme.html"}}});
    prepare_host_paths_with("browser_send", &mut args, || {
        Ok("/private/Acme caller".into())
    })
    .unwrap();
    assert_eq!(
        args["request"]["response"]["path"],
        "/private/Acme caller/../fixtures/Acme.html"
    );
    for response in [
        json!({"kind":"route_fulfill","path":"/private/Acme.html"}),
        json!({"kind":"route_fulfill","body":"Acme"}),
        json!({"kind":"route_fulfill_binary","status":200,"bodyBase64":"","path":"ignored"}),
    ] {
        let mut args = json!({"request":{"operation":"callback_resolve","callback":callback,"response":response}});
        let before = args.clone();
        prepare_host_paths_with("browser_send", &mut args, || {
            panic!("No relative fulfill path")
        })
        .unwrap();
        assert_eq!(args, before);
    }
    let mut args = json!({"request":{"operation":"callback_resolve","callback":callback,"response":{"kind":"route_fulfill","path":"Acme\u{0}"}}});
    assert_eq!(
        prepare_host_paths_with("browser_send", &mut args, || Ok("/private".into()))
            .unwrap_err()
            .code,
        "ROUTE_FILE_PATH_INVALID"
    );
}

#[test]
fn certificate_native_browser_paths_bind_to_the_invoking_frontend() {
    let mut args = json!({"engine":"chrome","context":{"clientCertificates":[{"origin":"https://localhost","certPath":"Acme.pem","keyPath":"Acme.key"}]}});
    let mut reads = 0;
    prepare_host_paths_with("browser_connect", &mut args, || {
        reads += 1;
        Ok("/private/Acme caller".into())
    })
    .unwrap();
    assert_eq!(reads, 1);
    assert_eq!(
        args["context"]["clientCertificates"][0]["certPath"],
        "/private/Acme caller/Acme.pem"
    );
    assert_eq!(
        args["context"]["clientCertificates"][0]["keyPath"],
        "/private/Acme caller/Acme.key"
    );
}
#[test]
fn certificate_client_paths_cwd_failure_is_private_and_undispatched() {
    let mut args = json!({"request":{"operation":"http_context_create","options":{"clientCertificates":[{"origin":"https://example.com","pfxPath":"Acme private.pfx"}]}}});
    let error = prepare_host_paths_with("browser_send", &mut args, || {
        Err(std::io::ErrorKind::NotFound.into())
    })
    .unwrap_err();
    assert_eq!(error.code, "HOST_IO");
    assert_eq!(error.details.as_ref().unwrap()["dispatched"], false);
    assert!(!format!("{error:?}").contains("Acme private"));
}

#[test]
fn tag_requests_become_handle_evaluations_and_read_caller_files_like_the_sdk_client() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("acme.js"), "window.acme = 1;").unwrap();
    std::fs::write(root.path().join("acme.css"), "body{color:red}").unwrap();
    let scope = json!({"connectionId":"Acme"});
    let mut args = json!({"request":{"operation":"batch","requests":[
        {"operation":"page_add_script_tag","scope":scope,"path":"acme.js","type":"module"},
        {"operation":"page_add_style_tag","scope":scope,"path":"acme.css"},
        {"operation":"page_add_script_tag","scope":scope,"url":"https://example.com/acme.js"},
        {"operation":"page_add_style_tag","scope":scope,"content":"p{}"}
    ]}});
    let directory = root.path().to_path_buf();
    prepare_host_paths_with("browser_send", &mut args, || Ok(directory.clone())).unwrap();
    let requests = args["request"]["requests"].as_array().unwrap();
    for request in requests {
        assert_eq!(request["operation"], "evaluate");
        assert_eq!(request["returnHandle"], true);
        assert_eq!(request["isFunction"], true);
        assert_eq!(request["scope"], scope);
    }
    let script = &requests[0]["argument"]["value"];
    assert_eq!(script["content"], "window.acme = 1;\n//# sourceURL=acme.js");
    assert_eq!(script["type"], "module");
    assert!(script["url"].is_null());
    assert_eq!(
        requests[1]["argument"]["value"]["content"],
        "body{color:red}\n/*# sourceURL=acme.css*/"
    );
    assert!(requests[1]["argument"]["value"].get("type").is_none());
    assert_eq!(
        requests[2]["argument"]["value"]["url"],
        "https://example.com/acme.js"
    );
    assert_eq!(requests[3]["argument"]["value"]["content"], "p{}");
    for (request, code) in [
        (
            json!({"operation":"page_add_script_tag","scope":scope}),
            "INVALID_ARGUMENT",
        ),
        (
            json!({"operation":"page_add_style_tag","scope":scope,"path":"missing.css"}),
            "TAG_FILE_IO",
        ),
        (
            json!({"operation":"page_add_style_tag","scope":scope,"path":"."}),
            "TAG_FILE_NOT_REGULAR",
        ),
    ] {
        let mut args = json!({ "request": request });
        let error = prepare_host_paths_with("browser_send", &mut args, || Ok(directory.clone()))
            .unwrap_err();
        assert_eq!(error.code, code);
        assert_eq!(error.details.unwrap()["dispatched"], false);
    }
    // The byte bound applies to file bytes, not the appended sourceURL comment.
    std::fs::write(root.path().join("limit.js"), "a".repeat(512 * 1024)).unwrap();
    std::fs::write(root.path().join("over.js"), "a".repeat(512 * 1024 + 1)).unwrap();
    let mut args =
        json!({"request":{"operation":"page_add_script_tag","scope":scope,"path":"limit.js"}});
    prepare_host_paths_with("browser_send", &mut args, || Ok(directory.clone())).unwrap();
    assert_eq!(args["request"]["operation"], "evaluate");
    // Oversized files and content that escapes past the encoded request bound
    // refuse before dispatch rather than at the frame or argument limit.
    for request in [
        json!({"operation":"page_add_script_tag","scope":scope,"path":"over.js"}),
        json!({"operation":"page_add_style_tag","scope":scope,"content":"a".repeat(512 * 1024 + 1)}),
        json!({"operation":"page_add_script_tag","scope":scope,"content":"\"".repeat(500 * 1024)}),
    ] {
        let mut args = json!({ "request": request });
        let error = prepare_host_paths_with("browser_send", &mut args, || Ok(directory.clone()))
            .unwrap_err();
        assert_eq!(error.code, "TAG_CONTENT_BOUNDS");
        assert_eq!(error.details.unwrap()["dispatched"], false);
    }
}

#[test]
fn storage_state_client_paths_bind_input_and_export_files_to_the_caller() {
    let mut args = json!({"request":{"operation":"batch","requests":[{"operation":"http_context_create","options":{"storageState":"../Acme state.json"}},{"operation":"http_context_storage_state","httpContext":{},"options":{"path":"out/Acme.json"}}]}});
    let mut reads = 0;
    prepare_host_paths_with("browser_send", &mut args, || {
        reads += 1;
        Ok("/private/Acme caller".into())
    })
    .unwrap();
    assert_eq!(reads, 1);
    assert_eq!(
        args["request"]["requests"][0]["options"]["storageState"],
        "/private/Acme caller/../Acme state.json"
    );
    assert_eq!(
        args["request"]["requests"][1]["options"]["path"],
        "/private/Acme caller/out/Acme.json"
    );
    // Browser contexts bind creation state and snapshot exports the same way.
    let mut args = json!({"request":{"operation":"batch","requests":[{"operation":"context_create","options":{"storageState":"state.json"}},{"operation":"storage_export","context":{},"options":{"path":"out/Acme.json"}}]}});
    prepare_host_paths_with("browser_send", &mut args, || Ok("/private/Acme".into())).unwrap();
    assert_eq!(
        args["request"]["requests"][0]["options"]["storageState"],
        "/private/Acme/state.json"
    );
    assert_eq!(
        args["request"]["requests"][1]["options"]["path"],
        "/private/Acme/out/Acme.json"
    );
    let mut connect =
        json!({"engine":"chrome","mode":"headless","context":{"storageState":"state.json"}});
    prepare_host_paths_with("browser_connect", &mut connect, || {
        Ok("/private/Acme".into())
    })
    .unwrap();
    assert_eq!(
        connect["context"]["storageState"],
        "/private/Acme/state.json"
    );
    for mut args in [
        json!({"request":{"operation":"context_create","options":{"storageState":{"cookies":[],"origins":[]}}}}),
        json!({"request":{"operation":"http_context_create","options":{"storageState":{"cookies":[]}}}}),
        json!({"request":{"operation":"http_context_create","options":{"storageState":"/private/Acme.json"}}}),
        json!({"request":{"operation":"http_context_storage_state","httpContext":{},"options":{"indexedDB":true}}}),
        json!({"request":{"operation":"http_context_create","options":{"path":"Acme.json"}}}),
    ] {
        let original = args.clone();
        prepare_host_paths_with("browser_send", &mut args, || {
            panic!("Inline, absolute and unrelated values must not read cwd")
        })
        .unwrap();
        assert_eq!(args, original);
    }
    let mut args = json!({"request":{"operation":"http_context_storage_state","httpContext":{},"options":{"path":""}}});
    let error =
        prepare_host_paths_with("browser_send", &mut args, || Ok("/private".into())).unwrap_err();
    assert_eq!(error.code, "HTTP_STORAGE_STATE_PATH_INVALID");
    assert_eq!(error.details.unwrap()["dispatched"], false);
}

#[test]
fn trace_export_client_paths_bind_one_caller_directory_without_editing_application_values() {
    let mut args = json!({"request":{"operation":"batch","requests":[{"operation":"trace_start","options":{"filePath":"nested/../Acme.data"}},{"operation":"trace_stop","filePath":".Acme.gz"},{"operation":"evaluate","argument":{"kind":"json","value":{"operation":"trace_stop","filePath":"application-data"}}}]}});
    let original = args["request"]["requests"][2].clone();
    let mut reads = 0;
    prepare_host_paths_with("browser_send", &mut args, || {
        reads += 1;
        Ok("/private/Acme/client".into())
    })
    .unwrap();
    assert_eq!(reads, 1);
    assert_eq!(
        args["request"]["requests"][0]["options"]["filePath"],
        "/private/Acme/client/Acme.data"
    );
    assert_eq!(
        args["request"]["requests"][1]["filePath"],
        "/private/Acme/client/.Acme.gz"
    );
    assert_eq!(args["request"]["requests"][2], original);
}

#[test]
fn trace_export_client_paths_leave_unrelated_absolute_and_refused_batches_without_cwd_reads() {
    for (name, mut args) in [
        (
            "browser_send",
            json!({"request":{"operation":"trace_stop","filePath":"/private/Acme/trace.json"}}),
        ),
        (
            "browser_send",
            json!({"request":{"operation":"evaluate","expression":"Acme","argument":{"filePath":"data"}}}),
        ),
        (
            "browser_send",
            json!({"request":{"operation":"trace_start","options":{"filePath":null}}}),
        ),
        (
            "browser_send",
            json!({"request":{"operation":"batch","requests":vec![json!({"operation":"trace_stop","filePath":"Acme.json"});9]}}),
        ),
        (
            "browser_status",
            json!({"request":{"operation":"trace_stop","filePath":"Acme.json"}}),
        ),
    ] {
        let original = args.clone();
        prepare_host_paths_with(name, &mut args, || {
            panic!("Unrelated/absolute paths must not read cwd")
        })
        .unwrap();
        assert_eq!(args, original);
    }
}

#[test]
fn trace_export_client_paths_report_undispatched_cwd_and_invalid_path_refusals() {
    let mut args = json!({"request":{"operation":"trace_stop","filePath":"Acme.json"}});
    let error = prepare_host_paths_with("browser_send", &mut args, || {
        Err(std::io::ErrorKind::NotFound.into())
    })
    .unwrap_err();
    assert_eq!(error.code, "HOST_IO");
    assert_eq!(error.details.unwrap()["dispatched"], false);
    let mut args =
        json!({"request":{"operation":"trace_stop","filePath":"http://example.com/Acme.json"}});
    let error = prepare_host_paths_with("browser_send", &mut args, || Ok("/private/Acme".into()))
        .unwrap_err();
    assert_eq!(error.code, "TRACE_EXPORT_PATH_INVALID");
}

#[test]
fn workflow_negotiation_follows_only_executable_requests() {
    assert!(requires_workflows(
        &json!({"engine":"firefox","context":{"locale":"en-US"}})
    ));
    assert!(!requires_workflows(
        &json!({"engine":"firefox","context":{"hasTouch":true}})
    ));
    assert!(requires_workflows(
        &json!({"request":{"operation":"batch","requests":[{"operation":"storage_export"}]}})
    ));
    assert!(requires_workflows(
        &json!({"request":{"operation":"context_create","options":{"screen":{"width":640,"height":480}}}})
    ));
    assert!(!requires_workflows(
        &json!({"request":{"operation":"evaluate","argument":{"kind":"json","value":{"operation":"storage_export","context":{"locale":"en-US"}}}}})
    ));
}

fn reference() -> Value {
    json!(Reference {
        connection_id: Uuid::new_v4(),
        id: Uuid::new_v4(),
        generation: Uuid::new_v4()
    })
}

fn investigation_actions(page: &Value) -> Vec<Value> {
    vec![
        json!({"action":"diagnostics_read","context":page,"query":{"sequence":17,"includeStack":true,"worker":page,"levels":["error"],"resourceTypes":[]}}),
        json!({"action":"diagnostics_arguments_set","context":page,"enabled":true}),
        json!({"action":"diagnostics_arguments_list","context":page}),
        json!({"action":"diagnostics_arguments_read","context":page,"groupId":page["id"],"query":{"index":0,"path":["action"],"properties":{"offset":0,"limit":10}}}),
        json!({"action":"diagnostics_arguments_remove","context":page,"groupId":page["id"]}),
        json!({"action":"profile_status","page":page}),
        json!({"action":"profile_stop","page":page}),
        json!({"action":"heap_capture","page":page,"maxBytes":1048576}),
        json!({"action":"heap_stop","page":page}),
        json!({"action":"heap_import","developer":page,"path":"/private/tmp/Acme/heap.heapsnapshot","maxBytes":1048576,"identityGroup":"11111111-1111-4111-8111-111111111111"}),
        json!({"action":"heap_query","developer":page,"snapshotId":"11111111-1111-4111-8111-111111111111","query":{"kind":"class_nodes","classId":1,"filterName":"attributedToSpecificNativeContext","objectId":17,"limit":3}}),
        json!({"action":"heap_compare","developer":page,"snapshotId":"11111111-1111-4111-8111-111111111111","baseSnapshotId":"22222222-2222-4222-8222-222222222222","classIndex":0,"maxNameLength":64,"offset":2,"limit":3}),
        json!({"action":"heap_query","developer":page,"snapshotId":"11111111-1111-4111-8111-111111111111","query":{"kind":"edges","nodeId":17,"sortBy":"name","minRetainedSize":12,"excludePrimitives":false,"limit":3}}),
        json!({"action":"heap_query","developer":page,"snapshotId":"11111111-1111-4111-8111-111111111111","query":{"kind":"class_nodes","classId":1,"offset":2,"limit":3}}),
        json!({"action":"heap_query","developer":page,"snapshotId":"11111111-1111-4111-8111-111111111111","query":{"kind":"contexts","scopeInfoNodeId":17,"retainedSize":{"min":1,"max":1024},"offset":2,"limit":3}}),
        json!({"action":"heap_query","developer":page,"snapshotId":"11111111-1111-4111-8111-111111111111","query":{"kind":"objects","className":"Acme","limit":10}}),
        json!({"action":"heap_query","developer":page,"snapshotId":"11111111-1111-4111-8111-111111111111","maxNameLength":64,"query":{"kind":"objects","className":"Acme","propertyName":"child","nodeType":"object","retainedSize":{"min":1,"max":1024},"selfSize":{"max":512},"sortBy":"id","limit":10}}),
        json!({"action":"heap_query","developer":page,"snapshotId":"11111111-1111-4111-8111-111111111111","maxNameLength":32,"query":{"kind":"retaining_paths","nodeId":17,"maxDepth":4,"maxNodes":30,"maxSiblings":3}}),
        json!({"action":"trace_status"}),
        json!({"action":"trace_stop"}),
        json!({"action":"performance_query","developer":page,"analysisId":"11111111-1111-4111-8111-111111111111","query":{"kind":"vitals","insightSetId":"NAVIGATION_0"}}),
        json!({"action":"page_css","developer":page,"page":page,"locator":{"kind":"css","selector":"#Acme"},"offset":10,"limit":10}),
        json!({"action":"developer_tools_discover","scope":page}),
        json!({"action":"developer_tool_result_read","handle":page}),
        json!({"action":"web_mcp_debug","scope":page,"input":{"action":"invoke","name":"acme-debug","parameters":{"action":"keep"}}}),
        json!({"action":"web_mcp","scope":page,"input":{"action":"invoke","toolRef":page["id"],"parameters":{"action":"keep"}}}),
        json!({"action":"browser_extension","input":{"action":"trigger_action","extensionId":page["id"],"page":page}}),
        json!({"action":"browser_pwa","input":{"action":"launch","appId":page["id"],"url":"https://example.com/acme/"}}),
        json!({"action":"developer_tool_call","registry":page,"toolId":"t0","parameters":{"amount":2}}),
        json!({"action":"developer_tools_release","registry":page}),
        json!({"action":"framework_prepare","page":page}),
        json!({"action":"framework_open","scope":page}),
        json!({"action":"framework_query","registry":page,"query":{"kind":"tree","limit":20}}),
        json!({"action":"framework_release","registry":page}),
        json!({"action":"page_debugger","page":page,"input":{"action":"status","includeFrames":true}}),
        json!({"action":"context_debugger","context":page,"input":{"action":"run_to","pauseId":page["id"],"location":{"file":"Acme-journey.rs","line":30}}}),
        json!({"action":"network_route","scope":page,"input":{"action":"add_matcher","matcher":{"kind":"glob","pattern":"**/acme"}}}),
        json!({"action":"page_init_script_add","page":page,"source":"globalThis.Acme={action:'keep'}"}),
        json!({"action":"context_init_script_add","context":page,"source":"globalThis.Acme={action:'keep'}"}),
        json!({"action":"context_init_script_list","context":page}),
        json!({"action":"context_init_script_remove","context":page,"scriptId":page["id"]}),
        json!({"action":"context_framework_prepare","context":page}),
        json!({"action":"page_route_set","page":page,"url":"https://example.com/acme"}),
        json!({"action":"context_route_set","context":page,"url":"https://example.com/acme"}),
        json!({"action":"web_socket_route_add","scope":page,"url":"wss://example.com/socket"}),
        json!({"action":"web_socket_route_send","scope":page,"frame":page,"routeId":page["id"],"socketId":"a".repeat(64),"to":"page","message":{"data":"AP8=","isBase64":true}}),
        json!({"action":"web_socket_route_list","scope":page}),
        json!({"action":"web_socket_route_remove","scope":page,"routeId":page["id"]}),
        json!({"action":"har_register","path":"/tmp/acme.har"}),
        json!({"action":"har_list"}),
        json!({"action":"page_har_set","page":page,"archiveId":page["id"]}),
        json!({"action":"page_har_set","page":page,"archiveId":null}),
        json!({"action":"context_har_set","context":page,"archiveId":null}),
        json!({"action":"har_capture_start","context":page,"content":"embed"}),
        json!({"action":"har_capture_start","context":page,"content":"attach","replacesArchiveId":page["id"]}),
        json!({"action":"har_capture_status","context":page}),
        json!({"action":"har_capture_stop","context":page}),
        json!({"action":"har_finalize","archiveId":page["id"]}),
        json!({"action":"har_remove","archiveId":page["id"]}),
        json!({"action":"binding_add","scope":page,"name":"acmeBridge"}),
        json!({"action":"binding_read","callback":page}),
        json!({"action":"http_context_create","options":{"baseUrl":"https://example.com/","browserContext":page}}),
        json!({"action":"http_fetch","httpContext":page,"request":{"url":"https://example.com/acme","method":"POST","body":{"kind":"text","text":"{\"action\":\"keep\"}"}}}),
        json!({"action":"page_route_set","page":page,"url":"https://example.com/acme","authenticationChallenges":true}),
        json!({"action":"callback_resolve","callback":page,"response":{"kind":"route_fulfill","status":200,"body":"{\"action\":\"keep\"}","contentType":"application/json"}}),
        json!({"action":"callback_resolve","callback":page,"response":{"kind":"route_fulfill_binary","status":201,"bodyBase64":"AP+A","headers":[{"name":"content-type","value":"application/octet-stream"}]}}),
        json!({"action":"callback_resolve","callback":page,"response":{"kind":"route_fulfill_evidence","status":201,"networkEvidenceId":page["id"],"headers":[{"name":"content-type","value":"application/octet-stream"}]}}),
        json!({"action":"callback_resolve","callback":page,"response":{"kind":"route_har","archiveId":page["id"],"notFound":"abort"}}),
        json!({"action":"callback_resolve","callback":page,"response":{"kind":"route_continue_with","overrides":{"url":"https://example.com/rewrite?private=Acme","method":"POST","headers":[{"name":"x-acme","value":"new"}],"bodyBase64":"AP+A"}}}),
        json!({"action":"callback_resolve","callback":page,"response":{"kind":"route_fallback"}}),
        json!({"action":"callback_resolve","callback":page,"response":{"kind":"route_fallback_with","overrides":{"method":"POST","bodyBase64":"AP8="}}}),
    ]
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one table of every investigation operation and its translation"
)]
fn investigation_operations_build_the_catalog_and_translate_without_touching_payloads() {
    let page = reference();
    let target = format!("browser:{}", page["connectionId"].as_str().unwrap());
    for action in investigation_actions(&page) {
        let operation = action["action"].clone();
        let replaces = action.get("replacesArchiveId").cloned();
        let heap_query = action.get("query").cloned();
        let heap_name_length = action.get("maxNameLength").cloned();
        let heap_class_index = action.get("classIndex").cloned();
        let import_group = action.get("identityGroup").cloned();
        let (name, args) = translate("act", json!({"targetId":target,"actions":[action]})).unwrap();
        assert_eq!(name, "browser_send");
        assert_eq!(args["request"]["requests"][0]["operation"], operation);
        let _: Request = serde_json::from_value(args["request"].clone()).unwrap();
        if operation == "heap_query" {
            assert_eq!(args["request"]["requests"][0]["query"], heap_query.unwrap());
            if let Some(length) = heap_name_length.as_ref() {
                assert_eq!(&args["request"]["requests"][0]["maxNameLength"], length);
            }
        }
        if operation == "heap_compare" {
            assert_eq!(
                args["request"]["requests"][0]["classIndex"],
                heap_class_index.unwrap()
            );
            assert_eq!(
                args["request"]["requests"][0]["maxNameLength"],
                heap_name_length.unwrap_or(Value::Null)
            );
        }
        if operation == "heap_import" {
            assert_eq!(
                args["request"]["requests"][0]["identityGroup"],
                import_group.unwrap()
            );
        }
        if operation == "har_capture_start" {
            assert_eq!(
                args["request"]["requests"][0]["content"],
                if replaces.is_some() {
                    "attach"
                } else {
                    "embed"
                }
            );
            assert_eq!(
                args["request"]["requests"][0]["replacesArchiveId"],
                replaces.unwrap_or(Value::Null)
            );
        }
        if operation == "callback_resolve" {
            let response = &args["request"]["requests"][0]["response"];
            if response["kind"] == "route_har" {
                assert_eq!(response["archiveId"], page["id"]);
            } else if response["kind"] == "route_fulfill_evidence" {
                assert_eq!(response["networkEvidenceId"], page["id"]);
            } else if response["kind"] == "route_continue_with" {
                assert_eq!(response["overrides"]["bodyBase64"], "AP+A");
                assert_eq!(response["overrides"]["method"], "POST");
            } else if response["kind"] == "route_fulfill_binary" {
                assert_eq!(response["bodyBase64"], "AP+A");
            } else if response["kind"] == "route_fallback" {
                assert_eq!(response, &json!({"kind":"route_fallback"}));
            } else if response["kind"] == "route_fallback_with" {
                assert_eq!(response["overrides"]["method"], "POST");
                assert_eq!(response["overrides"]["bodyBase64"], "AP8=");
            } else {
                assert_eq!(response["body"], "{\"action\":\"keep\"}");
            }
        }
        if operation == "page_debugger" {
            assert_eq!(args["request"]["requests"][0]["action"]["kind"], "status");
        }
        if operation == "context_debugger" {
            assert_eq!(args["request"]["requests"][0]["action"]["kind"], "run_to");
            assert_eq!(
                args["request"]["requests"][0]["action"]["location"]["file"],
                "Acme-journey.rs"
            );
        }
        if operation == "network_route" {
            assert_eq!(
                args["request"]["requests"][0]["action"]["kind"],
                "add_matcher"
            );
            assert_eq!(
                args["request"]["requests"][0]["action"]["matcher"]["kind"],
                "glob"
            );
        }
        if matches!(operation.as_str(), Some("web_mcp_debug" | "web_mcp")) {
            assert_eq!(args["request"]["requests"][0]["action"]["kind"], "invoke");
            assert_eq!(
                args["request"]["requests"][0]["action"]["parameters"]["action"],
                "keep"
            );
        }
        if operation == "http_fetch" {
            assert_eq!(
                args["request"]["requests"][0]["request"]["body"]["text"],
                "{\"action\":\"keep\"}"
            );
        }
    }
}

#[test]
fn shared_actions_keep_native_scopes_and_application_values() {
    let page = reference();
    let target = format!("browser:{}", page["connectionId"].as_str().unwrap());
    let id = Uuid::new_v4();
    let (name, args) = translate("act", json!({"targetId":target,"actionId":id,"actions":[
        {"action":"page_action","page":page,"locator":{"kind":"css","selector":"button"},"input":{"action":"click"}},
        {"action":"page_input","page":page,"actions":[{"action":"key_up","key":"Shift"}]},
        {"action":"evaluate","scope":page,"expression":"value => value","argument":{"kind":"json","value":{"action":"Acme","kind":"private","operation":"keep"}}}
    ]})).unwrap();
    assert_eq!(name, "browser_send");
    assert_eq!(args["operationId"], json!(id));
    assert_eq!(args["request"]["requests"][0]["action"]["kind"], "click");
    assert_eq!(args["request"]["requests"][0]["locator"]["kind"], "css");
    assert_eq!(
        args["request"]["requests"][1]["actions"][0]["kind"],
        "key_up"
    );
    assert_eq!(
        args["request"]["requests"][2]["argument"]["value"]["operation"],
        "keep"
    );
    let _: Request = serde_json::from_value(args["request"].clone()).unwrap();
    let tool = crate::browser::surface::tools()
        .into_iter()
        .find(|t| t["name"] == name)
        .unwrap();
    crate::schema::validate(&tool["inputSchema"], &args).unwrap();
}

#[test]
fn source_wrappers_translate_one_executable_layer_without_discriminator_collisions() {
    let page = reference();
    let target = format!("browser:{}", page["connectionId"].as_str().unwrap());
    let source = json!({"file":"src/Acme-journey.rs","line":30,"column":3});
    let input = json!({"action":"at_source","source":source,"input":{"action":"fill","value":"{\"action\":\"keep\",\"kind\":\"data\"}"}});
    let args = json!({"targetId":target,"actions":[{"action":"page_action","page":page,"locator":{"kind":"css","selector":"input"},"input":input}]});
    let (_, translated) = translate("act", args.clone()).unwrap();
    assert_eq!(
        translated["request"]["requests"][0]["action"],
        json!({"kind":"at_source","source":source,"action":{"kind":"fill","value":"{\"action\":\"keep\",\"kind\":\"data\"}"}})
    );
    let _: Request = serde_json::from_value(translated["request"].clone()).unwrap();
    let mut nested = args.clone();
    nested["actions"][0]["input"]["input"] = input;
    assert!(translate("act", nested).is_err());
    let mut conflicting = args;
    conflicting["actions"][0]["input"]["kind"] = json!("click");
    assert!(translate("act", conflicting).is_err());
}

#[test]
fn browser_connect_defaults_to_headless_and_preserves_explicit_modes() {
    use crate::browser::contract::{LaunchOptions, Mode};

    for (args, expected) in [
        (json!({"engine":"chrome"}), Mode::Headless),
        (json!({"engine":"chrome","mode":"headless"}), Mode::Headless),
        (json!({"engine":"chrome","mode":"headed"}), Mode::Headed),
    ] {
        for tool in ["connect", "browser_connect"] {
            let definition = crate::catalog::tool_named(tool).unwrap();
            crate::schema::validate(&definition["inputSchema"], &args).unwrap();
            let (name, envelope) = if tool == "connect" {
                translate(tool, args.clone()).unwrap()
            } else {
                (tool.into(), args.clone())
            };
            assert_eq!(name, "browser_connect");
            crate::browser::surface::validate(&name, &envelope).unwrap();
            let options: LaunchOptions = serde_json::from_value(envelope).unwrap();
            assert_eq!(options.mode, expected, "{tool}");
        }
    }
    for args in [
        json!({"engine":"chrome","mode":null}),
        json!({"engine":"chrome","mode":"visible"}),
    ] {
        assert!(translate("connect", args.clone()).is_err());
        assert!(crate::browser::surface::validate("browser_connect", &args).is_err());
        assert!(serde_json::from_value::<LaunchOptions>(args).is_err());
    }
}

#[test]
fn browser_media_fixture_connects_bind_paths_on_the_caller_and_require_chrome() {
    use crate::browser::contract::LaunchOptions;
    let args = json!({"engine":"chrome","mediaFixtures":{
        "microphonePath":"fixtures/acme speech.wav","cameraPath":"/private/acme/camera.y4m"
    }});
    for tool in ["connect", "browser_connect"] {
        let schema = crate::catalog::tool_named(tool).unwrap();
        crate::schema::validate(&schema["inputSchema"], &args).unwrap();
        let (name, mut envelope) = if tool == "connect" {
            translate(tool, args.clone()).unwrap()
        } else {
            (tool.into(), args.clone())
        };
        assert_eq!(name, "browser_connect");
        prepare_host_paths_with(&name, &mut envelope, || Ok("/private/acme".into())).unwrap();
        let mut launch: LaunchOptions = serde_json::from_value(envelope).unwrap();
        assert_eq!(launch.validate(), Ok(()));
        assert_eq!(
            launch.media_fixtures.paths().collect::<Vec<_>>(),
            [
                "/private/acme/fixtures/acme speech.wav",
                "/private/acme/camera.y4m"
            ]
        );
        launch.engine = crate::browser::contract::Engine::Firefox;
        let error = crate::browser::runtime::launch_options_error(launch.validate().unwrap_err());
        assert_eq!(error.code, "MEDIA_FIXTURES_UNSUPPORTED");
        assert_eq!(error.message, "mediaFixtures require engine chrome");
    }
}

#[test]
fn browser_media_fixture_path_errors_report_no_admission_or_dispatch() {
    for (path, code, message) in [
        (
            "",
            "MEDIA_FIXTURE_PATH_INVALID",
            "Media fixtures require a bounded host file path",
        ),
        (
            "fixtures/Acme.wav",
            "HOST_IO",
            "Cannot bind the relative media fixture path",
        ),
    ] {
        let mut args = json!({"engine":"chrome","mediaFixtures":{"microphonePath":path}});
        let error = prepare_host_paths_with("browser_connect", &mut args, || {
            Err(std::io::ErrorKind::NotFound.into())
        })
        .unwrap_err();
        assert_eq!(error.code, code);
        assert_eq!(error.message, message);
        assert_eq!(
            error.details,
            Some(json!({"admitted":false,"dispatched":false}))
        );
    }
}

#[test]
fn variants_refuse_cross_platform_options_and_conflicting_discriminators() {
    let target = format!("browser:{}", Uuid::new_v4());
    let page = reference();
    for args in [
        json!({"targetId":target,"actions":[]}),
        json!({"targetId":target,"actions":[{"action":"context_list","operation":"page_list"}]}),
        json!({"targetId":target,"actions":[{"action":"context_list"}],"settle":false}),
        json!({"targetId":target,"actions":[{"action":"page_action","page":page,"locator":{"kind":"css","selector":"button"},"input":{"action":"click","kind":"fill"}}]}),
        json!({"targetId":target,"actions":[{"action":"batch","requests":[]}]}),
    ] {
        assert!(translate("act", args).is_err());
    }
    assert!(
        translate(
            "connect",
            json!({"engine":"firefox","mode":"headless","boot":true})
        )
        .is_err()
    );
    assert!(translate("status", json!({"targetId":"android:Acme","waitMs":1})).is_err());
    assert!(validate_mobile("observe", &json!({"targetId":"android:Acme","page":page})).is_err());
    validate_mobile(
        "act",
        &json!({"actions":[{"action":"tap","point":{"x":0.3,"y":0.4}}]}),
    )
    .unwrap();
    assert!(translate("control_acquire", json!({"targetId":target})).is_err());
    assert_eq!(
        validate_mobile("cancel", &json!({"targetId":"android:Acme"}))
            .unwrap_err()
            .code,
        "CAPABILITY_UNAVAILABLE"
    );
}

#[test]
fn observe_status_and_cancel_share_the_retained_identity() {
    let target = format!("browser:{}", Uuid::new_v4());
    let id = Uuid::new_v4();
    let (_, screenshot) =
        translate("observe", json!({"targetId":target,"page":reference()})).unwrap();
    assert_eq!(screenshot["request"]["operation"], "page_screenshot");
    for (name, destination) in [("status", "browser_status"), ("cancel", "browser_cancel")] {
        let (name, args) = translate(name, json!({"targetId":target,"actionId":id})).unwrap();
        assert_eq!(name, destination);
        assert_eq!(args["operationId"], json!(id));
    }
    let retained = json!({"operation":{"id":id,"outcome":"unknown","settled":false,"result":{"completedActions":1}}});
    let mapped = result(retained.clone());
    assert_eq!(mapped["actionId"], json!(id));
    assert_eq!(mapped["operation"], retained["operation"]);
}

#[test]
fn browser_control_uses_shared_tools_and_keeps_page_scope_explicit() {
    let page = reference();
    let target = format!("browser:{}", page["connectionId"].as_str().unwrap());
    let (name, args) = translate("control_acquire", json!({"targetId":target,"owner":"viewer","viewerId":"acme-viewer","page":page,"expectedVersion":1})).unwrap();
    assert_eq!(name, "browser_control_acquire");
    assert_eq!(args["page"], page);
    let definition = super::super::surface::definition(&name).unwrap();
    crate::schema::validate(&definition["inputSchema"], &args).unwrap();
    assert!(
        !crate::catalog::tools()
            .iter()
            .any(|tool| tool["name"] == "browser_control_acquire")
    );
    assert!(validate_mobile("control_acquire", &json!({"targetId":"android:Acme","owner":"viewer","viewerId":"acme-viewer","page":page,"expectedVersion":1})).is_err());
}
#[test]
fn clipboard_actions_keep_their_input_discriminator_through_shared_translation() {
    let reference = json!({"connectionId":uuid::Uuid::new_v4(),"id":uuid::Uuid::new_v4(),"generation":uuid::Uuid::new_v4()});
    let action = json!({"action":"page_clipboard","page":reference,"origin":"https://example.com","input":{"action":"write","text":"Acme"}});
    let translated = super::request(action).unwrap();
    assert_eq!(translated["operation"], "page_clipboard");
    assert_eq!(translated["action"]["kind"], "write");
    let _: crate::browser::contract::Request = serde_json::from_value(translated).unwrap();
}

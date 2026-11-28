//! Real V8 snapshot bytes and the bundled native DevTools worker, in both modes.
use super::*;
use crate::browser::{
    heap::Query,
    page::{Load, Navigation},
};
use serde_json::json;
async fn send(session: &Session, request: Request) -> Value {
    let label = format!("{request:?}");
    let operation = settled(
        session,
        session.submit(Uuid::new_v4(), request, 30000).unwrap(),
    )
    .await;
    assert_eq!(
        operation.outcome,
        Outcome::Succeeded,
        "{label}: {operation:?}"
    );
    operation.result.unwrap()
}
fn reference(value: &Value, key: &str) -> Reference {
    serde_json::from_value(value[key].clone()).unwrap()
}
fn model_id(value: &Value) -> Uuid {
    serde_json::from_value(value["snapshotId"].clone()).unwrap()
}
fn heap_compare(
    developer: Reference,
    snapshot_id: Uuid,
    base_snapshot_id: Uuid,
    class_index: Option<usize>,
    limit: usize,
) -> Request {
    Request::HeapCompare {
        developer,
        snapshot_id,
        base_snapshot_id,
        class_index,
        max_name_length: 100,
        offset: 0,
        limit,
    }
}
fn heap_query(developer: Reference, snapshot_id: Uuid, query: Query) -> Request {
    Request::HeapQuery {
        developer,
        snapshot_id,
        query,
        max_name_length: 1024,
    }
}
#[tokio::test]
async fn native_heap_capture_queries_dominators_and_comparison_use_the_bundled_worker() {
    std::env::var_os("TESTRUN_BROWSER_ROOT").expect("Select a private browser cache");
    let bundle = crate::browser::preparation::prepare_chrome(|_| {})
        .await
        .unwrap();
    for mode in [Mode::Headless, Mode::Headed] {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("state");
        crate::artifacts::directory(&directory).unwrap();
        let session = Session::launch_connection(
            bundle.clone(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            LaunchOptions {
                media_fixtures: Default::default(),
                proxy: None,
                engine: Engine::Chrome,
                mode,
                context: Default::default(),
            },
            &directory,
        )
        .unwrap();
        let result=std::panic::AssertUnwindSafe(async{
            let ready=successful(&session,session.startup()).await;let page=reference(&ready,"page");
            send(&session,Request::PageNavigate{page,navigation:Navigation::SetContent{html:"<title>Acme heap</title><script>class AcmeHeapRecord{constructor(){this.privateText='Acme heap private';this.child={value:42}}}globalThis.acmeHeapRecords=Array.from({length:3},()=>new AcmeHeapRecord());</script>".into(),wait_until:Load::Load}}).await;
            let opened=send(&session,Request::DeveloperOpen{page,panel:Some("heap-profiler".into())}).await;let developer=reference(&opened,"developer");
            let capture=send(&session,Request::HeapCapture{page,max_bytes:crate::browser::heap::MAX_BYTES}).await;
            let report=&capture["reports"][0];assert_eq!(report["format"],"v8-heap-snapshot-json");assert_eq!(report["complete"],true);
            assert!(!capture.to_string().contains("Acme heap private"));
            let raw=std::fs::read(report["attachment"]["path"].as_str().unwrap()).unwrap();let snapshot:Value=serde_json::from_slice(&raw).unwrap();assert!(snapshot["snapshot"]["node_count"].as_u64().unwrap()>0);
            let evidence_id=serde_json::from_value(report["evidenceId"].clone()).unwrap();
            let loaded=send(&session,Request::HeapLoad{developer,evidence_id}).await;
            let snapshot_id=serde_json::from_value(loaded["snapshotId"].clone()).unwrap();
            let objects=send(&session,heap_query(developer,snapshot_id,serde_json::from_value(json!({"kind":"objects","className":"AcmeHeapRecord","offset":0,"limit":10})).unwrap())).await;
            let items=objects["result"]["data"]["items"].as_array().expect("Object query items");assert!(items.len()>=3,"{objects}");
            let node_id=items.iter().find(|item|item["name"]=="AcmeHeapRecord"&&item["type"]=="object").unwrap()["id"].as_u64().unwrap();
            let filtered=send(&session,heap_query(developer,snapshot_id,serde_json::from_value(json!({"kind":"objects","className":"AcmeHeapRecord","propertyName":"privateText","nodeType":"object","retainedSize":{"min":1},"selfSize":{"min":1},"sortBy":"id","limit":10})).unwrap())).await;
            let filtered_items=filtered["result"]["data"]["items"].as_array().unwrap();assert_eq!(filtered_items.len(),3,"{filtered}");assert!(filtered_items.windows(2).all(|items|items[0]["id"].as_u64().unwrap()<=items[1]["id"].as_u64().unwrap()));
            for filter in [json!({"propertyName":"AcmeMissingProperty"}),json!({"selfSize":{"max":0}}),json!({"retainedSize":{"max":0}})] {
                let mut query=json!({"kind":"objects","className":"AcmeHeapRecord","nodeType":"object","limit":10});query.as_object_mut().unwrap().extend(filter.as_object().unwrap().clone());
                let empty=send(&session,heap_query(developer,snapshot_id,serde_json::from_value(query).unwrap())).await;assert!(empty["result"]["data"]["items"].as_array().unwrap().is_empty(),"{empty}");
            }
            let short=send(&session,Request::HeapQuery{developer,snapshot_id,query:Query::Object{node_id},max_name_length:2}).await;
            assert_eq!(short["result"]["data"]["name"],"Ac");assert_eq!(short["result"]["truncated"],true);
            let classes=send(&session,heap_query(developer,snapshot_id,Query::Classes{filter_name:None,object_id:None,offset:0,limit:1})).await;
            let class_id=classes["result"]["data"]["items"][0]["classId"].as_u64().unwrap();
            assert!(classes["result"]["data"]["objectCount"].as_u64().unwrap()>0);
            assert!(classes["result"]["data"]["totalSelfSize"].as_u64().unwrap()>0);
            let instances=send(&session,heap_query(developer,snapshot_id,Query::ClassNodes{class_id,filter_name:None,object_id:None,offset:0,limit:1})).await;
            assert_eq!(instances["result"]["data"]["items"].as_array().unwrap().len(),1,"{instances}");
            let edges=send(&session,heap_query(developer,snapshot_id,serde_json::from_value(json!({"kind":"edges","nodeId":node_id,"sortBy":"name","excludePrimitives":false,"limit":100})).unwrap())).await;
            assert!(edges["result"]["data"]["items"].as_array().unwrap().iter().any(|edge|edge["name"]=="privateText"&&edge["node"]["type"]=="string"),"{edges}");
            let excluded=send(&session,heap_query(developer,snapshot_id,serde_json::from_value(json!({"kind":"edges","nodeId":node_id,"minRetainedSize":9007199254740991u64})).unwrap())).await;
            assert!(excluded["result"]["data"]["items"].as_array().unwrap().is_empty(),"{excluded}");
            let default_paths=send(&session,serde_json::from_value(json!({"operation":"heap_query","developer":developer,"snapshotId":snapshot_id,"query":{"kind":"retaining_paths","nodeId":node_id}})).unwrap()).await;
            assert!(!default_paths["result"]["data"].is_null(),"{default_paths}");
            for query in [Query::Summary{},Query::Object{node_id},Query::Edges{node_id,sort_by:crate::browser::heap::EdgeSort::Name,min_retained_size:None,exclude_primitives:false,offset:0,limit:10},Query::Retainers{node_id,offset:0,limit:10},Query::Dominators{node_id},Query::RetainingPaths{node_id,max_depth:Some(4),max_nodes:Some(30),max_siblings:Some(3)},Query::Classes{filter_name:Some(crate::browser::heap::Retention::NoNativeContext),object_id:None,offset:0,limit:10},Query::DuplicateStrings{offset:0,limit:10},Query::Contexts{retained_size:None,scope_info_node_id:None,offset:0,limit:10}]{
                let value=send(&session,heap_query(developer,snapshot_id,query)).await;assert!(!value["result"]["data"].is_null(),"{value}");
            }
            send(&session,Request::Evaluate{scope:page,expression:"acmeHeapRecords.push(new AcmeHeapRecord());true".into(),is_function:Some(false),argument:Default::default(),return_handle:false}).await;
            let second=send(&session,Request::HeapCapture{page,max_bytes:crate::browser::heap::MAX_BYTES}).await;
            let evidence_id=serde_json::from_value(second["reports"][0]["evidenceId"].clone()).unwrap();let second=send(&session,Request::HeapLoad{developer,evidence_id}).await;let second_id=serde_json::from_value(second["snapshotId"].clone()).unwrap();
            let compared=send(&session,heap_compare(developer,second_id,snapshot_id,None,100)).await;
            let difference=compared["result"]["data"]["classes"].as_array().unwrap().iter().find(|class|class["name"]=="AcmeHeapRecord").unwrap();
            assert_eq!(difference["addedCount"],1,"{difference}");assert_eq!(difference["removedCount"],0,"{difference}");
            let class_index=difference["classIndex"].as_u64().unwrap() as usize;
            let short=send(&session,Request::HeapCompare{developer,snapshot_id:second_id,base_snapshot_id:snapshot_id,class_index:Some(class_index),max_name_length:2,offset:0,limit:10}).await;
            assert_eq!(short["result"]["data"]["classDiff"]["name"],"Ac");
            assert!(compared["result"]["data"]["totals"]["addedCount"].as_u64().unwrap()>0);
            let detailed=send(&session,heap_compare(developer,second_id,snapshot_id,Some(class_index),10)).await;
            let added=&detailed["result"]["data"]["objects"][0];assert_eq!(added["change"],"added");
            let added_id=added["id"].as_u64().unwrap();
            let object=send(&session,heap_query(developer,second_id,Query::Object{node_id:added_id})).await;
            assert_eq!(object["result"]["data"]["name"],"AcmeHeapRecord");
            send(&session,Request::HeapStop{page}).await;
            send(&session,Request::HeapRelease{developer,snapshot_id:second_id}).await;
            let third=send(&session,Request::HeapCapture{page,max_bytes:crate::browser::heap::MAX_BYTES}).await;
            let evidence_id=serde_json::from_value(third["reports"][0]["evidenceId"].clone()).unwrap();let third=send(&session,Request::HeapLoad{developer,evidence_id}).await;let third_id=serde_json::from_value(third["snapshotId"].clone()).unwrap();
            let incomparable=settled(&session,session.submit(Uuid::new_v4(),heap_compare(developer,third_id,snapshot_id,None,10),30000).unwrap()).await;
            assert_eq!(incomparable.code.as_deref(),Some("HEAP_INCOMPARABLE"));
            send(&session,Request::HeapStop{page}).await;
            send(&session,Request::HeapRelease{developer,snapshot_id}).await;
            let stale=settled(&session,session.submit(Uuid::new_v4(),heap_query(developer,snapshot_id,Query::Summary{}),30000).unwrap()).await;assert_eq!(stale.code.as_deref(),Some("UNKNOWN_HEAP_SNAPSHOT"));
            let input=directory.join("Acme.heapsnapshot");crate::artifacts::write(&input,&raw).unwrap();
            let imported=send(&session,Request::HeapImport{developer,path:input.to_str().unwrap().into(),max_bytes:crate::browser::heap::MAX_BYTES,identity_group:None}).await;
            let imported_report=&imported["reports"][0];let imported_evidence=serde_json::from_value(imported_report["evidenceId"].clone()).unwrap();
            std::fs::write(&input,b"Acme changed source").unwrap();
            let imported_model=send(&session,Request::HeapLoad{developer,evidence_id:imported_evidence}).await;let imported_id=model_id(&imported_model);
            assert_eq!(imported_model["identity"]["verified"],false);
            let read=send(&session,heap_query(developer,imported_id,serde_json::from_value(json!({"kind":"objects","className":"AcmeHeapRecord","nodeType":"object","limit":10})).unwrap())).await;
            assert_eq!(read["result"]["data"]["items"].as_array().unwrap().len(),3,"{read}");
            let ungrouped=settled(&session,session.submit(Uuid::new_v4(),heap_compare(developer,imported_id,imported_id,None,10),30000).unwrap()).await;
            assert_eq!(ungrouped.code.as_deref(),Some("HEAP_INCOMPARABLE"));
            send(&session,Request::HeapRelease{developer,snapshot_id:imported_id}).await;
            let group=serde_json::from_value(third["identity"]["identityEpoch"].clone()).unwrap();
            let mut imports=Vec::new();let mut models=Vec::new();
            for _ in 0..2{
                let imported=send(&session,Request::HeapImport{developer,path:imported_report["attachment"]["path"].as_str().unwrap().into(),max_bytes:crate::browser::heap::MAX_BYTES,identity_group:Some(group)}).await;
                let evidence_id=serde_json::from_value(imported["reports"][0]["evidenceId"].clone()).unwrap();imports.push(evidence_id);
                let model=send(&session,Request::HeapLoad{developer,evidence_id}).await;let model=model_id(&model);models.push(model);
                if models.len()==1{
                    let mixed=settled(&session,session.submit(Uuid::new_v4(),heap_compare(developer,model,third_id,None,10),30000).unwrap()).await;
                    assert_eq!(mixed.code.as_deref(),Some("HEAP_INCOMPARABLE"));
                    send(&session,Request::HeapRelease{developer,snapshot_id:third_id}).await;
                }
            }
            let compared=send(&session,heap_compare(developer,models[1],models[0],None,10)).await;
            assert_eq!(compared["comparisonIdentity"]["verified"],false);assert_eq!(compared["result"]["data"]["total"],0);
            send(&session,Request::HeapRelease{developer,snapshot_id:models[0]}).await;
            std::fs::write(&input,br#"{"snapshot":{"meta":{"node_fields":["invalid"],"edge_fields":["invalid"]},"node_count":1,"edge_count":0},"nodes":[0],"edges":[],"strings":[]}"#).unwrap();
            let invalid=send(&session,Request::HeapImport{developer,path:input.to_str().unwrap().into(),max_bytes:crate::browser::heap::MAX_BYTES,identity_group:None}).await;
            let invalid_evidence=serde_json::from_value(invalid["reports"][0]["evidenceId"].clone()).unwrap();imports.push(invalid_evidence);
            let invalid=settled(&session,session.submit(Uuid::new_v4(),Request::HeapLoad{developer,evidence_id:invalid_evidence},30000).unwrap()).await;
            assert_eq!(invalid.code.as_deref(),Some("HEAP_MODEL_FAILED"));
            let retained=invalid.result.unwrap();assert_eq!(retained["modelRetained"],true);
            send(&session,Request::HeapRelease{developer,snapshot_id:model_id(&retained)}).await;
            send(&session,Request::DeveloperClose{developer}).await;
            imports.push(imported_evidence);for evidence_id in imports{send(&session,Request::DeveloperEvidenceRemove{evidence_id}).await;}
            assert!(session.status()["heap"]["models"].as_array().unwrap().is_empty());
            let evidence=send(&session,Request::DeveloperEvidenceList{}).await;assert_eq!(evidence["evidence"].as_array().unwrap().len(),3);
            let page_read=send(&session,Request::Evaluate{scope:page,expression:"document.title".into(),is_function:Some(false),argument:Default::default(),return_handle:false}).await;
            assert_eq!(page_read["value"]["value"],"Acme heap");
        }).catch_unwind().await;
        successful(&session, session.disconnect(30000).unwrap()).await;
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }
}

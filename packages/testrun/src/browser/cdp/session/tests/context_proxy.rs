//! Native context allocation binds normalized proxy settings and effective auth.
use super::*;

#[tokio::test]
async fn context_proxy_native_creation_binds_private_fields_and_initial_http_defaults() {
    for bypass in [None, Some(" localhost , 127.0.0.1 ")] {
        for inherited in [false, true] {
            let (mut worker, mut peer, _root) = fixture().await;
            let mut options:ContextOptions=serde_json::from_value(json!({"viewport":{"width":640,"height":480},"deviceScaleFactor":1,
            "proxy":{"server":"http://AcmeIgnored:ignored@127.0.0.1:8080/path?Acme=1","bypass":bypass,"username":"Acme proxy","password":"proxy example"},
            "httpCredentials":{"username":"Acme origin","password":"origin example"}})).unwrap();
            if inherited {
                worker.shared.state.lock().unwrap().launch_proxy = options.proxy.take();
            }
            let running = tokio::spawn(async move {
                let result = worker
                    .execute_one(
                        Uuid::new_v4(),
                        Request::ContextCreate { options },
                        deadline(),
                    )
                    .await;
                (worker, result)
            });
            let create = command(&mut peer, "Target.createBrowserContext").await;
            if inherited {
                assert_eq!(create["params"], json!({"disposeOnDetach":true}));
            } else {
                assert_eq!(
                    create["params"],
                    json!({"disposeOnDetach":true,"proxyServer":"http://127.0.0.1:8080","proxyBypassList":if bypass.is_some(){"localhost,127.0.0.1"}else{"<-loopback>"}})
                );
            }
            assert!(!create.to_string().contains("proxy example"));
            assert!(!create.to_string().contains("AcmeIgnored"));
            reply(
                &mut peer,
                &create,
                json!({"browserContextId":"Acme-proxy-context"}),
            )
            .await;
            let downloads = command(&mut peer, "Browser.setDownloadBehavior").await;
            reply(&mut peer, &downloads, json!({})).await;
            let (mut worker, result) = running.await.unwrap();
            let receipt = result.unwrap();
            let context: Reference = serde_json::from_value(receipt["context"].clone()).unwrap();
            assert!(!receipt.to_string().contains("proxy example"));
            let stored = &worker.context_layout[&context];
            assert_eq!(
                stored.proxy.as_ref().unwrap().server,
                "http://127.0.0.1:8080"
            );
            let url = url::Url::parse("http://127.0.0.1/auth").unwrap();
            assert_eq!(
                stored
                    .http_credentials
                    .as_ref()
                    .unwrap()
                    .matching(&url)
                    .unwrap()
                    .username,
                "Acme proxy"
            );
            let mut client = crate::browser::http::Options::default();
            client.inherit(stored);
            worker
                .http_contexts
                .inherit_credentials(&mut client, context, None);
            assert_eq!(
                client
                    .effective_credentials()
                    .unwrap()
                    .unwrap()
                    .matching(&url)
                    .unwrap()
                    .username,
                "Acme proxy"
            );
            assert_eq!(
                serde_json::to_value(&client.proxy).unwrap()["server"],
                "http://127.0.0.1:8080"
            );
            assert!(
                tokio::time::timeout(Duration::from_millis(25), peer.next())
                    .await
                    .is_err()
            );
        }
    }
}

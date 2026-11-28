//! Native allocation, private preparation and retained relay cleanup.
use super::*;
use crate::http_fixtures::certificates;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
fn options(input: Value) -> ContextOptions {
    ContextOptions {
        client_certificates: Some(serde_json::from_value(input).unwrap()),
        ..Default::default()
    }
}

#[test]
fn native_certificate_options_preserve_browser_creation_tls_and_user_agent_refusals() {
    let mut options = ContextOptions {
        client_certificates: Some(vec![]),
        ..Default::default()
    };
    chrome_options(&options).unwrap();
    for value in [true, false] {
        options.ignore_https_errors = Some(value);
        chrome_options(&options).unwrap();
    }
    options.user_agent = Some("Acme caller".into());
    chrome_options(&options).unwrap();
    options.permissions = Some(vec![crate::browser::settings::Permission::Push]);
    assert_eq!(
        chrome_options(&options).unwrap_err().code,
        "PERMISSION_UNSUPPORTED"
    );
}

#[tokio::test]
async fn native_certificate_context_retains_private_proxy_and_cached_associated_http_material() {
    let keys = certificates::Identities::new().await;
    let (worker, mut peer, _root) = fixture().await;
    let input = keys.encrypted_input("https://localhost", 0, true);
    let options = options(json!([input]));
    let running = tokio::spawn(async move {
        let mut worker = worker;
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
    let proxy = create["params"]["proxyServer"].as_str().unwrap();
    assert!(proxy.starts_with("socks5://127.0.0.1:"));
    assert_eq!(create["params"]["proxyBypassList"], "<-loopback>");
    assert_eq!(create["params"].as_object().unwrap().len(), 3);
    let port = url::Url::parse(proxy).unwrap().port().unwrap();
    reply(
        &mut peer,
        &create,
        json!({"browserContextId":"Acme-certificate-context"}),
    )
    .await;
    let downloads = command(&mut peer, "Browser.setDownloadBehavior").await;
    reply(&mut peer, &downloads, json!({})).await;
    let (mut worker, result) = running.await.unwrap();
    let receipt = result.unwrap();
    let context: Reference = serde_json::from_value(receipt["context"].clone()).unwrap();
    assert_eq!(receipt.as_object().unwrap().len(), 1);
    assert!(worker.certificate_tunnels.active(context));
    assert!(worker.certificate_tunnels.cached(context).is_some());
    assert_eq!(
        worker.context_layout[&context]
            .client_certificates
            .as_ref()
            .unwrap()
            .len(),
        1
    );
    let client = worker
        .execute_one(
            Uuid::new_v4(),
            Request::HttpContextCreate {
                options: crate::browser::http::Options {
                    browser_context: Some(context),
                    user_agent: Some("Acme owned browser".into()),
                    ..Default::default()
                },
            },
            deadline(),
        )
        .await
        .unwrap();
    assert!(client["httpContext"].is_object());
    assert!(!client.to_string().contains("Acme fixture passphrase"));
    let client = serde_json::from_value(client["httpContext"].clone()).unwrap();
    worker.http_contexts.remove(client).unwrap();
    worker
        .certificate_tunnels
        .close(Some(context), deadline())
        .await
        .unwrap();
    assert!(!worker.certificate_tunnels.active(context));
    assert!(
        tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn native_certificate_invalid_material_refuses_before_native_allocation_or_tunnel() {
    let (mut worker, mut peer, _root) = fixture().await;
    let options =
        options(json!([{"origin":"https://localhost","cert":"QWNtZQ==","key":"QWNtZQ=="}]));
    let error = worker
        .execute_one(
            Uuid::new_v4(),
            Request::ContextCreate { options },
            deadline(),
        )
        .await
        .unwrap_err();
    assert!(
        error.code.starts_with("HTTP_CLIENT_CERTIFICATE_"),
        "{error:?}"
    );
    assert!(worker.index.list(Kind::Context).is_empty());
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    worker
        .certificate_tunnels
        .close(None, deadline())
        .await
        .unwrap();
}

#[tokio::test]
async fn native_certificate_refused_allocation_retains_the_original_relay_until_joined_cleanup() {
    let keys = certificates::Identities::new().await;
    let (mut worker, mut peer, _root) = fixture().await;
    let options = options(json!([keys.input("https://localhost", 0)]));
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
    let port = url::Url::parse(create["params"]["proxyServer"].as_str().unwrap())
        .unwrap()
        .port()
        .unwrap();
    send(
        &mut peer,
        json!({"id":create["id"],"error":{"code":-32000,"message":"Acme refused allocation"}}),
    )
    .await;
    let (mut worker, result) = running.await.unwrap();
    let error = result.unwrap_err();
    assert_eq!(
        error.partial.as_ref().unwrap()["clientCertificateTunnelRetained"],
        true
    );
    assert_eq!(
        error.partial.as_ref().unwrap()["cleanup"],
        "owned-connection"
    );
    let mut stalled = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    stalled.write_all(&[5]).await.unwrap();
    worker
        .certificate_tunnels
        .close(None, deadline())
        .await
        .unwrap();
    assert!(
        tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
            .await
            .is_err()
    );
    let mut byte = [0];
    match stalled.read(&mut byte).await {
        Ok(0) => {}
        Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset),
        other => panic!("Retained relay survived joined close: {other:?}"),
    }
    worker
        .certificate_tunnels
        .close(None, deadline())
        .await
        .unwrap();
}

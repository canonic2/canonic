//! Context passkeys reconciled into each page's virtual authenticator.
use super::investigation::owned_page;
use super::*;
use crate::browser::credentials::{self, Action, Credential, MAX_CREDENTIALS};
use base64::Engine as _;

fn standard(value: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(credentials::decode(value, 4096).unwrap())
}
fn cdp(credential: &Credential) -> Value {
    json!({"credentialId":standard(&credential.id),"isResidentCredential":true,"rpId":credential.rp_id,
        "privateKey":standard(&credential.private_key),"userHandle":standard(&credential.user_handle),"signCount":credential.sign_count})
}

/// Runs `action` and answers its page reconcile, which finds `held` on the
/// page; returns the credential changes it sent.
async fn reconcile(
    worker: Worker,
    peer: &mut Peer,
    context: Reference,
    action: Action,
    held: &[Value],
    install: bool,
) -> (Worker, Value, Vec<Value>) {
    let task = tokio::spawn(async move {
        let mut worker = worker;
        let result = worker
            .context_credentials(Uuid::new_v4(), context, action, deadline())
            .await;
        (worker, result)
    });
    if install {
        let enable = command(peer, "WebAuthn.enable").await;
        reply(peer, &enable, json!({})).await;
        let add = command(peer, "WebAuthn.addVirtualAuthenticator").await;
        reply(peer, &add, json!({"authenticatorId":"authenticator-acme"})).await;
    }
    let get = command(peer, "WebAuthn.getCredentials").await;
    reply(peer, &get, json!({"credentials":held})).await;
    let mut changes = Vec::new();
    while let Ok(Some(Ok(Message::Text(message)))) =
        tokio::time::timeout(Duration::from_millis(100), peer.next()).await
    {
        let change: Value = serde_json::from_str(&message).unwrap();
        reply(peer, &change, json!({})).await;
        changes.push(change);
    }
    let (worker, result) = task.await.unwrap();
    (
        worker,
        result.unwrap_or_else(|error| panic!("{error:?}")),
        changes,
    )
}
fn create() -> Action {
    Action::Create {
        rp_id: "example.com".into(),
        id: None,
        user_handle: None,
        sign_count: None,
    }
}
fn methods(changes: &[Value]) -> Vec<(&str, &str)> {
    changes
        .iter()
        .map(|change| {
            (
                change["method"].as_str().unwrap(),
                change["params"]["credentialId"]
                    .as_str()
                    .or(change["params"]["credential"]["credentialId"].as_str())
                    .unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn page_passkeys_the_registry_never_recorded_stay_and_dropped_copies_go() {
    let (worker, mut peer, _root, context, _page) = owned_page().await;
    let (worker, _, changes) =
        reconcile(worker, &mut peer, context, Action::Install {}, &[], true).await;
    assert!(changes.is_empty());
    // The page registered a passkey whose event is still queued.
    let own = cdp(&Credential::generate("example.com", None, None, 0).unwrap());
    let (worker, created, changes) = reconcile(
        worker,
        &mut peer,
        context,
        create(),
        std::slice::from_ref(&own),
        false,
    )
    .await;
    let created = created["credential"]["id"].as_str().unwrap().to_owned();
    let created_cdp = standard(&created);
    assert_eq!(
        methods(&changes),
        [("WebAuthn.addCredential", created_cdp.as_str())]
    );
    // Deleting the registry's copy removes it from the page; the page's own
    // passkey is never touched.
    let held = [own, cdp(&worker.context_credential_records(context)[0])];
    let (_, deleted, changes) = reconcile(
        worker,
        &mut peer,
        context,
        Action::Delete { id: created },
        &held,
        false,
    )
    .await;
    assert_eq!(deleted["deleted"], true);
    assert_eq!(
        methods(&changes),
        [("WebAuthn.removeCredential", created_cdp.as_str())]
    );
}

#[tokio::test]
async fn a_merge_over_capacity_refuses_before_any_change() {
    let (worker, mut peer, _root, context, _page) = owned_page().await;
    let (worker, _, _) = reconcile(worker, &mut peer, context, Action::Install {}, &[], true).await;
    let (mut worker, _, _) = reconcile(worker, &mut peer, context, create(), &[], false).await;
    let template = Credential::generate("example.com", None, None, 0).unwrap();
    let restored = (0..MAX_CREDENTIALS)
        .map(|index| Credential {
            id: credentials::encode(&u32::try_from(index).unwrap().to_be_bytes()),
            ..template.clone()
        })
        .collect::<Vec<_>>();
    let error = worker
        .restore_credentials(Uuid::new_v4(), context, restored.clone(), false, deadline())
        .await
        .unwrap_err();
    assert_eq!(error.code, "CREDENTIAL_CAPACITY");
    assert_eq!(worker.context_credential_records(context).len(), 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(25), peer.next())
            .await
            .is_err()
    );
    worker
        .check_restore_capacity(context, &restored, true)
        .unwrap();
}

//! Typed virtual WebAuthn authenticator commands for one owned context.
//! Ordinary requests and results carry credential references, never private
//! keys; key material enters and leaves only through private storage snapshots.
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Credentials one context's authenticator retains.
pub(crate) const MAX_CREDENTIALS: usize = 256;
/// The WebAuthn credential ID length limit.
const MAX_ID_BYTES: usize = 1023;
/// The WebAuthn user handle length limit.
const MAX_USER_HANDLE_BYTES: usize = 64;
const MAX_KEY_BYTES: usize = 4096;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Action {
    /// Pinned `credentials.install()`: current and future pages of the context
    /// use the virtual authenticator.
    Install {},
    /// Pinned `credentials.create(rpId)` with a generated P-256 key. Known keys
    /// are imported through a registered private snapshot instead.
    Create {
        rp_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        user_handle: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sign_count: Option<u32>,
    },
    /// Pinned `credentials.get()`, one keyset page in credential ID order.
    List {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rp_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        limit: Option<u64>,
    },
    /// Pinned `credentials.delete(id)`.
    Delete { id: String },
}

/// A relying party ID is a lowercase DNS name, as pages compute it from their
/// host. Addresses, ports and paths are refused.
pub(crate) fn validate_rp_id(rp_id: &str) -> Result<(), &'static str> {
    let labels = rp_id.split('.').collect::<Vec<_>>();
    let valid = !rp_id.is_empty()
        && rp_id.len() <= 253
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
        && !labels
            .last()
            .is_some_and(|label| label.bytes().all(|byte| byte.is_ascii_digit()));
    valid.then_some(()).ok_or("INVALID_RP_ID")
}

/// Decodes unpadded base64url within `max` decoded bytes.
pub(crate) fn decode(value: &str, max: usize) -> Result<Vec<u8>, &'static str> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| "INVALID_CREDENTIAL_ENCODING")?;
    if bytes.is_empty() || bytes.len() > max {
        return Err("CREDENTIAL_BOUNDS");
    }
    Ok(bytes)
}

pub(crate) fn encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

impl Action {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Install {} => Ok(()),
            Self::Create {
                rp_id,
                id,
                user_handle,
                ..
            } => {
                validate_rp_id(rp_id)?;
                if let Some(id) = id {
                    decode(id, MAX_ID_BYTES)?;
                }
                if let Some(handle) = user_handle {
                    decode(handle, MAX_USER_HANDLE_BYTES)?;
                }
                Ok(())
            }
            Self::List { rp_id, id, .. } => {
                if let Some(rp_id) = rp_id {
                    validate_rp_id(rp_id)?;
                }
                if let Some(id) = id {
                    decode(id, MAX_ID_BYTES)?;
                }
                Ok(())
            }
            Self::Delete { id } => decode(id, MAX_ID_BYTES).map(drop),
        }
    }
}

/// A complete credential in the pinned `VirtualCredential` shape. It holds a
/// private key, so it stays inside the owning backend and private snapshots.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Credential {
    pub id: String,
    pub rp_id: String,
    pub user_handle: String,
    pub private_key: String,
    pub public_key: String,
    pub sign_count: u32,
}
impl Credential {
    /// Validates the pinned shape, encodings and bounds, and that the keys are
    /// one P-256 pair, before anything reaches a browser.
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        validate_rp_id(&self.rp_id)?;
        decode(&self.id, MAX_ID_BYTES)?;
        decode(&self.user_handle, MAX_USER_HANDLE_BYTES)?;
        let private =
            openssl::pkey::PKey::private_key_from_pkcs8(&decode(&self.private_key, MAX_KEY_BYTES)?)
                .map_err(|_| "INVALID_CREDENTIAL_KEY")?;
        let public =
            openssl::pkey::PKey::public_key_from_der(&decode(&self.public_key, MAX_KEY_BYTES)?)
                .map_err(|_| "INVALID_CREDENTIAL_KEY")?;
        let curve = private
            .ec_key()
            .ok()
            .and_then(|key| key.group().curve_name());
        if curve != Some(openssl::nid::Nid::X9_62_PRIME256V1) || !private.public_eq(&public) {
            return Err("INVALID_CREDENTIAL_KEY");
        }
        Ok(())
    }
    /// The reference fields an ordinary result may carry.
    pub(crate) fn public(&self) -> Value {
        json!({"id":self.id,"rpId":self.rp_id,"userHandle":self.user_handle,"publicKey":self.public_key,"signCount":self.sign_count})
    }
    /// A fresh resident P-256 credential, as the pinned `create` generates.
    pub(crate) fn generate(
        rp_id: &str,
        id: Option<&str>,
        user_handle: Option<&str>,
        sign_count: u32,
    ) -> Result<Self, &'static str> {
        let group = openssl::ec::EcGroup::from_curve_name(openssl::nid::Nid::X9_62_PRIME256V1)
            .map_err(|_| "CREDENTIAL_GENERATION_FAILED")?;
        let key = openssl::ec::EcKey::generate(&group)
            .and_then(openssl::pkey::PKey::from_ec_key)
            .map_err(|_| "CREDENTIAL_GENERATION_FAILED")?;
        let private_key = key
            .private_key_to_pkcs8()
            .map_err(|_| "CREDENTIAL_GENERATION_FAILED")?;
        let public_key = key
            .public_key_to_der()
            .map_err(|_| "CREDENTIAL_GENERATION_FAILED")?;
        let random = |length: usize| {
            let mut bytes = vec![0; length];
            openssl::rand::rand_bytes(&mut bytes).map(|()| encode(&bytes))
        };
        let credential = Self {
            id: match id {
                Some(id) => id.into(),
                None => random(16).map_err(|_| "CREDENTIAL_GENERATION_FAILED")?,
            },
            rp_id: rp_id.into(),
            user_handle: match user_handle {
                Some(handle) => handle.into(),
                None => random(16).map_err(|_| "CREDENTIAL_GENERATION_FAILED")?,
            },
            private_key: encode(&private_key),
            public_key: encode(&public_key),
            sign_count,
        };
        credential.validate()?;
        Ok(credential)
    }
    /// The SPKI public key of a PKCS#8 private key, for backends whose
    /// credential records omit it.
    pub(crate) fn public_key_of(private_key: &str) -> Result<String, &'static str> {
        let key = openssl::pkey::PKey::private_key_from_pkcs8(&decode(private_key, MAX_KEY_BYTES)?)
            .map_err(|_| "INVALID_CREDENTIAL_KEY")?;
        key.public_key_to_der()
            .map(|der| encode(&der))
            .map_err(|_| "INVALID_CREDENTIAL_KEY")
    }
}

/// Validates a snapshot's `credentials` array: unique IDs within the bound.
pub(crate) fn validate_all(credentials: &Value) -> Result<Vec<Credential>, &'static str> {
    let credentials: Vec<Credential> =
        serde_json::from_value(credentials.clone()).map_err(|_| "INVALID_STORAGE_STATE")?;
    if credentials.len() > MAX_CREDENTIALS {
        return Err("CREDENTIAL_CAPACITY");
    }
    let mut ids = std::collections::BTreeSet::new();
    for credential in &credentials {
        credential.validate()?;
        if !ids.insert(credential.id.as_str()) {
            return Err("DUPLICATE_CREDENTIAL_ID");
        }
    }
    Ok(credentials)
}

/// One keyset page of public credential views in ID order, bound to the
/// context and filters.
pub(crate) fn page(
    context: super::contract::Reference,
    credentials: Vec<Credential>,
    rp_id: Option<&str>,
    id: Option<&str>,
    cursor: Option<String>,
    limit: Option<u64>,
    installed: bool,
) -> Result<Value, super::session::Error> {
    let rows = credentials
        .into_iter()
        .filter(|credential| rp_id.is_none_or(|rp_id| credential.rp_id == rp_id))
        .filter(|credential| id.is_none_or(|id| credential.id == id))
        .map(|credential| (credential.id.clone().into_bytes(), credential))
        .collect();
    let (items, next) = super::storage::keyset(
        "context_credentials",
        &json!({"context":context,"rpId":rp_id,"id":id}),
        rows,
        |credential| credential.public(),
        cursor.as_deref(),
        limit,
        "browser_send",
    )?;
    Ok(
        json!({"context":context,"credentials":items,"nextCursor":next,"installed":installed,"privateKeysReturned":false}),
    )
}

/// The result of a completed action other than `list`.
pub(crate) fn outcome(
    context: super::contract::Reference,
    action: &Action,
    credential: Option<&Credential>,
    installed: bool,
    deleted: Option<bool>,
) -> Value {
    let mut result = json!({"context":context,"action":action,"installed":installed,"privateKeysReturned":false,"restoration":"close_owned_context"});
    if let Some(credential) = credential {
        result["credential"] = credential.public();
        result["pageVisible"] = json!(installed);
    }
    if let Some(deleted) = deleted {
        result["deleted"] = json!(deleted);
    }
    result
}

pub(crate) fn capabilities(available: bool, chrome: bool) -> Value {
    json!({"available":available,"operations":["install","create","list","delete"],"scope":"owned-context",
        "implementation":if chrome {"cdp-webauthn-virtual-authenticator-per-page"} else {"pinned-playwright-credentials"},
        "algorithm":"ES256","resident":true,"privateKeysInResults":false,"keyImport":"registered-private-storage-snapshot",
        "maxCredentialsPerContext":MAX_CREDENTIALS,"physicalAuthenticators":false,"userVerification":"automatic",
        "pageRegistered":if chrome {"origin-page-immediately-sibling-pages-at-next-context_credentials-or-page-attach"} else {"context-wide"},
        "restoration":"close-owned-context"})
}

pub(crate) fn schema(reference: &Value) -> Value {
    let rp_id = json!({"type":"string","minLength":1,"maxLength":253,"description":"Relying party ID: the site's lowercase domain"});
    // Credential IDs and user handles are unpadded base64url.
    let encoded =
        json!({"type":"string","minLength":1,"maxLength":1364,"pattern":"^[A-Za-z0-9_-]+$"});
    let object = |properties: Value, required: &[&str]| json!({"type":"object","properties":properties,"required":required,"additionalProperties":false});
    let page = crate::pagination::properties(canonic_mcp_kit::page::Limits::STANDARD);
    let actions = vec![
        object(json!({"kind":{"const":"install"}}), &["kind"]),
        object(
            json!({"kind":{"const":"create"},"rpId":rp_id,"id":encoded,"userHandle":encoded,"signCount":{"type":"integer","minimum":0,"maximum":4_294_967_295_u64}}),
            &["kind", "rpId"],
        ),
        object(
            json!({"kind":{"const":"list"},"rpId":rp_id,"id":encoded,"cursor":page["cursor"],"limit":page["limit"]}),
            &["kind"],
        ),
        object(
            json!({"kind":{"const":"delete"},"id":encoded}),
            &["kind", "id"],
        ),
    ];
    object(
        json!({"operation":{"const":"context_credentials"},"context":reference,"action":{"oneOf":actions}}),
        &["operation", "context", "action"],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relying_party_ids_are_lowercase_dns_names() {
        for valid in [
            "example.com",
            "localhost",
            "app.acme.example.com",
            "a-b.example",
        ] {
            assert_eq!(validate_rp_id(valid), Ok(()), "{valid}");
        }
        for invalid in [
            "",
            "Example.com",
            "example.com:443",
            "https://example.com",
            "127.0.0.1",
            "-acme.example",
            "acme..example",
            "example.com/",
        ] {
            assert_eq!(validate_rp_id(invalid), Err("INVALID_RP_ID"), "{invalid}");
        }
    }

    #[test]
    fn generated_credentials_are_valid_p256_pairs_whose_public_views_omit_private_keys() {
        let credential = Credential::generate("example.com", None, None, 7).unwrap();
        assert_eq!(credential.rp_id, "example.com");
        assert_eq!(credential.sign_count, 7);
        assert_eq!(decode(&credential.id, MAX_ID_BYTES).unwrap().len(), 16);
        assert_eq!(
            Credential::public_key_of(&credential.private_key).unwrap(),
            credential.public_key
        );
        let public = credential.public();
        assert!(public.get("privateKey").is_none());
        assert!(!public.to_string().contains(&credential.private_key));
        let fixed = Credential::generate("example.com", Some("YWNtZQ"), Some("dXNlcg"), 0).unwrap();
        assert_eq!(
            (fixed.id.as_str(), fixed.user_handle.as_str()),
            ("YWNtZQ", "dXNlcg")
        );
    }

    #[test]
    fn mismatched_or_foreign_keys_and_duplicate_ids_are_refused() {
        let first = Credential::generate("example.com", None, None, 0).unwrap();
        let second = Credential::generate("example.com", None, None, 0).unwrap();
        let mut mismatched = first.clone();
        mismatched.public_key = second.public_key.clone();
        assert_eq!(mismatched.validate(), Err("INVALID_CREDENTIAL_KEY"));
        let rsa = openssl::rsa::Rsa::generate(2048)
            .and_then(openssl::pkey::PKey::from_rsa)
            .unwrap();
        let mut foreign = first.clone();
        foreign.private_key = encode(&rsa.private_key_to_pkcs8().unwrap());
        foreign.public_key = encode(&rsa.public_key_to_der().unwrap());
        assert_eq!(foreign.validate(), Err("INVALID_CREDENTIAL_KEY"));
        let mut padded = first.clone();
        padded.id = format!("{}=", first.id);
        assert_eq!(padded.validate(), Err("INVALID_CREDENTIAL_ENCODING"));
        assert_eq!(
            validate_all(&json!([first, first])),
            Err("DUPLICATE_CREDENTIAL_ID")
        );
        assert_eq!(validate_all(&json!([first, second])).unwrap().len(), 2);
        assert_eq!(
            validate_all(&json!([{"id":"YQ","rpId":"example.com"}])),
            Err("INVALID_STORAGE_STATE")
        );
    }

    #[test]
    fn actions_refuse_key_import_and_unknown_fields() {
        let import =
            json!({"kind":"create","rpId":"example.com","privateKey":"YQ","publicKey":"YQ"});
        assert!(serde_json::from_value::<Action>(import).is_err());
        let create: Action =
            serde_json::from_value(json!({"kind":"create","rpId":"example.com","signCount":4}))
                .unwrap();
        assert_eq!(create.validate(), Ok(()));
        let invalid: Action =
            serde_json::from_value(json!({"kind":"delete","id":"not base64!"})).unwrap();
        assert_eq!(invalid.validate(), Err("INVALID_CREDENTIAL_ENCODING"));
        let oversized: Action = serde_json::from_value(
            json!({"kind":"create","rpId":"example.com","userHandle":encode(&[1; 65])}),
        )
        .unwrap();
        assert_eq!(oversized.validate(), Err("CREDENTIAL_BOUNDS"));
    }

    #[test]
    fn credential_pages_follow_id_order_with_bound_cursors_and_no_private_keys() {
        let context = super::super::contract::Reference {
            connection_id: uuid::Uuid::new_v4(),
            id: uuid::Uuid::new_v4(),
            generation: uuid::Uuid::new_v4(),
        };
        let credentials = ["Yw", "YQ", "Yg"]
            .into_iter()
            .map(|id| Credential::generate("example.com", Some(id), None, 0).unwrap())
            .chain([Credential::generate("acme.example", Some("ZA"), None, 0).unwrap()])
            .collect::<Vec<_>>();
        let first = page(
            context,
            credentials.clone(),
            Some("example.com"),
            None,
            None,
            Some(2),
            true,
        )
        .unwrap();
        let ids = |page: &Value| {
            page["credentials"]
                .as_array()
                .unwrap()
                .iter()
                .map(|credential| credential["id"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&first), ["YQ", "Yg"]);
        assert!(
            first["credentials"]
                .as_array()
                .unwrap()
                .iter()
                .all(|credential| credential.get("privateKey").is_none())
        );
        assert!(
            credentials
                .iter()
                .all(|credential| !first.to_string().contains(&credential.private_key))
        );
        let cursor = first["nextCursor"].as_str().unwrap().to_owned();
        let second = page(
            context,
            credentials.clone(),
            Some("example.com"),
            None,
            Some(cursor.clone()),
            Some(2),
            true,
        )
        .unwrap();
        assert_eq!(ids(&second), ["Yw"]);
        assert!(second["nextCursor"].is_null());
        assert!(
            page(
                context,
                credentials,
                None,
                None,
                Some(cursor),
                Some(2),
                true
            )
            .is_err()
        );
    }
}

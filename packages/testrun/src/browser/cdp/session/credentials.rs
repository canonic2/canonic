//! Context virtual WebAuthn authenticators over native CDP. Chrome holds one
//! virtual authenticator per page target, so the context's credential
//! registry is owned here and reconciled into each page's authenticator.
use super::*;
use crate::browser::credentials::{self, Action, Credential, MAX_CREDENTIALS};
use base64::Engine as _;

/// Pages with an authenticator in one context.
const MAX_PAGES: usize = 512;

#[derive(Default)]
struct Authenticator {
    installed: bool,
    /// The context's credentials by ID, private keys included.
    registry: BTreeMap<String, Credential>,
    /// Page sessions and their authenticator IDs; `None` while being added.
    pages: BTreeMap<String, Option<String>>,
    /// A page added or changed a credential we could not record.
    untracked: u64,
    /// Page credentials being replaced with the registry's key; their
    /// deletion events leave the registry unchanged.
    replacing: BTreeSet<(String, String)>,
    /// Page credentials that are the registry's copy. Only these are removed
    /// when the registry drops them; any other page credential stays.
    synced: BTreeSet<(String, String)>,
}
#[derive(Default)]
pub(super) struct Authenticators(BTreeMap<Reference, Authenticator>);

fn standard(value: &str) -> Result<String, Error> {
    Ok(base64::engine::general_purpose::STANDARD
        .encode(credentials::decode(value, 4096).map_err(Error::from)?))
}
/// A pinned-shape credential from a CDP `WebAuthn.Credential`. Credentials
/// without a user handle are not discoverable and stay with their page.
fn from_cdp(credential: &Value) -> Option<Credential> {
    let url = |field: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(credential[field].as_str()?)
            .ok()
            .map(|bytes| credentials::encode(&bytes))
    };
    let private_key = url("privateKey")?;
    let converted = Credential {
        id: url("credentialId")?,
        rp_id: credential["rpId"].as_str()?.into(),
        user_handle: url("userHandle")?,
        public_key: Credential::public_key_of(&private_key).ok()?,
        private_key,
        sign_count: u32::try_from(credential["signCount"].as_u64()?).ok()?,
    };
    converted.validate().ok().map(|()| converted)
}
fn to_cdp(credential: &Credential) -> Result<Value, Error> {
    Ok(
        json!({"credentialId":standard(&credential.id)?,"isResidentCredential":true,"rpId":credential.rp_id,
        "privateKey":standard(&credential.private_key)?,"userHandle":standard(&credential.user_handle)?,"signCount":credential.sign_count}),
    )
}

impl Worker {
    /// Contexts and page attachments that ended release their authenticators.
    pub(super) fn fence_credentials(&mut self) {
        self.authenticators.0.retain(|context, authenticator| {
            authenticator
                .pages
                .retain(|session, _| self.index.retained_attachment(session));
            let pages = &authenticator.pages;
            authenticator
                .replacing
                .retain(|(session, _)| pages.contains_key(session));
            authenticator
                .synced
                .retain(|(session, _)| pages.contains_key(session));
            self.index.retained(*context, Kind::Context)
        });
    }
    /// Records credentials a page registers or uses, so sibling pages and
    /// snapshots see them. The event's page keeps its own copy.
    pub(super) fn credential_event(&mut self, event: &Event) {
        if !event.method.starts_with("WebAuthn.credential") {
            return;
        }
        let Some(session) = event.session_id.as_deref() else {
            return;
        };
        let Some(authenticator) = self.authenticators.0.values_mut().find(|authenticator| {
            authenticator
                .pages
                .get(session)
                .is_some_and(|id| id.as_deref() == event.params["authenticatorId"].as_str())
        }) else {
            return;
        };
        if event.method == "WebAuthn.credentialDeleted" {
            if let Some(id) = event.params["credentialId"]
                .as_str()
                .and_then(|id| base64::engine::general_purpose::STANDARD.decode(id).ok())
            {
                let key = (session.to_owned(), credentials::encode(&id));
                if !authenticator.replacing.remove(&key) {
                    authenticator.synced.remove(&key);
                    authenticator.registry.remove(&key.1);
                }
            }
            return;
        }
        match from_cdp(&event.params["credential"]) {
            Some(credential)
                if authenticator.registry.contains_key(&credential.id)
                    || authenticator.registry.len() < MAX_CREDENTIALS =>
            {
                authenticator
                    .synced
                    .insert((session.to_owned(), credential.id.clone()));
                authenticator
                    .registry
                    .insert(credential.id.clone(), credential);
            }
            _ => authenticator.untracked = authenticator.untracked.saturating_add(1),
        }
    }
    /// The sessions of the context's live pages.
    fn credential_pages(&self, context: Reference) -> Vec<String> {
        self.index
            .targets()
            .into_iter()
            .filter(|(_, kind, _, _)| *kind == Kind::Page)
            .filter_map(|(_, _, _, session)| session)
            .filter(|session| {
                self.index
                    .diagnostic_scope(session)
                    .is_some_and(|(owner, _, _)| owner == context)
            })
            .collect()
    }
    /// The page's authenticator ID, adding the authenticator when missing.
    async fn page_authenticator(
        &mut self,
        id: Uuid,
        context: Reference,
        session: &str,
        deadline: Instant,
    ) -> Result<String, Error> {
        let authenticator = self
            .authenticators
            .0
            .get_mut(&context)
            .ok_or("AUTHENTICATOR_NOT_INSTALLED")?;
        let existing = match authenticator.pages.get(session) {
            Some(None) => return Err("AUTHENTICATOR_PENDING".into()),
            Some(Some(existing)) => Some(existing.clone()),
            None if authenticator.pages.len() >= MAX_PAGES => {
                return Err("AUTHENTICATOR_PAGE_CAPACITY".into());
            }
            None => None,
        };
        if let Some(existing) = existing {
            return Ok(existing);
        }
        authenticator.pages.insert(session.into(), None);
        self.call(
            id,
            Some(session),
            "WebAuthn.enable",
            json!({"enableUI":false}),
            deadline,
        )
        .await?;
        let added = self
            .call(
                id,
                Some(session),
                "WebAuthn.addVirtualAuthenticator",
                json!({"options":{"protocol":"ctap2","ctap2Version":"ctap2_1","transport":"internal","hasResidentKey":true,"hasUserVerification":true,"isUserVerified":true,"automaticPresenceSimulation":true}}),
                deadline,
            )
            .await?;
        let added = added["authenticatorId"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 1024)
            .ok_or("BACKEND_PROTOCOL")?
            .to_owned();
        self.authenticators
            .0
            .get_mut(&context)
            .ok_or("AUTHENTICATOR_NOT_INSTALLED")?
            .pages
            .insert(session.into(), Some(added.clone()));
        Ok(added)
    }
    /// Adds the page's authenticator when missing, then adds the registry's
    /// missing credentials and removes the registry copies it dropped.
    /// A page's own copy of a held credential stays: Chrome reports a removal
    /// as `credentialDeleted`, so replacing it would delete it from the
    /// registry. Sign counts advance through `credentialAsserted` instead.
    async fn reconcile_authenticator(
        &mut self,
        id: Uuid,
        context: Reference,
        session: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        let authenticator_id = self
            .page_authenticator(id, context, session, deadline)
            .await?;
        let current = self
            .call_read(
                id,
                Some(session),
                "WebAuthn.getCredentials",
                json!({"authenticatorId":authenticator_id}),
                deadline,
            )
            .await?;
        let current = current["credentials"]
            .as_array()
            .ok_or("BACKEND_PROTOCOL")?
            .iter()
            .map(|credential| {
                let id = credential["credentialId"]
                    .as_str()
                    .and_then(|id| base64::engine::general_purpose::STANDARD.decode(id).ok())
                    .ok_or("BACKEND_PROTOCOL")?;
                Ok((credentials::encode(&id), from_cdp(credential)))
            })
            .collect::<Result<BTreeMap<_, _>, Error>>()?;
        let held = self
            .authenticators
            .0
            .get(&context)
            .ok_or("AUTHENTICATOR_NOT_INSTALLED")?;
        let registry = held.registry.clone();
        // Page credentials the registry never held, such as ones a full
        // registry could not record or whose events are still queued, stay
        // with their page. A held ID with another key, as a restored snapshot
        // can bring, is replaced; its deletion event is then not a registry
        // deletion.
        let dropped = |credential_id: &String| {
            !registry.contains_key(credential_id)
                && held
                    .synced
                    .contains(&(session.to_owned(), credential_id.clone()))
        };
        let rekeyed = current
            .iter()
            .filter(|(credential_id, credential)| {
                credential
                    .as_ref()
                    .zip(registry.get(*credential_id))
                    .is_some_and(|(credential, held)| held.private_key != credential.private_key)
            })
            .map(|(credential_id, _)| credential_id.clone())
            .collect::<BTreeSet<_>>();
        let removed = current
            .iter()
            .filter(|(credential_id, credential)| {
                credential.is_some() && (rekeyed.contains(*credential_id) || dropped(credential_id))
            })
            .map(|(credential_id, _)| credential_id.clone())
            .collect::<Vec<_>>();
        let authenticator = self
            .authenticators
            .0
            .get_mut(&context)
            .ok_or("AUTHENTICATOR_NOT_INSTALLED")?;
        authenticator.replacing.extend(
            rekeyed
                .iter()
                .map(|credential_id| (session.to_owned(), credential_id.clone())),
        );
        // Once reconciled, the page holds exactly the registry's copies.
        authenticator.synced.retain(|(page, _)| page != session);
        authenticator.synced.extend(
            registry
                .keys()
                .map(|credential_id| (session.to_owned(), credential_id.clone())),
        );
        for credential_id in removed {
            self.call(
                id,
                Some(session),
                "WebAuthn.removeCredential",
                json!({"authenticatorId":authenticator_id,"credentialId":standard(&credential_id)?}),
                deadline,
            )
            .await?;
        }
        for (credential_id, credential) in &registry {
            if rekeyed.contains(credential_id) || !current.contains_key(credential_id) {
                self.call(
                    id,
                    Some(session),
                    "WebAuthn.addCredential",
                    json!({"authenticatorId":authenticator_id,"credential":to_cdp(credential)?}),
                    deadline,
                )
                .await?;
            }
        }
        self.index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        if !self.index.retained_attachment(session) {
            return Err("STALE_REFERENCE".into());
        }
        Ok(())
    }
    /// Reconciles every live page of an installed context.
    async fn reconcile_authenticators(
        &mut self,
        id: Uuid,
        context: Reference,
        deadline: Instant,
    ) -> Result<Vec<String>, Error> {
        if !self
            .authenticators
            .0
            .get(&context)
            .is_some_and(|authenticator| authenticator.installed)
        {
            return Ok(Vec::new());
        }
        let pages = self.credential_pages(context);
        for session in &pages {
            self.reconcile_authenticator(id, context, session, deadline)
                .await?;
        }
        Ok(pages)
    }
    /// New pages of an installed context receive its authenticator before
    /// their first document runs.
    pub(super) async fn configure_credentials(
        &mut self,
        id: Uuid,
        kind: Kind,
        session: &str,
        deadline: Instant,
    ) -> Result<(), Error> {
        if kind != Kind::Page {
            return Ok(());
        }
        let Some((context, _, _)) = self.index.diagnostic_scope(session) else {
            return Ok(());
        };
        if !self
            .authenticators
            .0
            .get(&context)
            .is_some_and(|authenticator| authenticator.installed)
        {
            return Ok(());
        }
        self.reconcile_authenticator(id, context, session, deadline)
            .await
    }
    /// The context's credentials, for a private snapshot.
    pub(super) fn context_credential_records(&self, context: Reference) -> Vec<Credential> {
        self.authenticators
            .0
            .get(&context)
            .map(|authenticator| authenticator.registry.values().cloned().collect())
            .unwrap_or_default()
    }
    /// Records queued page credential events, then refuses a restore that
    /// would leave the registry over capacity, before anything changes.
    pub(super) fn check_restore_capacity(
        &mut self,
        context: Reference,
        restored: &[Credential],
        replace: bool,
    ) -> Result<(), Error> {
        self.drain()?;
        self.fence_credentials();
        let kept = match self.authenticators.0.get(&context) {
            Some(authenticator) if !replace => authenticator
                .registry
                .keys()
                .filter(|held| !restored.iter().any(|credential| &credential.id == *held))
                .count(),
            _ => 0,
        };
        if kept + restored.len() > MAX_CREDENTIALS {
            return Err("CREDENTIAL_CAPACITY".into());
        }
        Ok(())
    }
    /// Restores snapshot credentials as the pinned `setStorageState` does:
    /// `replace` swaps the registry and installs, or uninstalls when the
    /// snapshot has none; `merge` upserts and installs.
    pub(super) async fn restore_credentials(
        &mut self,
        id: Uuid,
        context: Reference,
        restored: Vec<Credential>,
        replace: bool,
        deadline: Instant,
    ) -> Result<&'static str, Error> {
        self.check_restore_capacity(context, &restored, replace)?;
        if restored.is_empty() {
            if !replace {
                return Ok("unchanged");
            }
            let Some(authenticator) = self.authenticators.0.remove(&context) else {
                return Ok("unchanged");
            };
            for (session, authenticator_id) in authenticator.pages {
                if let Some(authenticator_id) = authenticator_id
                    && self.index.retained_attachment(&session)
                {
                    self.call(
                        id,
                        Some(&session),
                        "WebAuthn.removeVirtualAuthenticator",
                        json!({"authenticatorId":authenticator_id}),
                        deadline,
                    )
                    .await?;
                }
            }
            return Ok("removed");
        }
        let authenticator = self.authenticators.0.entry(context).or_default();
        if replace {
            authenticator.registry.clear();
        }
        for credential in restored {
            authenticator
                .registry
                .insert(credential.id.clone(), credential);
        }
        authenticator.installed = true;
        self.reconcile_authenticators(id, context, deadline).await?;
        Ok(if replace { "restored" } else { "merged" })
    }
    pub(super) async fn context_credentials(
        &mut self,
        id: Uuid,
        context: Reference,
        action: Action,
        deadline: Instant,
    ) -> Result<Value, Error> {
        action.validate()?;
        self.drain()?;
        self.index
            .native(context, Kind::Context)
            .map_err(resource_error)?;
        self.fence_credentials();
        if Instant::now() >= deadline {
            return Err("NOT_DISPATCHED".into());
        }
        let result = self
            .context_credentials_step(id, context, &action, deadline)
            .await;
        result.map_err(|mut error| {
            let cause = error.partial.take();
            error.partial = Some(json!({"context":context,"action":action,"privateKeysReturned":false,"stateMayHaveChanged":true,"replayed":false,"restoration":"close_owned_context","cause":cause}));
            error
        })
    }
    async fn context_credentials_step(
        &mut self,
        id: Uuid,
        context: Reference,
        action: &Action,
        deadline: Instant,
    ) -> Result<Value, Error> {
        match action {
            Action::Install {} => {
                self.authenticators.0.entry(context).or_default().installed = true;
                let pages = self.reconcile_authenticators(id, context, deadline).await?;
                let mut result = credentials::outcome(context, action, None, true, None);
                result["backend"] = json!("chrome_cdp");
                result["pages"] = json!(pages.len());
                Ok(result)
            }
            Action::Create {
                rp_id,
                id: credential_id,
                user_handle,
                sign_count,
            } => {
                let authenticator = self.authenticators.0.entry(context).or_default();
                if authenticator.registry.len() >= MAX_CREDENTIALS {
                    return Err("CREDENTIAL_CAPACITY".into());
                }
                if credential_id
                    .as_ref()
                    .is_some_and(|credential| authenticator.registry.contains_key(credential))
                {
                    return Err("DUPLICATE_CREDENTIAL_ID".into());
                }
                let credential = Credential::generate(
                    rp_id,
                    credential_id.as_deref(),
                    user_handle.as_deref(),
                    sign_count.unwrap_or(0),
                )?;
                authenticator
                    .registry
                    .insert(credential.id.clone(), credential.clone());
                let installed = authenticator.installed;
                self.reconcile_authenticators(id, context, deadline).await?;
                Ok(credentials::outcome(
                    context,
                    action,
                    Some(&credential),
                    installed,
                    None,
                ))
            }
            Action::List {
                rp_id,
                id: credential_id,
                cursor,
                limit,
            } => {
                let installed = self
                    .authenticators
                    .0
                    .get(&context)
                    .is_some_and(|authenticator| authenticator.installed);
                self.reconcile_authenticators(id, context, deadline).await?;
                let mut page = credentials::page(
                    context,
                    self.context_credential_records(context),
                    rp_id.as_deref(),
                    credential_id.as_deref(),
                    cursor.clone(),
                    *limit,
                    installed,
                )?;
                page["untrackedPageCredentials"] = json!(
                    self.authenticators
                        .0
                        .get(&context)
                        .map_or(0, |authenticator| authenticator.untracked)
                );
                Ok(page)
            }
            Action::Delete { id: credential_id } => {
                let (deleted, installed) = match self.authenticators.0.get_mut(&context) {
                    Some(authenticator) => (
                        authenticator.registry.remove(credential_id).is_some(),
                        authenticator.installed,
                    ),
                    None => (false, false),
                };
                self.reconcile_authenticators(id, context, deadline).await?;
                Ok(credentials::outcome(
                    context,
                    action,
                    None,
                    installed,
                    Some(deleted),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cdp_credentials_convert_to_the_pinned_shape_and_back() {
        let credential = Credential::generate("example.com", None, None, 5).unwrap();
        let native = to_cdp(&credential).unwrap();
        assert_eq!(native["isResidentCredential"], true);
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(native["credentialId"].as_str().unwrap())
                .unwrap(),
            credentials::decode(&credential.id, 1023).unwrap()
        );
        assert_eq!(from_cdp(&native), Some(credential.clone()));
        let mut discoverable = native.clone();
        discoverable.as_object_mut().unwrap().remove("userHandle");
        assert_eq!(from_cdp(&discoverable), None);
        let mut corrupt = native;
        corrupt["privateKey"] = json!("AAAA");
        assert_eq!(from_cdp(&corrupt), None);
    }
}

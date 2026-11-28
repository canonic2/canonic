//! Provisioning validation for the owned runner.
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::time::SystemTime;

pub(super) const PERSONAL_WARNING: &str = "If you use a free Personal Team, its provisioning profile expires 7 days after issuance. Renew it in Xcode and run setup again after expiry.";

pub(super) fn profile(
    profile: &plist::Value,
    team: &str,
    serial: &str,
    bundle: &str,
    now: SystemTime,
) -> Result<Value> {
    let invalid = || {
        Error::new(
            "RUNNER_PROFILE_INVALID",
            "The runner provisioning profile does not authorize this team, device and app; renew its development profile in Xcode",
        )
    };
    let dictionary = profile.as_dictionary().ok_or_else(invalid)?;
    let expiration: SystemTime = dictionary
        .get("ExpirationDate")
        .and_then(plist::Value::as_date)
        .ok_or_else(invalid)?
        .into();
    if expiration <= now {
        return Err(Error::new(
            "RUNNER_PROFILE_EXPIRED",
            "The runner provisioning profile has expired; renew it in Xcode and run setup again",
        )
        .details(json!({"warning":PERSONAL_WARNING})));
    }
    let contains = |field: &str, expected: &str| {
        dictionary
            .get(field)
            .and_then(plist::Value::as_array)
            .is_some_and(|values| {
                values
                    .iter()
                    .any(|value| value.as_string() == Some(expected))
            })
    };
    let entitlements = dictionary
        .get("Entitlements")
        .and_then(plist::Value::as_dictionary)
        .ok_or_else(invalid)?;
    let app = entitlements
        .get("application-identifier")
        .and_then(plist::Value::as_string)
        .ok_or_else(invalid)?;
    // Older App ID prefixes can differ from the TeamIdentifier.
    let matches_app = dictionary
        .get("ApplicationIdentifierPrefix")
        .and_then(plist::Value::as_array)
        .is_some_and(|prefixes| {
            prefixes
                .iter()
                .filter_map(plist::Value::as_string)
                .any(|prefix| {
                    let expected = format!("{prefix}.{bundle}");
                    app == expected
                        || app.strip_suffix('*').is_some_and(|prefix| {
                            prefix.ends_with('.') && expected.starts_with(prefix)
                        })
                })
        });
    if !contains("TeamIdentifier", team)
        || !contains("ProvisionedDevices", serial)
        || !matches_app
        || entitlements
            .get("get-task-allow")
            .and_then(plist::Value::as_boolean)
            != Some(true)
        || dictionary
            .get("DeveloperCertificates")
            .and_then(plist::Value::as_array)
            .is_none_or(Vec::is_empty)
    {
        return Err(invalid());
    }
    Ok(
        json!({"profileExpiresAt":time::OffsetDateTime::from(expiration).format(&time::format_description::well_known::Rfc3339).map_err(|_|invalid())?,"warning":PERSONAL_WARNING}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn rejects_expired_or_mismatched_profiles() {
        let now = SystemTime::now();
        let value = json!({"TeamIdentifier":["ACME000001"],"ApplicationIdentifierPrefix":["PREFIX0001"],"ProvisionedDevices":["AcmePhone"],"Entitlements":{"application-identifier":"PREFIX0001.com.example.acme","get-task-allow":true},"DeveloperCertificates":["fixture"]});
        let mut bytes = Vec::new();
        plist::to_writer_xml(&mut bytes, &value).unwrap();
        let mut p: plist::Value = plist::from_bytes(&bytes).unwrap();
        p.as_dictionary_mut().unwrap().insert(
            "ExpirationDate".into(),
            plist::Value::Date((now + Duration::from_secs(3600)).into()),
        );
        profile(&p, "ACME000001", "AcmePhone", "com.example.acme", now).unwrap();
        for (team, serial, bundle) in [
            ("ACME000002", "AcmePhone", "com.example.acme"),
            ("ACME000001", "OtherPhone", "com.example.acme"),
            ("ACME000001", "AcmePhone", "com.example.other"),
        ] {
            assert_eq!(
                profile(&p, team, serial, bundle, now).unwrap_err().code,
                "RUNNER_PROFILE_INVALID"
            );
        }
        p.as_dictionary_mut()
            .unwrap()
            .insert("ExpirationDate".into(), plist::Value::Date(now.into()));
        assert_eq!(
            profile(&p, "ACME000001", "AcmePhone", "com.example.acme", now)
                .unwrap_err()
                .code,
            "RUNNER_PROFILE_EXPIRED"
        );
    }
}

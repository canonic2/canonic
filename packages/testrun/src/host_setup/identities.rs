//! Read eligible development teams from local certificate metadata on macOS.
use super::Teams;
use crate::{
    error::{Error, Result},
    process,
};
use std::{collections::BTreeMap, time::Duration};

pub(super) async fn discover() -> Result<Teams> {
    if !cfg!(target_os = "macos") {
        return Err(Error::new(
            "UNSUPPORTED_HOST",
            "Runner signing requires macOS and Xcode",
        ));
    }
    let output = process::run(
        "/usr/bin/security",
        &["find-identity", "-v", "-p", "codesigning"],
        Duration::from_secs(10),
        64 * 1024,
    )
    .await?;
    let identities = parse_identities(&String::from_utf8_lossy(&output.bytes));
    let certificates = process::run(
        "/usr/bin/security",
        &["find-certificate", "-a", "-p"],
        Duration::from_secs(10),
        4 * 1024 * 1024,
    )
    .await?;
    let pem = String::from_utf8_lossy(&certificates.bytes);
    let mut teams = Teams::new();
    for part in pem.split_inclusive("-----END CERTIFICATE-----") {
        let Some(start) = part.find("-----BEGIN CERTIFICATE-----") else {
            continue;
        };
        let info = process::run_input(
            "/usr/bin/openssl",
            &[
                "x509",
                "-noout",
                "-subject",
                "-fingerprint",
                "-sha1",
                "-nameopt",
                "RFC2253",
            ],
            &part.as_bytes()[start..],
            Duration::from_secs(3),
            16384,
        )
        .await?;
        if let Some((team, label)) =
            certificate_team(&String::from_utf8_lossy(&info.bytes), &identities)
        {
            teams.entry(team).or_insert(label);
        }
    }
    Ok(teams)
}

fn parse_identities(output: &str) -> BTreeMap<String, String> {
    output
        .lines()
        .filter_map(|line| {
            let (prefix, name) = line.split_once('"')?;
            let name = name.strip_suffix('"')?;
            if !name.starts_with("Apple Development:") && !name.starts_with("iPhone Developer:") {
                return None;
            }
            let hash = prefix.split_once(')')?.1.split_whitespace().next()?;
            (hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
                .then(|| (hash.to_ascii_uppercase(), name.to_owned()))
        })
        .collect()
}

/// Label a team with its certificate organization, the name Apple shows for the
/// team, falling back to the identity name when the subject has none.
fn certificate_team(info: &str, identities: &BTreeMap<String, String>) -> Option<(String, String)> {
    let fingerprint = info.lines().find_map(|line| {
        line.split_once("Fingerprint=")
            .map(|(_, value)| value.replace(':', "").to_ascii_uppercase())
    })?;
    let identity = identities.get(&fingerprint)?;
    let subject = info
        .lines()
        .find_map(|line| line.strip_prefix("subject=").map(str::trim))?;
    let attributes = subject_attributes(subject);
    let attribute = |key: &str| {
        attributes
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    };
    let team = attribute("OU").filter(|team| super::validate_team(team).is_ok())?;
    let label = attribute("O")
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(identity);
    Some((team.clone(), label.clone()))
}

/// Split an RFC 2253 subject into unescaped attribute pairs, decoding `\,`-style
/// and hex (`\C3\A9`) escapes.
fn subject_attributes(subject: &str) -> Vec<(String, String)> {
    let mut attributes = Vec::new();
    let mut current = Vec::new();
    let mut bytes = subject.bytes().peekable();
    while let Some(byte) = bytes.next() {
        match byte {
            b'\\' => {
                let Some(next) = bytes.next() else { break };
                let hex = |b: u8| char::from(b).to_digit(16);
                match (hex(next), bytes.peek().copied().and_then(hex)) {
                    (Some(high), Some(low)) => {
                        bytes.next();
                        current.push(u8::try_from(high * 16 + low).unwrap_or(b'?'));
                    }
                    _ => current.push(next),
                }
            }
            b',' => attributes.push(std::mem::take(&mut current)),
            _ => current.push(byte),
        }
    }
    attributes.push(current);
    attributes
        .into_iter()
        .filter_map(|part| {
            let part = String::from_utf8_lossy(&part).into_owned();
            let (name, value) = part.split_once('=')?;
            Some((name.trim().to_owned(), value.to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uses_certificate_team_metadata_and_only_valid_development_identities() {
        let output = format!(
            " 1) {} \"Apple Development: Acme (PERSON0001)\"\n 2) {} \"Apple Distribution: Acme (PERSON0002)\"",
            "A".repeat(40),
            "B".repeat(40)
        );
        let identities = parse_identities(&output);
        assert_eq!(identities.len(), 1);
        let certificate = format!(
            "subject=CN=Apple Development: Acme (PERSON0001),OU=ACME000001,O=Acme\nSHA1 Fingerprint={}",
            "AA:".repeat(20).trim_end_matches(':')
        );
        assert_eq!(
            certificate_team(&certificate, &identities).unwrap(),
            ("ACME000001".into(), "Acme".into())
        );
        assert!(certificate_team(&certificate.replace("AA:", "BB:"), &identities).is_none());
    }

    #[test]
    fn labels_teams_with_unescaped_organization_or_identity() {
        let identities = BTreeMap::from([(
            "A".repeat(40),
            "Apple Development: Acme (PERSON0001)".to_owned(),
        )]);
        let certificate = |subject: &str| {
            format!(
                "subject={subject}\nSHA1 Fingerprint={}",
                "AA:".repeat(20).trim_end_matches(':')
            )
        };
        let team = |subject: &str| certificate_team(&certificate(subject), &identities);
        assert_eq!(
            team(
                r"UID=PERSON0001,CN=Apple Development: Acme (PERSON0001),OU=ACME000001,O=Acme\, Inc.,C=US"
            ),
            Some(("ACME000001".into(), "Acme, Inc.".into()))
        );
        assert_eq!(
            team(r"CN=Apple Development: Acme (PERSON0001),OU=ACME000001,O=Acm\C3\A9 Studio"),
            Some(("ACME000001".into(), "Acmé Studio".into()))
        );
        assert_eq!(
            team("CN=Apple Development: Acme (PERSON0001),OU=ACME000001,C=US"),
            Some((
                "ACME000001".into(),
                "Apple Development: Acme (PERSON0001)".into()
            ))
        );
        assert_eq!(
            team(r"CN=Apple Development: Acme\, OU=ACME000009 (PERSON0001),OU=ACME000001,O=Acme"),
            Some(("ACME000001".into(), "Acme".into()))
        );
        assert!(team("CN=Apple Development: Acme (PERSON0001),OU=acme,O=Acme").is_none());
    }
}

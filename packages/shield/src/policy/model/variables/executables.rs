//! Pure declarations for executable discovery during reviewed pack setup.
use crate::policy::Error;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged, deny_unknown_fields)]
pub(crate) enum Candidate {
    Home { home: String },
    Environment { env: String, path: String },
    PathSearch { path_search: String },
}

impl Candidate {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        let path = match self {
            Self::Home { home } => home,
            Self::Environment { env, path } => {
                if env.is_empty()
                    || env.len() > 64
                    || !env
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                {
                    return Err(Error::Contract(
                        "invalid executable discovery environment name",
                    ));
                }
                path
            }
            Self::PathSearch { path_search } => {
                if path_search.is_empty()
                    || path_search.len() > 128
                    || !path_search.as_bytes()[0].is_ascii_alphanumeric()
                    || !path_search
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
                {
                    return Err(Error::Contract(
                        "PATH discovery requires a literal executable name",
                    ));
                }
                return Ok(());
            }
        };
        if path.len() > 1024 {
            return Err(Error::Limit);
        }
        if path.contains(['\\', ':', '\0'])
            || path.split('/').any(|part| matches!(part, "" | "." | ".."))
        {
            return Err(Error::Contract(
                "executable discovery paths use relative segments separated by /",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn discovery_candidates_use_one_portable_path_grammar() {
        for path in [
            "",
            "/bin/acme",
            "bin/../acme",
            "bin//acme",
            "bin/./acme",
            r"bin\acme",
            r"C:\acme",
            "C:acme",
        ] {
            assert!(
                Candidate::Home { home: path.into() }.validate().is_err(),
                "{path}"
            );
        }
        for env in ["", "Acme", "ACME-HOME", "ACME=HOME"] {
            assert!(
                Candidate::Environment {
                    env: env.into(),
                    path: "bin/acme".into()
                }
                .validate()
                .is_err(),
                "{env}"
            );
        }
        assert!(
            Candidate::Home {
                home: ".acme/bin/acme".into()
            }
            .validate()
            .is_ok()
        );
        assert!(
            serde_json::from_value::<Candidate>(json!({"home":"bin/acme","extra":true})).is_err()
        );
    }
}

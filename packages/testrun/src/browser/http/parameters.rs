//! Ordered query pairs append; encoded query strings replace the query.
use super::{Error, Url};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryParameter {
    pub name: String,
    pub value: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Parameters {
    Encoded(String),
    Pairs(Vec<QueryParameter>),
}
impl Parameters {
    pub(super) fn validate(&self) -> Result<(), Error> {
        let valid = match self {
            Self::Encoded(value) => value.len() <= 4096,
            Self::Pairs(fields) => {
                fields.len() <= 128
                    && fields
                        .iter()
                        .all(|field| field.name.len() <= 1024 && field.value.len() <= 4096)
                    && fields
                        .iter()
                        .map(|field| field.name.len() + field.value.len() + 2)
                        .sum::<usize>()
                        <= 4096
            }
        };
        if valid {
            Ok(())
        } else {
            Err("HTTP_QUERY_BOUNDS".into())
        }
    }
    pub(super) fn apply(&self, url: &mut Url) -> Result<(), Error> {
        self.validate()?;
        match self {
            Self::Encoded(value) if !value.is_empty() => {
                let query = value.strip_prefix('?').unwrap_or(value);
                url.set_query((!query.is_empty()).then_some(query));
            }
            Self::Pairs(fields) if !fields.is_empty() => {
                let existing = url.query_pairs().into_owned().collect::<Vec<_>>();
                url.set_query(None);
                let mut query = url.query_pairs_mut();
                query.extend_pairs(existing);
                query.extend_pairs(fields.iter().map(|field| (&field.name, &field.value)));
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn query_pairs_append_and_encoded_strings_replace_without_changing_authority() {
        let initial = Url::parse("https://example.com/acme?first=Acme%20one").unwrap();
        let mut url = initial.clone();
        Parameters::Pairs(vec![
            QueryParameter {
                name: "label".into(),
                value: "two +&".into(),
            },
            QueryParameter {
                name: "label".into(),
                value: "東京".into(),
            },
        ])
        .apply(&mut url)
        .unwrap();
        assert_eq!(
            url.as_str(),
            "https://example.com/acme?first=Acme+one&label=two+%2B%26&label=%E6%9D%B1%E4%BA%AC"
        );
        assert_eq!(url.origin(), initial.origin());
        Parameters::Encoded("?label=Acme%20two".into())
            .apply(&mut url)
            .unwrap();
        assert_eq!(url.query(), Some("label=Acme%20two"));
        let before = url.clone();
        Parameters::Encoded(String::new()).apply(&mut url).unwrap();
        Parameters::Pairs(vec![]).apply(&mut url).unwrap();
        assert_eq!(url, before);
        Parameters::Encoded("?".into()).apply(&mut url).unwrap();
        assert_eq!(url.query(), None);
        assert_eq!(
            Parameters::Encoded("x".repeat(4097))
                .validate()
                .unwrap_err()
                .code,
            "HTTP_QUERY_BOUNDS"
        );
    }
}

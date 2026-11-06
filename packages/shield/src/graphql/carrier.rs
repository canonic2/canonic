//! Closed GitHub CLI API grammar; never reads files, configuration or credentials.
use super::Failure;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Carrier {
    pub(super) query: String,
    pub(super) operation: Option<String>,
    pub(super) variables: BTreeMap<String, Value>,
    pub(super) paginate: bool,
}
#[expect(
    clippy::too_many_lines,
    reason = "closed CLI grammar preserves option arity and field semantics in one bounded pass"
)]
pub(super) fn decode(
    args: &[String],
    environment: Option<&BTreeMap<String, String>>,
) -> Result<Carrier, Failure> {
    let fail = || Failure::new("carrier", "unsupported API arguments");
    if args.first().map(String::as_str) != Some("api") {
        return Err(fail());
    }
    let mut fields = BTreeMap::new();
    let mut singleton = BTreeSet::new();
    let mut endpoint = None;
    let mut hostname = None;
    let (mut paginate, mut slurp, mut ended) = (false, false, false);
    let mut index = 1;
    while index < args.len() {
        let arg = &args[index];
        index += 1;
        if arg == "--" && !ended {
            ended = true;
            continue;
        }
        if ended || !arg.starts_with('-') || arg == "-" {
            if endpoint.replace(arg.as_str()).is_some() {
                return Err(fail());
            }
            continue;
        }
        let options: Vec<(&str, Option<&str>)> = if arg.starts_with("--") {
            let (name, value) = arg
                .split_once('=')
                .map_or((arg.as_str(), None), |(n, v)| (n, Some(v)));
            vec![(name, value)]
        } else {
            let mut options = vec![];
            let bytes = arg.as_bytes();
            let mut offset = 1;
            while offset < bytes.len() {
                let name = match bytes[offset] {
                    b'f' => "-f",
                    b'F' => "-F",
                    b'X' => "-X",
                    b'i' => "-i",
                    _ => return Err(fail()),
                };
                offset += 1;
                let rest = &arg[offset..];
                if name != "-i" || rest.starts_with('=') {
                    options.push((
                        name,
                        (!rest.is_empty()).then(|| rest.strip_prefix('=').unwrap_or(rest)),
                    ));
                    break;
                }
                options.push((name, None));
            }
            options
        };
        for (name, attached) in options {
            let valued = matches!(
                name,
                "-f" | "--raw-field" | "-F" | "--field" | "-X" | "--method" | "--hostname"
            );
            let value = if valued {
                Some(if let Some(value) = attached {
                    value
                } else {
                    let value = args.get(index).ok_or_else(fail)?;
                    index += 1;
                    value.as_str()
                })
            } else {
                attached
            };
            if matches!(name, "-f" | "--raw-field" | "-F" | "--field") {
                let (key, value) = value.and_then(|v| v.split_once('=')).ok_or_else(fail)?;
                if !super::model::name(key) {
                    return Err(fail());
                }
                let typed = matches!(name, "-F" | "--field");
                if typed
                    && (value.starts_with('@')
                        || ["{owner}", "{repo}", "{branch}"]
                            .iter()
                            .any(|p| value.contains(p)))
                {
                    return Err(Failure::new("carrier", "unresolved field input"));
                }
                let value = if typed {
                    if let Ok(number) = value.parse::<i64>() {
                        Value::from(number)
                    } else {
                        match value {
                            "true" => Value::Bool(true),
                            "false" => Value::Bool(false),
                            "null" => Value::Null,
                            _ => Value::String(value.into()),
                        }
                    }
                } else {
                    Value::String(value.into())
                };
                if fields.insert(key.to_owned(), value).is_some() {
                    return Err(Failure::new("carrier", "duplicate field"));
                }
                continue;
            }
            let canonical = match name {
                "-X" | "--method" => "method",
                "--hostname" => "hostname",
                "--paginate" => "paginate",
                "--slurp" => "slurp",
                "--silent" => "silent",
                "-i" | "--include" => "include",
                _ => return Err(fail()),
            };
            if !singleton.insert(canonical) {
                return Err(fail());
            }
            match canonical {
                "method" if value != Some("POST") => return Err(fail()),
                "hostname" => hostname = value,
                "paginate" | "slurp" | "silent" | "include" => {
                    let enabled = match value {
                        None | Some("true") => true,
                        Some("false") => false,
                        _ => return Err(fail()),
                    };
                    if canonical == "paginate" {
                        paginate = enabled;
                    }
                    if canonical == "slurp" {
                        slurp = enabled;
                    }
                }
                _ => {}
            }
        }
    }
    if endpoint != Some("graphql") || slurp && !paginate {
        return Err(fail());
    }
    let host = hostname.or_else(|| {
        environment
            .and_then(|e| e.get("GH_HOST"))
            .map(String::as_str)
    });
    if host.is_some_and(|host| host != "github.com") {
        return Err(Failure::new("carrier", "unsupported host"));
    }
    let query = fields
        .remove("query")
        .and_then(|v| v.as_str().map(str::to_owned))
        .filter(|q| !q.is_empty())
        .ok_or_else(|| Failure::new("carrier", "missing query"))?;
    let operation = fields
        .remove("operationName")
        .map(|v| v.as_str().map(str::to_owned).ok_or_else(fail))
        .transpose()?;
    Ok(Carrier {
        query,
        operation,
        variables: fields,
        paginate,
    })
}

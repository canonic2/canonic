//! Bounded syntax validation; never resolves addresses or performs I/O.
use super::invalid;
use crate::RpcError;
use iri_string::types::UriStr;

pub(super) fn url(value: &str) -> Result<(), RpcError> {
    let uri = UriStr::new(value).map_err(|_| invalid("HTTPS URL"))?;
    let authority = uri
        .authority_str()
        .filter(|s| !s.is_empty() && !s.contains('@'))
        .ok_or_else(|| invalid("URL authority"))?;
    if value.len() > 4096
        || !uri.scheme_str().eq_ignore_ascii_case("https")
        || uri.fragment_str().is_some()
        || authority.ends_with(':')
    {
        return Err(invalid("HTTPS URL"));
    }
    let port = if let Some(address) = authority.strip_prefix('[') {
        let (address, suffix) = address.split_once(']').ok_or_else(|| invalid("URL host"))?;
        if address.parse::<std::net::Ipv6Addr>().is_err() {
            return Err(invalid("URL host"));
        }
        if suffix.is_empty() {
            None
        } else {
            Some(
                suffix
                    .strip_prefix(':')
                    .ok_or_else(|| invalid("URL port"))?,
            )
        }
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if host.is_empty() || host.contains(':') {
            return Err(invalid("URL host"));
        }
        Some(port)
    } else {
        None
    };
    if port.is_some_and(|port| {
        port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()) || port.parse::<u16>().is_err()
    }) {
        return Err(invalid("URL port"));
    }
    Ok(())
}

pub(super) fn validate(format: &str, value: &str) -> Result<(), RpcError> {
    let valid = match format {
        "uri" => UriStr::new(value).is_ok(),
        "email" => email(value),
        "date" => date(value),
        "date-time" => datetime(value),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(invalid("string format"))
    }
}

fn email(value: &str) -> bool {
    let Some((local, domain)) = value.rsplit_once('@') else {
        return false;
    };
    let atom = !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~".contains(&b));
    let quoted = if let Some(inside) = local.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        let mut escaped = false;
        let mut valid = true;
        for byte in inside.bytes() {
            if escaped {
                valid &= (32..=126).contains(&byte);
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else {
                valid &= (32..=126).contains(&byte) && byte != b'"';
            }
        }
        valid && !escaped
    } else {
        false
    };
    let domain_valid = domain
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .is_some_and(|address| {
            address.parse::<std::net::Ipv4Addr>().is_ok()
                || address
                    .strip_prefix("IPv6:")
                    .is_some_and(|address| address.parse::<std::net::Ipv6Addr>().is_ok())
        })
        || domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    !local.is_empty()
        && local.len() <= 64
        && value.len() <= 254
        && value.is_ascii()
        && (atom || quoted)
        && domain_valid
}

fn digits(value: &str, start: usize, end: usize) -> Option<u32> {
    value
        .get(start..end)
        .filter(|s| s.bytes().all(|b| b.is_ascii_digit()))?
        .parse()
        .ok()
}

fn date(value: &str) -> bool {
    if value.len() != 10 || !value.is_ascii() || &value[4..5] != "-" || &value[7..8] != "-" {
        return false;
    }
    let (Some(year), Some(month), Some(day)) = (
        digits(value, 0, 4),
        digits(value, 5, 7),
        digits(value, 8, 10),
    ) else {
        return false;
    };
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        _ => return false,
    };
    day > 0 && day <= days
}

fn datetime(value: &str) -> bool {
    if !value.is_ascii()
        || value.len() < 20
        || !date(&value[..10])
        || !matches!(&value[10..11], "T" | "t")
        || &value[13..14] != ":"
        || &value[16..17] != ":"
    {
        return false;
    }
    if digits(value, 11, 13).is_none_or(|n| n > 23)
        || digits(value, 14, 16).is_none_or(|n| n > 59)
        || digits(value, 17, 19).is_none_or(|n| n > 59)
    {
        return false;
    }
    let tail = &value[19..];
    let offset = if let Some(fraction) = tail.strip_prefix('.') {
        let count = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if count == 0 {
            return false;
        }
        &fraction[count..]
    } else {
        tail
    };
    if matches!(offset, "Z" | "z") {
        return true;
    }
    offset.len() == 6
        && matches!(&offset[..1], "+" | "-")
        && &offset[3..4] == ":"
        && digits(offset, 1, 3).is_some_and(|n| n <= 23)
        && digits(offset, 4, 6).is_some_and(|n| n <= 59)
}

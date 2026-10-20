//! Lexical paths, URLs, names and clock values the export builds with. Pure,
//! except for the clock and the random job and revision identifiers.
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// `path.resolve`: absolute and without `.` or `..`, without touching disk.
pub(super) fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `path.resolve(base, relative)`.
pub(super) fn resolve(base: &Path, relative: &str) -> PathBuf {
    normalize(&base.join(relative))
}

/// Whether `file` is `root` or inside it.
pub(super) fn inside(root: &Path, file: &Path) -> bool {
    normalize(file).starts_with(normalize(root))
}

/// `path.relative(root, file)` with `/` separators, for a file inside `root`.
pub(super) fn relative(root: &Path, file: &Path) -> String {
    let file = normalize(file);
    let root = normalize(root);
    file.strip_prefix(&root).map_or_else(
        |_| file.to_string_lossy().replace('\\', "/"),
        |rest| {
            rest.components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        },
    )
}

pub(super) fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub(super) fn basename(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The lower-case extension with its dot, as `path.extname` gives it.
pub(super) fn extension(path: &Path) -> String {
    let name = basename(path);
    match name.rfind('.') {
        Some(0) | None => String::new(),
        Some(at) => name[at..].to_lowercase(),
    }
}

fn segments(path: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if out.last().is_some_and(|last| *last != "..") {
                    out.pop();
                } else {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// `path.posix.join` of relative parts.
pub(super) fn posix_join(parts: &[&str]) -> String {
    let joined = parts.join("/");
    let out = segments(&joined).join("/");
    if out.is_empty() { ".".to_owned() } else { out }
}

/// `path.posix.dirname` of a relative path.
pub(super) fn posix_dirname(path: &str) -> String {
    match path.trim_end_matches('/').rfind('/') {
        Some(0) => "/".to_owned(),
        Some(at) => path[..at].to_owned(),
        None => ".".to_owned(),
    }
}

pub(super) fn posix_basename(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_owned()
}

/// `path.posix.relative` between two paths relative to the same folder.
pub(super) fn posix_relative(from: &str, to: &str) -> String {
    let from = segments(from);
    let to = segments(to);
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    let mut out: Vec<&str> = vec![".."; from.len() - common];
    out.extend(&to[common..]);
    out.join("/")
}

/// Lower-case words joined by `-`, or `fallback`.
fn dashed(value: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for character in value.to_lowercase().chars() {
        if character.is_ascii_lowercase() || character.is_ascii_digit() {
            if gap && !out.is_empty() {
                out.push('-');
            }
            gap = false;
            out.push(character);
        } else {
            gap = true;
        }
    }
    if out.is_empty() {
        fallback.to_owned()
    } else {
        out
    }
}

/// A source export's names: `canonic` when nothing is left.
pub(super) fn slug(value: Option<&str>) -> String {
    dashed(
        value.filter(|value| !value.is_empty()).unwrap_or("canonic"),
        "canonic",
    )
}

/// A PDF's and an image archive's names: `workbench` when nothing is left.
pub(super) fn stem(value: &str) -> String {
    dashed(value, "workbench")
}

/// The first eight hex digits of a text's SHA-256.
pub(super) fn short_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(text.as_bytes());
    hex(&digest[..4])
}

/// Bytes as lower-case hex digits.
pub(super) fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    out
}

/// `%` and a byte's two upper-case hex digits.
fn percent(out: &mut String, byte: u8) {
    const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
    out.push('%');
    out.push(char::from(DIGITS[usize::from(byte >> 4)]));
    out.push(char::from(DIGITS[usize::from(byte & 15)]));
}

/// Twenty-four random hex digits from the system's random source, for
/// job IDs and capture revisions.
pub(super) fn random_hex() -> String {
    let mut bytes = [0_u8; 12];
    getrandom::fill(&mut bytes).expect("the system's random source is available");
    hex(&bytes)
}

/// Now, as `Date.prototype.toISOString` writes it.
pub(super) fn iso_now() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    iso(now.as_millis())
}

fn iso(millis: u128) -> String {
    let seconds = i64::try_from(millis / 1000).unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    // Howard Hinnant's civil-from-days.
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        millis % 1000
    )
}

const QUERY_SAFE: &[u8] = b"*-._";

/// `URLSearchParams` encoding of one name or value.
pub(super) fn form_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || QUERY_SAFE.contains(&byte) {
            out.push(char::from(byte));
        } else if byte == b' ' {
            out.push('+');
        } else {
            percent(&mut out, byte);
        }
    }
    out
}

/// `encodeURIComponent`.
pub(super) fn component_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(char::from(byte));
        } else {
            percent(&mut out, byte);
        }
    }
    out
}

fn path_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if b" \"<>`#?{}".contains(&byte) || !byte.is_ascii() || byte < 0x20 {
            percent(&mut out, byte);
        } else {
            out.push(char::from(byte));
        }
    }
    out
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte == b'+' {
            out.push(b' ');
        } else if byte == b'%'
            && let Some(value) = text
                .get(at + 1..at + 3)
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(value);
            at += 2;
        } else {
            out.push(byte);
        }
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `new URL(url).searchParams.get(name)`.
pub(super) fn query_param(url: &str, name: &str) -> Option<String> {
    let query = url.split('#').next()?.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        (decode(key) == name).then(|| decode(value))
    })
}

fn origin(base: &str) -> &str {
    let after = base.find("://").map_or(0, |at| at + 3);
    base[after..]
        .find('/')
        .map_or(base, |slash| &base[..after + slash])
}

/// `new URL(input, base).href` for the addresses the export makes.
pub(super) fn resolve_url(input: &str, base: &str) -> String {
    if input.contains("://") {
        return input.to_owned();
    }
    if let Some(rest) = input.strip_prefix("//") {
        let scheme = base.split("://").next().unwrap_or("http");
        return format!("{scheme}://{rest}");
    }
    let (path, query) = input
        .split_once('?')
        .map_or((input, None), |(path, query)| (path, Some(query)));
    let joined = if path.starts_with('/') {
        format!("{}{}", origin(base), path_encode(path))
    } else {
        let directory = base.rfind('/').map_or(base, |at| &base[..=at]);
        format!("{directory}{}", path_encode(path))
    };
    match query {
        Some(query) => format!("{joined}?{query}"),
        None => joined,
    }
}

/// A page address with `params` set, as `searchParams.set` writes them.
pub(super) fn page_url(file: &str, base: &str, params: &[(&str, &str)]) -> String {
    let mut url = resolve_url(file, base);
    for (index, (name, value)) in params.iter().enumerate() {
        url.push(if index == 0 && !url.contains('?') {
            '?'
        } else {
            '&'
        });
        url.push_str(&form_encode(name));
        url.push('=');
        url.push_str(&form_encode(value));
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_lower_case_words_joined_by_dashes() {
        assert_eq!(slug(Some("Acme UI")), "acme-ui");
        assert_eq!(slug(Some("")), "canonic");
        assert_eq!(slug(Some("!!")), "canonic");
        assert_eq!(stem("  Acme / Designs  "), "acme-designs");
        assert_eq!(stem("…"), "workbench");
    }

    #[test]
    fn posix_paths_join_and_relate_like_node() {
        assert_eq!(posix_join(&["", "README.md"]), "README.md");
        assert_eq!(
            posix_join(&["pages", "screenshots", ""]),
            "pages/screenshots"
        );
        assert_eq!(posix_dirname("src/button/README.md"), "src/button");
        assert_eq!(posix_dirname("README.md"), ".");
        assert_eq!(
            posix_relative("src/button", "src/button/screenshots/a.jpg"),
            "screenshots/a.jpg"
        );
        assert_eq!(
            posix_relative("pages", "src/shared/dot.svg"),
            "../src/shared/dot.svg"
        );
        assert_eq!(posix_relative(".", "README.md"), "README.md");
        assert_eq!(posix_relative("a", "a"), "");
    }

    #[test]
    fn addresses_resolve_against_the_workbench() {
        let base = "http://127.0.0.1:3579/";
        assert_eq!(
            page_url(
                "pages/sign in.html",
                base,
                &[("actions", "off"), ("state", "an error")]
            ),
            "http://127.0.0.1:3579/pages/sign%20in.html?actions=off&state=an+error"
        );
        assert_eq!(
            resolve_url("/a.html?x=1", base),
            "http://127.0.0.1:3579/a.html?x=1"
        );
        assert_eq!(
            resolve_url("http://example.com/a", base),
            "http://example.com/a"
        );
        assert_eq!(
            query_param("http://x/a.md?lens=web&b=1", "lens").as_deref(),
            Some("web")
        );
        assert_eq!(query_param("http://x/a.md", "lens"), None);
    }

    #[test]
    fn times_are_written_as_javascript_writes_them() {
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso(1_700_000_000_123), "2023-11-14T22:13:20.123Z");
        assert_eq!(iso(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert_eq!(random_hex().len(), 24);
        assert_ne!(random_hex(), random_hex());
    }
}

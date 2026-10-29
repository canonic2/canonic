//! Recognizes the read-only curl and wget forms that a web fetch and an
//! optional file write describe completely, without executing them.
//! Any other option, a second URL or an unusual URL leaves the command to
//! argv policy.
use std::collections::BTreeMap;

/// The HTTP client a recognized command runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Client {
    Curl,
    Wget,
}

/// One recognized download: the fetch it performs and the file it writes.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Fetch<'a> {
    pub(crate) client: Client,
    pub(crate) url: &'a str,
    pub(crate) method: &'static str,
    /// Whether the client follows redirects: `follow` or `none`.
    pub(crate) redirects: &'static str,
    /// The file written with the response body; stdout and `/dev/null` are none.
    pub(crate) output: Option<&'a str>,
    /// curl's leading `-q`, which skips its configuration file.
    pub(crate) skips_config: bool,
    /// Whether the client looks up `~/.netrc` credentials when a server asks
    /// for them: wget does unless `--no-netrc`; curl only with `-n`.
    pub(crate) reads_netrc: bool,
}

/// The recognized fetch for a literal command, or None when any part of it is
/// outside the read-only subset.
pub(crate) fn fetch<'a>(executable: &str, arguments: &[&'a str]) -> Option<Fetch<'a>> {
    match executable {
        "curl" | "/bin/curl" | "/usr/bin/curl" => curl(arguments),
        "wget" | "/bin/wget" | "/usr/bin/wget" => wget(arguments),
        _ => None,
    }
}

/// Whether reported environment values can change what a client requests:
/// a proxy or a relocated configuration or credentials file.
pub(crate) fn environment_changes_request(environment: &BTreeMap<String, String>) -> bool {
    environment
        .iter()
        .any(|(name, value)| environment_entry_changes_request(name, !value.is_empty()))
}

/// Shared effect predicate for reported environment and native observations.
/// Values are not retained: only whether a proxy value is nonempty matters.
pub(crate) fn environment_entry_changes_request(name: &str, nonempty: bool) -> bool {
    let proxy = ["http_proxy", "https_proxy", "all_proxy", "no_proxy"]
        .contains(&name.to_ascii_lowercase().as_str());
    (proxy && nonempty)
        || [
            "CURL_HOME",
            "XDG_CONFIG_HOME",
            "WGETRC",
            "SYSTEM_WGETRC",
            "NETRC",
        ]
        .contains(&name)
}

fn curl<'a>(arguments: &[&'a str]) -> Option<Fetch<'a>> {
    let mut url = None;
    let mut output = None;
    let mut head = false;
    let mut request = None;
    let mut follow = false;
    let mut skips_config = false;
    let mut options = true;
    let mut position = 0;
    while position < arguments.len() {
        let argument = arguments[position];
        position += 1;
        if !options || argument == "-" || !argument.starts_with('-') {
            set_once(&mut url, argument)?;
        } else if argument == "--" {
            options = false;
        } else if position == 1 && argument == "-q" {
            skips_config = true;
        } else if let Some(name) = argument.strip_prefix("--") {
            // curl long options take their value as the next argument.
            match name {
                // Retrying repeats the same read-only request.
                "silent" | "show-error" | "fail" | "fail-with-body" | "include" | "globoff"
                | "compressed" | "retry-connrefused" | "retry-all-errors" => {}
                "head" => head = true,
                "location" => follow = true,
                _ => {
                    let value = *arguments.get(position)?;
                    position += 1;
                    curl_value(name, value, &mut url, &mut output, &mut request)?;
                }
            }
        } else {
            let flags = &argument[1..];
            for (index, flag) in flags.char_indices() {
                match flag {
                    's' | 'S' | 'f' | 'i' | 'g' => {}
                    'I' => head = true,
                    'L' => follow = true,
                    'm' | 'X' | 'w' | 'o' => {
                        let attached = &flags[index + 1..];
                        let value = if attached.is_empty() {
                            position += 1;
                            *arguments.get(position - 1)?
                        } else {
                            attached
                        };
                        let name = match flag {
                            'm' => "max-time",
                            'X' => "request",
                            'w' => "write-out",
                            _ => "output",
                        };
                        curl_value(name, value, &mut url, &mut output, &mut request)?;
                        break;
                    }
                    _ => return None,
                }
            }
        }
    }
    let method = match (head, request) {
        (true, Some("GET")) => return None,
        (true, _) | (false, Some("HEAD")) => "HEAD",
        (false, _) => "GET",
    };
    Some(Fetch {
        client: Client::Curl,
        url: literal_url(url?)?,
        method,
        redirects: if follow { "follow" } else { "none" },
        output: output.filter(|path| !["-", "/dev/null"].contains(path)),
        skips_config,
        reads_netrc: false,
    })
}

fn curl_value<'a>(
    name: &str,
    value: &'a str,
    url: &mut Option<&'a str>,
    output: &mut Option<&'a str>,
    request: &mut Option<&'a str>,
) -> Option<()> {
    match name {
        "max-time" | "connect-timeout" => positive(value).then_some(()),
        "retry" | "retry-delay" | "retry-max-time" => {
            (!value.is_empty() && value.bytes().all(|b| b.is_ascii_digit())).then_some(())
        }
        "request" if ["GET", "HEAD"].contains(&value) => set_once(request, value),
        "write-out" if inspection_write_out(value) => Some(()),
        "output" if !value.is_empty() => set_once(output, value),
        "url" => set_once(url, value),
        _ => None,
    }
}

fn wget<'a>(arguments: &[&'a str]) -> Option<Fetch<'a>> {
    let mut url = None;
    let mut output = None;
    let mut spider = false;
    let mut follow = true;
    let mut no_hsts = false;
    let mut no_netrc = false;
    let mut options = true;
    let mut position = 0;
    while position < arguments.len() {
        let argument = arguments[position];
        position += 1;
        if !options || argument == "-" || !argument.starts_with('-') {
            set_once(&mut url, argument)?;
        } else if argument == "--" {
            options = false;
        } else if argument == "-nv" {
        } else if let Some(option) = argument.strip_prefix("--") {
            let (name, attached) = option
                .split_once('=')
                .map_or((option, None), |(name, value)| (name, Some(value)));
            match (name, attached) {
                ("quiet" | "no-verbose" | "server-response", None) => {}
                ("spider", None) => spider = true,
                ("no-hsts", None) => no_hsts = true,
                ("no-netrc", None) => no_netrc = true,
                _ => {
                    let value = if let Some(value) = attached {
                        value
                    } else {
                        position += 1;
                        *arguments.get(position - 1)?
                    };
                    wget_value(name, value, &mut output, &mut follow)?;
                }
            }
        } else {
            let flags = &argument[1..];
            for (index, flag) in flags.char_indices() {
                match flag {
                    'q' | 'S' => {}
                    'O' | 'T' => {
                        let attached = &flags[index + 1..];
                        let value = if attached.is_empty() {
                            position += 1;
                            *arguments.get(position - 1)?
                        } else {
                            attached
                        };
                        let name = if flag == 'O' {
                            "output-document"
                        } else {
                            "timeout"
                        };
                        wget_value(name, value, &mut output, &mut follow)?;
                        break;
                    }
                    _ => return None,
                }
            }
        }
    }
    // -O opens/truncates its output at startup, even with --spider. Keep the
    // combination outside this subset instead of discarding that write.
    if spider && output.is_some() {
        return None;
    }
    // Without -O, wget names its file from the URL and the files already in cwd.
    // Without --no-hsts, it reads and rewrites ~/.wget-hsts outside file policy.
    if (output.is_none() && !spider) || !no_hsts {
        return None;
    }
    Some(Fetch {
        client: Client::Wget,
        url: literal_url(url?)?,
        method: "GET",
        redirects: if follow { "follow" } else { "none" },
        output: output.filter(|path| !["-", "/dev/null"].contains(path)),
        skips_config: false,
        reads_netrc: !no_netrc,
    })
}

fn wget_value<'a>(
    name: &str,
    value: &'a str,
    output: &mut Option<&'a str>,
    follow: &mut bool,
) -> Option<()> {
    match name {
        "output-document" if !value.is_empty() => set_once(output, value),
        "timeout" => positive(value).then_some(()),
        "tries" => (positive(value) && value.bytes().all(|b| b.is_ascii_digit())).then_some(()),
        "max-redirect" if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => {
            *follow = value.bytes().any(|b| b != b'0');
            Some(())
        }
        _ => None,
    }
}

fn set_once<'a>(slot: &mut Option<&'a str>, value: &'a str) -> Option<()> {
    slot.replace(value).is_none().then_some(())
}

/// A positive decimal number of seconds or attempts.
fn positive(value: &str) -> bool {
    let mut parts = value.splitn(2, '.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next();
    !whole.is_empty()
        && whole.bytes().all(|b| b.is_ascii_digit())
        && fraction.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()))
        && value.bytes().any(|b| b.is_ascii_digit() && b != b'0')
}

/// curl write-out restricted to response inspection: `@file` reads a file and
/// directives such as `%output{}` write one.
fn inspection_write_out(value: &str) -> bool {
    const VARIABLES: [&str; 9] = [
        "http_code",
        "response_code",
        "size_download",
        "time_total",
        "content_type",
        "url_effective",
        "remote_ip",
        "remote_port",
        "num_connects",
    ];
    if value.starts_with('@') {
        return false;
    }
    let mut rest = value;
    while let Some(index) = rest.find('%') {
        rest = &rest[index + 1..];
        if let Some(after) = rest.strip_prefix('%') {
            rest = after;
            continue;
        }
        let Some((name, after)) = rest.strip_prefix('{').and_then(|r| r.split_once('}')) else {
            return false;
        };
        if !VARIABLES.contains(&name) {
            return false;
        }
        rest = after;
    }
    true
}

/// A literal absolute HTTP(S) URL with a host and no user information,
/// whitespace, backslashes or curl globbing.
fn literal_url(value: &str) -> Option<&str> {
    if !(value.starts_with("http://") || value.starts_with("https://"))
        || value
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "\\{}[]".contains(c))
    {
        return None;
    }
    let url = url::Url::parse(value).ok()?;
    (url.host_str().is_some_and(|host| !host.is_empty())
        && url.username().is_empty()
        && url.password().is_none())
    .then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recognized(source: &str) -> Option<Fetch<'_>> {
        let words: Vec<&str> = source.split(' ').collect();
        fetch(words[0], &words[1..])
    }

    const URL: &str = "https://registry.example.com/catalog.json";

    #[test]
    fn curl_downloads_are_a_fetch_and_an_output_write() {
        let fetch = recognized(
            "curl --fail --silent --show-error --output /tmp/catalog.json https://registry.example.com/catalog.json",
        )
        .unwrap();
        assert_eq!(
            fetch,
            Fetch {
                client: Client::Curl,
                url: URL,
                method: "GET",
                redirects: "none",
                output: Some("/tmp/catalog.json"),
                skips_config: false,
                reads_netrc: false,
            }
        );
        let bundled =
            recognized("curl -fsSLo catalog.json https://registry.example.com/catalog.json")
                .unwrap();
        assert_eq!(
            (bundled.redirects, bundled.output),
            ("follow", Some("catalog.json"))
        );
        let head =
            recognized("/usr/bin/curl -q -sI https://registry.example.com/catalog.json").unwrap();
        assert_eq!(
            (head.method, head.output, head.skips_config),
            ("HEAD", None, true)
        );
        for source in [
            "curl https://registry.example.com/catalog.json",
            "curl -s -o - https://registry.example.com/catalog.json",
            "curl -s -o /dev/null -w %{http_code} https://registry.example.com/catalog.json",
            "curl -m 2.5 --connect-timeout 3 -X GET --url https://registry.example.com/catalog.json",
            "curl -s -- https://registry.example.com/catalog.json",
            "curl -s --retry 10 --retry-connrefused --retry-delay 1 -w \\nHTTP%{http_code}\\n https://registry.example.com/catalog.json",
            "curl --retry 0 --retry-max-time 30 --retry-all-errors https://registry.example.com/catalog.json",
        ] {
            assert_eq!(recognized(source).map(|f| f.output), Some(None), "{source}");
        }
    }

    #[test]
    fn curl_forms_outside_the_read_only_subset_are_not_recognized() {
        for source in [
            "curl -d @notes.txt https://registry.example.com/",
            "curl --data x https://registry.example.com/",
            "curl -F file=@notes.txt https://registry.example.com/",
            "curl -T notes.txt https://registry.example.com/",
            "curl -H Authorization:x https://registry.example.com/",
            "curl -u user:secret https://registry.example.com/",
            "curl -n https://registry.example.com/",
            "curl -K acme.conf https://registry.example.com/",
            "curl -x http://proxy.example.com https://registry.example.com/",
            "curl --resolve registry.example.com:443:192.0.2.1 https://registry.example.com/",
            "curl -O https://registry.example.com/catalog.json",
            "curl -X POST https://registry.example.com/",
            "curl -I -X GET https://registry.example.com/",
            "curl -w @format.txt https://registry.example.com/",
            "curl -w %output{acme.txt} https://registry.example.com/",
            "curl -m 0 https://registry.example.com/",
            "curl --retry many https://registry.example.com/",
            "curl --retry-delay 1.5 https://registry.example.com/",
            "curl --output=catalog.json https://registry.example.com/",
            "curl -o a.json -o b.json https://registry.example.com/",
            "curl https://registry.example.com/a https://registry.example.com/b",
            "curl https://registry.example.com/{a,b}",
            "curl https://user@registry.example.com/",
            "curl http:registry.example.com/",
            "curl ftp://registry.example.com/",
            "curl -s -q https://registry.example.com/",
            "curl -s",
            "/opt/acme/bin/curl https://registry.example.com/",
        ] {
            assert_eq!(recognized(source), None, "{source}");
        }
    }

    #[test]
    fn wget_downloads_need_a_named_output_or_spider() {
        let fetch = recognized(
            "wget --no-hsts -q -O /tmp/catalog.json https://registry.example.com/catalog.json",
        )
        .unwrap();
        assert_eq!(
            (
                fetch.client,
                fetch.method,
                fetch.redirects,
                fetch.output,
                fetch.reads_netrc
            ),
            (
                Client::Wget,
                "GET",
                "follow",
                Some("/tmp/catalog.json"),
                true
            )
        );
        let pinned = recognized("wget --no-hsts --no-netrc --max-redirect=0 -qO- https://registry.example.com/catalog.json").unwrap();
        assert_eq!(
            (pinned.redirects, pinned.output, pinned.reads_netrc),
            ("none", None, false)
        );
        let spider = recognized(
            "/usr/bin/wget --no-hsts -nv --spider --tries 2 -T 5 https://registry.example.com/",
        )
        .unwrap();
        assert_eq!(spider.output, None);
        for options in [
            "-O /dev/null",
            "-qO/dev/null",
            "--output-document=/dev/null",
        ] {
            let source = format!("wget --no-hsts --no-netrc {options} {URL}");
            assert_eq!(recognized(&source).map(|fetch| fetch.output), Some(None));
        }
        for source in [
            // Without --no-hsts, wget reads and rewrites ~/.wget-hsts.
            "wget -qO- https://registry.example.com/catalog.json",
            "wget --no-netrc -q -O catalog.json https://registry.example.com/catalog.json",
            "wget --no-hsts https://registry.example.com/catalog.json",
            "wget --no-hsts -r -O site.html https://registry.example.com/",
            "wget --no-hsts --post-data=x -O - https://registry.example.com/",
            "wget --no-hsts --header=X:y -O - https://registry.example.com/",
            "wget --no-hsts --user=acme -O - https://registry.example.com/",
            "wget --no-hsts --hsts-file=hsts -O - https://registry.example.com/",
            "wget --no-hsts -e robots=off -O - https://registry.example.com/",
            "wget --no-hsts -i urls.txt -O -",
            "wget --no-hsts -P downloads -O - https://registry.example.com/",
            "wget --no-hsts --tries 1.5 -O - https://registry.example.com/",
            "wget --no-hsts -O - https://registry.example.com/a https://registry.example.com/b",
        ] {
            assert_eq!(recognized(source), None, "{source}");
        }
    }

    #[test]
    fn wget_spider_with_any_output_option_is_not_a_modeled_fetch() {
        for options in [
            "--spider -O out.txt",
            "-O out.txt --spider",
            "--spider -qOout.txt",
            "--output-document=out.txt --spider",
            "--spider --output-document out.txt",
            "--spider -O -",
        ] {
            let source = format!("wget --no-hsts --no-netrc {options} {URL}");
            assert_eq!(recognized(&source), None, "{source}");
        }
    }

    #[test]
    fn reported_proxies_and_configuration_locations_change_the_request() {
        let report =
            |name: &str, value: &str| BTreeMap::from([(name.to_owned(), value.to_owned())]);
        for (name, value) in [
            ("https_proxy", "http://proxy.example.com"),
            ("HTTPS_PROXY", "http://proxy.example.com"),
            ("ALL_PROXY", "socks5://proxy.example.com"),
            ("CURL_HOME", "/srv/acme"),
            ("WGETRC", "/srv/acme/wgetrc"),
            ("NETRC", "/srv/acme/netrc"),
        ] {
            assert!(environment_changes_request(&report(name, value)), "{name}");
        }
        assert!(!environment_changes_request(&report("https_proxy", "")));
        assert!(!environment_changes_request(&report("HOME", "/home/acme")));
    }
}

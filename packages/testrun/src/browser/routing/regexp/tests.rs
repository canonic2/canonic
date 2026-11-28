use super::*;
use crate::browser::routing::Matcher;

fn matches(pattern: &str, flags: &str, url: &str) -> bool {
    Matcher::Regex {
        pattern: pattern.into(),
        flags: flags.into(),
    }
    .compile()
    .unwrap_or_else(|code| panic!("{pattern}/{flags}: {code}"))
    .is_match(url)
}

#[test]
fn regular_syntax_empty_languages_match_javascript() {
    let url = "https://example.com/Acme/one";
    for flags in ["", "u", "gy", "di"] {
        assert!(matches("", flags, url));
        assert!(!matches("[]", flags, url));
        assert!(!matches("[]+|missing", flags, url));
        assert!(matches("[]*Acme", flags.trim_matches(['g', 'y']), url));
    }
}

#[test]
fn regular_syntax_unambiguous_legacy_octal_matches_javascript() {
    let url = "https://example.com/Acme/one";
    for pattern in [r"\057Acme", r"[\101]cme", r"[\41-\132]cme", r"[\8\9A]cme"] {
        assert!(matches(pattern, "", url), "{pattern}");
    }
    for pattern in [r"\012", r"\08", r"[\400]", r"[\777]"] {
        assert_eq!(translate(pattern, "u"), Err(INVALID), "{pattern}");
    }
    for pattern in [r"(A)\1", r"\1(A)", r"(?<n>A)\k<n>"] {
        assert_eq!(translate(pattern, ""), Err(UNSUPPORTED), "{pattern}");
    }
}

#[test]
fn regular_syntax_capture_aware_legacy_numeric_escapes_match_javascript() {
    for (pattern, path, expected) in [
        (r"\101cme", "/Acme", true),
        (r"\141cme", "/acme", true),
        (r"\57Acme", "/Acme", true),
        (r"\89", "/89", true),
        (r"(A)\8", "/A8", true),
        (r"(A)\80", "/A80", true),
        (r"(A)\101", "/AA", true),
        (r"\101(A)", "/AA", true),
        (r"\10189", "/A89", true),
        (r"\777", "/Acme?7", true),
        (r"\400", "/0", false),
        (r"\1", "/Acme", false),
        (r"[(]|\1", "/Acme", false),
        (r"\(\1", "/Acme", false),
        (r"(?:A)\1", "/A", false),
        (r"(A)\11", "/AA", false),
    ] {
        assert_eq!(
            matches(pattern, "", &format!("https://example.com{path}")),
            expected,
            "{pattern}"
        );
    }
    for pattern in [r"(A)\1", r"\1(A)", r"(?:A)\1(A)", r"(?<n>A)\1"] {
        assert_eq!(translate(pattern, ""), Err(UNSUPPORTED), "{pattern}");
    }
    let captures = "()".repeat(101);
    assert_eq!(translate(&format!(r"{captures}\101"), ""), Err(UNSUPPORTED));
    for pattern in [r"\1", r"(A)\8", r"(A)\80", r"\101"] {
        assert_eq!(translate(pattern, "u"), Err(INVALID), "{pattern}");
    }
    assert!(translate(&format!("\\{}", "8".repeat(2000)), "").is_ok());
    // Retain the current non-ASCII matching restriction and compiled budgets.
    assert_eq!(translate(r"\377", ""), Err(UNSUPPORTED));
}

#[test]
fn regular_syntax_legacy_k_identity_respects_complete_named_capture_layout() {
    for (pattern, path) in [
        (r"\k", "/k"),
        (r"[\k]", "/k"),
        (r"(A)\k", "/Ak"),
        (r"(?:A)[\k]", "/Ak"),
        (r"[(?<n>)]\k", "/)k"),
        (r"\(\?<n>\)\k", "/(?<n>)k"),
    ] {
        assert!(
            matches(pattern, "", &format!("https://example.com{path}")),
            "{pattern}"
        );
    }
    for pattern in [r"(?<n>A)\k<n>", r"\k<n>(?<n>A)"] {
        assert_eq!(translate(pattern, ""), Err(UNSUPPORTED), "{pattern}");
    }
    for (pattern, flags) in [
        (r"\k", "u"),
        (r"[\k]", "u"),
        (r"(?<n>A)\k", ""),
        (r"(?<n>A)\k<missing>", ""),
        (r"(?<n>A)[\k]", ""),
        (r"[\k](?<n>A)", ""),
    ] {
        assert_eq!(translate(pattern, flags), Err(INVALID), "{pattern}/{flags}");
    }
}

#[test]
fn javascript_regular_syntax_matches_like_regexp() {
    let url = "https://example.com/acme/v2/items?id=42&q=a{b}";
    for (pattern, flags, expected) in [
        // Named groups and pattern modifiers.
        (r"/(?<version>v\d)/(?<kind>items)", "", true),
        (r"(?i:ACME)/v2", "", true),
        (r"(?i:ACME)/V2", "", false),
        (r"(?-i:ACME)", "i", false),
        (r"(?m-s:acme)", "", true),
        // Escapes.
        (r"\u{61}cme", "u", true),
        (r"\x61cme", "", true),
        (r"a\{b\}", "u", true),
        (r"\/acme\/", "", true),
        // Annex B: unmatched braces and unknown escapes are literals.
        (r"q=a{b}", "", true),
        (r"\acme", "", true),
        (r"items\?id", "", true),
        (r"\c", "", false),
        // Classes.
        (r"[\w-]+\.com", "", true),
        (r"[\d-x]=", "", false),
        (r"id=[\d-x]", "", true),
        (r"[^]acme", "", true),
        (r"[a-c][a-c]me", "", true),
        (r"[[]", "", false),
        // Quantifiers and anchors.
        (r"^https://(\w+\.)+com/", "", true),
        (r"\d{2}$", "", false),
        (r"id=\d{2}&", "", true),
        (r"v\d{1,}?/", "", true),
        // Flags in any order, and `d` without matching effect.
        ("ACME", "id", true),
        ("ACME", "mi", true),
        ("ACME", "u", false),
        // Pinned `lastIndex = 0`: `g` has no effect; `y` anchors at the URL start.
        ("acme", "g", true),
        ("acme", "gy", false),
        ("https", "y", true),
        ("v2", "my", false),
    ] {
        assert_eq!(matches(pattern, flags, url), expected, "{pattern}/{flags}");
    }
    assert!(matches(r"\x00|\0|[\b]|\cJ|v2", "", url));
    // Rust class set operations and POSIX classes stay JavaScript literals.
    assert!(matches("[a-z&&[^b]]", "", "https://example.com/&]"));
    assert!(!matches("[[:alpha:]]", "", "https://example.com/acme"));
    assert!(matches("[[:alpha:]]", "", "https://example.com/a]"));
    // A four-digit Unicode escape, and its Annex B literal form without digits.
    assert!(matches(&format!("\\{}0061cme", 'u'), "", url));
    assert!(!matches(&format!("\\{}cme", 'u'), "", url));
}

#[test]
fn non_regular_stateful_and_malformed_syntax_refuse_with_distinct_codes() {
    for (pattern, flags) in [
        ("(?=acme)", ""),
        ("(?!acme)", ""),
        ("(?<=acme)", ""),
        ("(?<!acme)", ""),
        (r"(a)\1", ""),
        (r"(?<n>a)\k<n>", ""),
        (r"\u{E9}", "u"),
        (r"\u{1F600}", "u"),
        (r"\p{L}", "u"),
        ("(?<n>a)|(?<n>b)", ""),
        ("acme", "v"),
    ] {
        assert_eq!(
            translate(pattern, flags),
            Err(UNSUPPORTED),
            "{pattern}/{flags}"
        );
    }
    for (pattern, flags) in [
        ("*acme", ""),
        ("a**", ""),
        ("^*", ""),
        (r"\b+", ""),
        ("{1}", ""),
        ("a{2,1}", ""),
        ("(acme", ""),
        ("acme)", ""),
        ("[acme", ""),
        ("[z-a]", ""),
        ("(?<1n>a)", ""),
        ("(?-:a)", ""),
        ("(?i-i:a)", ""),
        ("(?x:a)", ""),
        ("(?P<n>a)", ""),
        ("a{", "u"),
        ("a}", "u"),
        (r"\a", "u"),
        (r"\-", "u"),
        (r"[\d-x]", "u"),
        (r"\c", "u"),
        (r"\x6", "u"),
        ("acme", "ii"),
        ("acme", "q"),
        ("東京", ""),
        ("a\nb", ""),
    ] {
        assert_eq!(translate(pattern, flags), Err(INVALID), "{pattern}/{flags}");
    }
    assert!(translate(&"a".repeat(2049), "").is_err());
    assert_eq!(
        Matcher::Regex {
            pattern: "a{99999999999}".into(),
            flags: String::new(),
        }
        .compile()
        .err(),
        Some("ROUTE_PATTERN_BOUNDS")
    );
}

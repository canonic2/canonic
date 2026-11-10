//! Portable Unix touch footprint. Date syntax and unknown option modes stay partial.

pub(crate) struct Footprint<'a> {
    pub(crate) reference: Option<&'a str>,
    pub(crate) destinations: Vec<&'a str>,
    pub(crate) complete: bool,
}

pub(crate) fn parse<'a>(executable: &str, arguments: &[&'a str]) -> Option<Footprint<'a>> {
    if !matches!(executable, "touch" | "/bin/touch" | "/usr/bin/touch") {
        return None;
    }
    let mut result = Footprint {
        reference: None,
        destinations: Vec::new(),
        complete: cfg!(any(unix, windows)),
    };
    let mut words = arguments.iter();
    let mut options = true;
    let mut terminated = false;
    while let Some(&word) = words.next() {
        if options && word == "--" {
            options = false;
            terminated = true;
        } else if options && word == "-r" {
            let reference = words.next().copied();
            result.complete &= cfg!(unix)
                && result.reference.is_none()
                && reference.is_some_and(|value| !value.is_empty() && value != "-");
            result.reference = reference;
        } else if options && word.starts_with('-') && word != "-" {
            result.complete &=
                !word.starts_with("--") && word[1..].bytes().all(|flag| b"amc".contains(&flag));
        } else {
            result.complete &=
                !word.is_empty() && word != "-" && (terminated || !word.starts_with('-'));
            options = false;
            result.destinations.push(word);
        }
    }
    result.complete &= !result.destinations.is_empty();
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_is_consumed_before_terminators_and_all_destinations_are_retained() {
        let found = parse("touch", &["-am", "-r", "--", "--", "-one", "two"]).unwrap();
        assert_eq!(found.reference, Some("--"));
        assert_eq!(found.destinations, ["-one", "two"]);
        assert_eq!(found.complete, cfg!(unix));
        for words in [
            &["-r"][..],
            &["-r", "a"],
            &["-r", "a", "-r", "b", "out"],
            &["-rref", "out"],
            &["out", "-r", "ref"],
            &["-t", "203001010000", "out"],
            &["-r", "-", "out"],
        ] {
            assert!(!parse("touch", words).unwrap().complete, "{words:?}");
        }
    }
}

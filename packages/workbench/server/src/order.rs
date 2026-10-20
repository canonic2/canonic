//! Text ordered as JavaScript's `localeCompare` orders it in the root locale,
//! for the names the Node pipeline sorts that way (files in a folder). ASCII
//! follows ICU's root collation: punctuation and symbols, then digits, then
//! letters, case-insensitively first, lower case before upper case second.
//! Other characters follow their code points after ASCII letters.
use std::cmp::Ordering;

/// ICU root order of ASCII punctuation and symbols.
const PUNCTUATION: &str = " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";

fn primary(character: char) -> u32 {
    if let Some(at) = PUNCTUATION.find(character) {
        return 1 + u32::try_from(at).unwrap_or(0);
    }
    if character.is_ascii_digit() {
        return 100 + u32::from(character) - u32::from('0');
    }
    if character.is_ascii_alphabetic() {
        return 200 + u32::from(character.to_ascii_lowercase()) - u32::from('a');
    }
    1000 + u32::from(character)
}

/// `left.localeCompare(right)` for the names Workbench sorts.
pub(crate) fn locale_compare(left: &str, right: &str) -> Ordering {
    let primaries = |text: &str| text.chars().map(primary).collect::<Vec<_>>();
    primaries(left)
        .cmp(&primaries(right))
        .then_with(|| {
            // Lower case sorts before upper case at the same position.
            let cases = |text: &str| text.chars().map(char::is_uppercase).collect::<Vec<_>>();
            cases(left).cmp(&cases(right))
        })
        .then_with(|| left.cmp(right))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_sort_as_the_compiler_sorts_them() {
        let mut names = vec![
            "basic.tsx",
            "basic.ts",
            "basic-two.ts",
            "Basic.ts",
            "basic_2.ts",
            "basic2.ts",
        ];
        names.sort_by(|a, b| locale_compare(a, b));
        assert_eq!(
            names,
            [
                "basic_2.ts",
                "basic-two.ts",
                "basic.ts",
                "Basic.ts",
                "basic.tsx",
                "basic2.ts"
            ]
        );
        let mut symbols: Vec<char> = " !\"#$%&()*+,-./:;<=>?@[\\]^_`{|}~0123456789aAbBzZ"
            .chars()
            .collect();
        symbols.sort_by(|a, b| locale_compare(&a.to_string(), &b.to_string()));
        assert_eq!(
            symbols.into_iter().collect::<String>(),
            " _-,;:!?.\"()[]{}@*/\\&#%`^+<=>|~$0123456789aAbBzZ"
        );
    }
}

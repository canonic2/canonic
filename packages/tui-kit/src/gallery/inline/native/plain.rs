//! Bounded plain fixture input; typed numbers have no implicit default or execution authority.

use super::super::{InlineDemo, Region};
use crate::question_form::{Kind, Question};
use anyhow::Result;
use std::io::{self, BufRead, Read, Write};

fn input(prompt: &str) -> Result<Option<String>> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut bytes = Vec::new();
    let count = io::stdin()
        .lock()
        .take(65_537)
        .read_until(b'\n', &mut bytes)?;
    if count == 0 {
        return Ok(None);
    }
    if count > 65_536 {
        anyhow::bail!("plain input exceeds 64 KiB; nothing confirmed");
    }
    let text = String::from_utf8(bytes)?.trim().to_owned();
    Ok((text != "cancel").then_some(text))
}
fn choose(labels: &[(String, bool)]) -> Result<Option<usize>> {
    for (i, (label, enabled)) in labels.iter().enumerate() {
        println!(
            "  {}  {}{}",
            i + 1,
            label,
            if *enabled { "" } else { " · unavailable" }
        );
    }
    loop {
        let Some(text) = input("type a listed number, or cancel · no default: ")? else {
            return Ok(None);
        };
        if let Ok(n) = text.parse::<usize>()
            && n > 0
            && labels.get(n - 1).is_some_and(|(_, enabled)| *enabled)
        {
            return Ok(Some(n - 1));
        }
        println!("type an available number; empty input chooses nothing");
    }
}
pub(super) fn run(demo: &InlineDemo) -> Result<()> {
    let result = match &demo.region {
        Region::Picker(p) => {
            println!("Deploy the fixture image to 3 services?");
            let labels = p
                .choices()
                .iter()
                .map(|c| (format!("{} · {}", c.label, c.detail), c.disabled.is_none()))
                .collect::<Vec<_>>();
            choose(&labels)?.and_then(|i| {
                let c = &p.choices()[i];
                (c.id != "cancel").then(|| c.label.clone())
            })
        }
        Region::Permission(p) => {
            let request = p.request();
            println!("{}\n{}\n{}", request.title, request.summary, request.reason);
            for r in &request.resources {
                println!("{} · {}", r.name, r.detail);
            }
            let labels = request
                .choices
                .iter()
                .map(|c| (format!("{} · {}", c.label, c.scope), c.disabled.is_none()))
                .collect::<Vec<_>>();
            if let Some(i) = choose(&labels)? {
                let c = &request.choices[i];
                if (c.wider || c.destructive)
                    && input(&format!(
                        "confirm scope by typing exactly '{}', or cancel: ",
                        c.scope
                    ))?
                    .as_deref()
                        != Some(c.scope.as_str())
                {
                    None
                } else {
                    Some(format!("{} · {}", c.label, c.scope))
                }
            } else {
                None
            }
        }
        Region::Questions(q) => {
            println!("{}", q.request().title);
            let mut answers = Vec::new();
            for question in &q.request().questions {
                let Some(answer) = question_input(question)? else {
                    println!("question → cancelled · nothing sent");
                    return Ok(());
                };
                answers.push(format!("{}: {answer}", question.label));
            }
            if input("type send to confirm all answers, or cancel: ")?.as_deref() == Some("send") {
                Some(answers.join("; "))
            } else {
                None
            }
        }
        Region::Dock => {
            println!("plain host · retained draft: {}", demo.chat.input());
            println!("interactive Chat requires cursor control · nothing sent");
            return Ok(());
        }
    };
    println!(
        "preview → {}",
        result.map_or_else(
            || "cancelled · nothing executed".into(),
            |s| format!("{s} · fixture only")
        )
    );
    Ok(())
}
fn question_input(q: &Question) -> Result<Option<String>> {
    println!("{}\n{}", q.prompt, q.description);
    if q.kind == Kind::Single && !q.other {
        let labels = q
            .choices
            .iter()
            .map(|c| (c.label.clone(), c.disabled.is_none()))
            .collect::<Vec<_>>();
        return Ok(choose(&labels)?.map(|i| q.choices[i].label.clone()));
    }
    if q.kind != Kind::Text {
        for (i, c) in q.choices.iter().enumerate() {
            println!(
                "  {}  {}{}",
                i + 1,
                c.label,
                if c.disabled.is_none() {
                    ""
                } else {
                    " · unavailable"
                }
            );
        }
    }
    loop {
        let prompt = if q.kind == Kind::Text {
            "type answer, or cancel: "
        } else {
            "type numbers separated by spaces, or other: text, or cancel: "
        };
        let Some(text) = input(prompt)? else {
            return Ok(None);
        };
        let valid = if q.kind == Kind::Text {
            (!q.required || !text.is_empty())
                && q.characters.is_none_or(|n| text.chars().count() <= n)
        } else if let Some(other) = text.strip_prefix("other: ") {
            q.other && !other.is_empty() && q.characters.is_none_or(|n| other.chars().count() <= n)
        } else {
            let numbers = text
                .split_whitespace()
                .map(str::parse::<usize>)
                .collect::<std::result::Result<Vec<_>, _>>();
            numbers.is_ok_and(|ns| {
                let unique = ns.iter().collect::<std::collections::BTreeSet<_>>();
                unique.len() == ns.len()
                    && (!q.required || !ns.is_empty())
                    && ns.len() >= q.minimum
                    && q.maximum.is_none_or(|n| ns.len() <= n)
                    && (q.kind != Kind::Single || ns.len() == 1)
                    && ns.iter().all(|n| {
                        *n > 0 && q.choices.get(n - 1).is_some_and(|c| c.disabled.is_none())
                    })
            })
        };
        if valid {
            return Ok(Some(text));
        }
        println!("answer does not meet the supplied choices or limits; nothing sent");
    }
}

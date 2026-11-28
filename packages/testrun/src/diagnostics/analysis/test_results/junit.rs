//! Selected JUnit XML dialect. Unknown result extensions refuse, never imply pass.
use super::*;
use quick_xml::{Reader, events::Event};
use std::collections::BTreeMap;
struct CaseData {
    name: String,
    class: String,
    time: Option<f64>,
    outcome: String,
    message: String,
}
pub(super) fn decode(bytes: &[u8], index: &mut Index) -> Result<()> {
    let input = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let mut reader = Reader::from_str(input);
    let mut state = Parser::default();
    loop {
        let event = reader.read_event().map_err(|_| invalid())?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                state.start(e, empty, index)?;
            }
            Event::End(e) => {
                if state.stack.last().map(String::as_str) != Some(e.name().as_ref()) {
                    return Err(invalid());
                }
                finish(&mut state.stack, &mut state.case, &mut state.cases, index)?;
            }
            Event::Text(t) => {
                let text = t.xml10_content();
                if matches!(
                    state.stack.last().map(String::as_str),
                    Some("failure" | "error" | "skipped")
                ) {
                    let c = state.case.as_mut().ok_or_else(invalid)?;
                    if c.message.len() + text.len() > 8192 {
                        return Err(invalid());
                    }
                    c.message.push_str(&text);
                } else if state.stack.is_empty() && !text.trim().is_empty() {
                    return Err(invalid());
                }
            }
            Event::CData(t) => {
                if matches!(
                    state.stack.last().map(String::as_str),
                    Some("failure" | "error" | "skipped")
                ) {
                    let c = state.case.as_mut().ok_or_else(invalid)?;
                    let t = t.xml10_content();
                    if c.message.len() + t.len() > 8192 {
                        return Err(invalid());
                    }
                    c.message.push_str(&t);
                }
            }
            Event::DocType(_) => return Err(invalid()),
            Event::GeneralRef(r) => {
                if state.stack.is_empty() {
                    return Err(invalid());
                }
                let value = match r.as_ref() {
                    "amp" => '&',
                    "lt" => '<',
                    "gt" => '>',
                    "quot" => '"',
                    "apos" => '\'',
                    _ => r
                        .resolve_char_ref()
                        .map_err(|_| invalid())?
                        .ok_or_else(invalid)?,
                };
                if value.is_control() && !matches!(value, '\n' | '\r' | '\t') {
                    return Err(invalid());
                }
                if matches!(
                    state.stack.last().map(String::as_str),
                    Some("failure" | "error" | "skipped")
                ) {
                    let c = state.case.as_mut().ok_or_else(invalid)?;
                    if c.message.len() + value.len_utf8() > 8192 {
                        return Err(invalid());
                    }
                    c.message.push(value);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !state.stack.is_empty() || state.roots != 1 || state.case.is_some() {
        return Err(invalid());
    }
    index.unsupported.push("JUnit package/build/device/attempt/capture identity and suite-level completeness are unknown; system output and properties remain only in raw evidence; normalize explicit identities/attachments with test-results-v1".into());
    Ok(())
}
fn finish(
    stack: &mut Vec<String>,
    case: &mut Option<CaseData>,
    cases: &mut usize,
    index: &mut Index,
) -> Result<()> {
    let name = stack.pop().ok_or_else(invalid)?;
    if name == "testcase" {
        *cases += 1;
        if *cases > 1024 {
            return Err(invalid());
        }
        let c = case.take().ok_or_else(invalid)?;
        let id = format!("junit-case-{}", *cases);
        let mut row = super::super::decode::row(
            if matches!(c.outcome.as_str(), "assertion_failed" | "unavailable") {
                "failure"
            } else {
                "event"
            },
            &c.name,
            format!("/testcase/{}", *cases - 1),
            json!({"test":{"id":id,"attempt":null,"name":c.name,"className":c.class,"outcome":c.outcome,"message":c.message,"durationMs":c.time.map(|n|n*1000.0),"junitErrorUnclassified":c.outcome=="unavailable"},"runner":"junit_producer_unknown","identityProvenance":"unattributed_junit_document","attachmentsVerified":false}),
        );
        row.correlations.push(id);
        super::super::decode::push(index, row)?;
    }
    Ok(())
}

#[derive(Default)]
struct Parser {
    stack: Vec<String>,
    roots: usize,
    nodes: usize,
    case: Option<CaseData>,
    cases: usize,
}
impl Parser {
    fn validate_element(&mut self, name: &str) -> Result<()> {
        if !matches!(
            name,
            "testsuites"
                | "testsuite"
                | "testcase"
                | "failure"
                | "error"
                | "skipped"
                | "system-out"
                | "system-err"
                | "properties"
                | "property"
        ) {
            return Err(invalid());
        }
        if self.stack.is_empty() {
            self.roots += 1;
            if self.roots != 1 || !matches!(name, "testsuites" | "testsuite") {
                return Err(invalid());
            }
        } else {
            let valid = match self.stack.last().map(String::as_str) {
                Some("testsuites") => matches!(
                    name,
                    "testsuite" | "properties" | "system-out" | "system-err"
                ),
                Some("testsuite") => matches!(
                    name,
                    "testsuite" | "testcase" | "properties" | "system-out" | "system-err"
                ),
                Some("testcase") => matches!(
                    name,
                    "failure" | "error" | "skipped" | "properties" | "system-out" | "system-err"
                ),
                Some("properties") => name == "property",
                _ => false,
            };
            if !valid {
                return Err(invalid());
            }
        }

        Ok(())
    }
    fn start(
        &mut self,
        e: quick_xml::events::BytesStart<'_>,
        empty: bool,
        index: &mut Index,
    ) -> Result<()> {
        self.nodes += 1;
        if self.nodes > 20000 || self.stack.len() >= 32 {
            return Err(invalid());
        }
        let name = e.name().as_ref().to_owned();
        self.validate_element(&name)?;
        let mut attrs = BTreeMap::new();
        for a in e.attributes() {
            let a = a.map_err(|_| invalid())?;
            let key = a.key.as_ref().to_owned();
            let value = a
                .normalized_value(quick_xml::XmlVersion::Explicit1_0)
                .map_err(|_| invalid())?
                .into_owned();
            if value.len() > 8192 || attrs.insert(key, value).is_some() {
                return Err(invalid());
            }
        }
        if name == "testcase" {
            if self.case.is_some() || self.stack.last().map(String::as_str) != Some("testsuite") {
                return Err(invalid());
            }
            let name = attrs.get("name").ok_or_else(invalid)?.clone();
            label(&name)?;
            let class = attrs.get("classname").cloned().unwrap_or_default();
            if class.len() > 256 {
                return Err(invalid());
            }
            let time = attrs
                .get("time")
                .map(|s| s.parse::<f64>().map_err(|_| invalid()))
                .transpose()?;
            if time.is_some_and(|n| !n.is_finite() || !(0.0..=86400.0).contains(&n)) {
                return Err(invalid());
            }
            self.case = Some(CaseData {
                name,
                class,
                time,
                outcome: "passed".into(),
                message: String::new(),
            });
        } else if matches!(name.as_str(), "failure" | "error" | "skipped") {
            let c = self.case.as_mut().ok_or_else(invalid)?;
            if c.outcome != "passed" || self.stack.last().map(String::as_str) != Some("testcase") {
                return Err(invalid());
            }
            c.outcome = match name.as_str() {
                "failure" => "assertion_failed",
                "error" => "unavailable",
                _ => "skipped",
            }
            .into();
            c.message = attrs.get("message").cloned().unwrap_or_default();
        }
        self.stack.push(name.clone());
        if empty {
            finish(&mut self.stack, &mut self.case, &mut self.cases, index)?;
        }
        Ok(())
    }
}

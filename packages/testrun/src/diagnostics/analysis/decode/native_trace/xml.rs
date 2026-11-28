//! Bounded xctrace XML/reference reader. Unknown schemas stay raw and unavailable.
use super::*;
use quick_xml::{Reader, events::Event};
mod activity_monitor;
pub(crate) use activity_monitor::activity_monitor_export;
pub(super) use activity_monitor::decode_activity_monitor;
struct Node {
    name: String,
    attrs: BTreeMap<String, String>,
    text: String,
    children: Vec<Node>,
}
fn parse(input: &str) -> Result<Node> {
    parse_bounded(input, 40000)
}
fn parse_bounded(input: &str, max_nodes: usize) -> Result<Node> {
    let mut reader = Reader::from_str(input);
    let mut stack = vec![Node {
        name: "root".into(),
        attrs: BTreeMap::new(),
        text: String::new(),
        children: vec![],
    }];
    let mut nodes = 0;
    loop {
        let event = reader.read_event().map_err(|_| invalid())?;
        let empty = matches!(&event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                let name = e.name().as_ref().to_owned();
                let mut attrs = BTreeMap::new();
                for a in e.attributes() {
                    let a = a.map_err(|_| invalid())?;
                    attrs.insert(
                        a.key.as_ref().to_owned(),
                        a.normalized_value(quick_xml::XmlVersion::Explicit1_0)
                            .map_err(|_| invalid())?
                            .into_owned(),
                    );
                }
                let node = Node {
                    name,
                    attrs,
                    text: String::new(),
                    children: vec![],
                };
                nodes += 1;
                if nodes > max_nodes || stack.len() > 32 {
                    return Err(invalid());
                }
                if empty {
                    stack.last_mut().unwrap().children.push(node);
                } else {
                    stack.push(node);
                }
            }
            Event::End(_) => {
                if stack.len() < 2 {
                    return Err(invalid());
                }
                let node = stack.pop().unwrap();
                stack.last_mut().unwrap().children.push(node);
            }
            Event::Text(t) => stack.last_mut().unwrap().text.push_str(&t.xml10_content()),
            Event::DocType(_) => return Err(invalid()),
            Event::GeneralRef(r) => {
                let name = r.as_ref();
                let value = match name {
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
                stack.last_mut().unwrap().text.push(value);
            }
            Event::Eof => break,
            Event::CData(t) => stack.last_mut().unwrap().text.push_str(&t.xml10_content()),
            _ => {}
        }
    }
    if stack.len() != 1 {
        return Err(invalid());
    }
    Ok(stack.pop().unwrap())
}
fn collect<'a>(n: &'a Node, tag: &str, out: &mut Vec<&'a Node>) {
    if n.name == tag {
        out.push(n);
    }
    for c in &n.children {
        collect(c, tag, out);
    }
}
type References<'a> = BTreeMap<(&'a str, &'a str), &'a Node>;

fn ids<'a>(n: &'a Node, map: &mut References<'a>) -> Result<()> {
    if let Some(id) = n.attrs.get("id")
        && map.insert((&n.name, id), n).is_some()
    {
        return Err(invalid());
    }
    for c in &n.children {
        ids(c, map)?;
    }
    Ok(())
}
fn reference_target<'a>(node: &Node, map: &References<'a>) -> Result<Option<&'a Node>> {
    node.attrs
        .get("ref")
        .map(|id| {
            map.get(&(node.name.as_str(), id.as_str()))
                .copied()
                .ok_or_else(invalid)
        })
        .transpose()
}
fn expand(n: &Node, map: &References<'_>, depth: usize, budget: &mut usize) -> Result<Node> {
    if depth > 32 || *budget == 0 {
        return Err(invalid());
    }
    *budget -= 1;
    if let Some(target) = reference_target(n, map)? {
        return expand(target, map, depth + 1, budget);
    }
    Ok(Node {
        name: n.name.clone(),
        attrs: n.attrs.clone(),
        text: n.text.clone(),
        children: n
            .children
            .iter()
            .map(|child| expand(child, map, depth + 1, budget))
            .collect::<Result<_>>()?,
    })
}
fn first<'a>(n: &'a Node, tag: &str) -> Option<&'a Node> {
    if n.name == tag {
        return Some(n);
    }
    n.children.iter().find_map(|c| first(c, tag))
}
fn child<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
    node.children.iter().find(|child| child.name == name)
}

pub(crate) fn recording_summary(bytes: &[u8]) -> Result<Option<Value>> {
    let root = parse(std::str::from_utf8(bytes).map_err(|_| invalid())?)?;
    let summary = child(&root, "trace-toc")
        .and_then(|toc| {
            toc.children
                .iter()
                .find(|run| run.name == "run" && run.attrs.get("number").is_some_and(|n| n == "1"))
        })
        .and_then(|run| child(run, "info"))
        .and_then(|info| child(info, "summary"));
    Ok(summary.map(|summary| {
        let end_reason = child(summary, "end-reason")
            .map(|reason| reason.text.trim())
            .filter(|reason| !reason.is_empty() && reason.len() <= 256);
        let duration_seconds = child(summary, "duration")
            .and_then(|duration| duration.text.trim().parse::<f64>().ok())
            .filter(|value| value.is_finite() && *value >= 0.0);
        json!({"endReason":end_reason,"durationSeconds":duration_seconds,
            "deviceDisconnected":end_reason.is_some_and(|reason| reason.eq_ignore_ascii_case("Device disconnected"))})
    }))
}
fn value(n: &Node, tags: &[&str]) -> Option<f64> {
    tags.iter().find_map(|tag| {
        first(n, tag)
            .and_then(|n| n.text.trim().parse::<f64>().ok())
            .filter(|n| n.is_finite())
    })
}
fn json_node(n: &Node) -> Value {
    json!({"tag":n.name,"attributes":n.attrs,"text":n.text,"children":n.children.iter().map(json_node).collect::<Vec<_>>()})
}
pub(super) fn memory_tables(bytes: &[u8]) -> Result<Vec<String>> {
    let root = parse(std::str::from_utf8(bytes).map_err(|_| invalid())?)?;
    let mut tables = vec![];
    collect(&root, "table", &mut tables);
    let names: BTreeSet<_> = tables
        .into_iter()
        .filter_map(|n| n.attrs.get("schema"))
        .filter(|s| matches!(s.as_str(), "heap-allocations" | "vm-tracker" | "leaks"))
        .cloned()
        .collect();
    Ok(names.into_iter().collect())
}
type Measurement = (f64, &'static str, &'static str);
fn table_measurement(
    name: &str,
    r: &Node,
    observation: &mut Value,
) -> Result<(&'static str, Option<Measurement>)> {
    Ok(
        if matches!(name, "heap-allocations" | "vm-tracker" | "leaks") {
            let mut frames = vec![];
            collect(r, "frame", &mut frames);
            if frames.len() > 128 {
                return Err(invalid());
            }
            observation["allocationStackLeafFirst"] =
                json!(frames.iter().map(|f| json_node(f)).collect::<Vec<_>>());
            observation["memoryMeaning"] = json!(
                "producer export observation; not an Xcode memory graph, GC event, native dominator or independently validated leak; addresses do not match across captures"
            );
            let metric=r.children.iter().find(|n|n.name=="size-in-bytes").and_then(|n|n.text.trim().parse::<f64>().ok()).filter(|n|n.is_finite()).map(|v|(v,"bytes","explicit direct producer size-in-bytes field; allocation/VM bucket semantics remain in exported fields; not retained size or RSS"));
            (
                if name == "heap-allocations" {
                    "allocation"
                } else if name == "leaks" {
                    "retention"
                } else {
                    "metric"
                },
                metric,
            )
        } else if name == "time-profile" {
            let mut frames = vec![];
            collect(r, "frame", &mut frames);
            if frames.len() > 128 {
                return Err(invalid());
            }
            let mut names:Vec<_>=frames.into_iter().map(|f|json!({"name":f.attrs.get("name").cloned().unwrap_or_else(||f.text.clone()),"address":f.attrs.get("addr"),"producer":json_node(f),"resolved":false})).collect();
            names.reverse();
            observation["framesRootFirst"] = json!(names);
            if let Some(weight) = value(r, &["weight"]) {
                (
                    "stack",
                    Some((
                        weight,
                        "nanoseconds",
                        "xctrace Time Profiler reported sample weight; not off-CPU or wall time",
                    )),
                )
            } else {
                (
                    "stack",
                    Some((
                        1.0,
                        "samples",
                        "one exported xctrace sample; sampling interval/weight unavailable",
                    )),
                )
            }
        } else if let Some(duration) = value(r, &["duration"]) {
            (
                "span",
                Some((
                    duration,
                    "nanoseconds",
                    "producer xctrace interval; overlapping intervals are not additive; no inferred wait cause",
                )),
            )
        } else {
            ("event", None)
        },
    )
}

pub(super) fn decode(index: &mut Index, e: &Value, section: &Value, ordinal: usize) -> Result<()> {
    let root = parse(text(&section["text"])?)?;
    let mut map = BTreeMap::new();
    ids(&root, &mut map)?;
    let name = text(&section["name"])?;
    if name == "toc" {
        emit(
            index,
            e,
            "event",
            "xctrace table inventory",
            format!("/sections/{ordinal}/text"),
            None,
            None,
            json!({"inventory":json_node(&root),"selectedRun":1,"templateValidation":"separate per target/template; no live range certified"}),
        )?;
        return Ok(());
    }
    let mut rows = vec![];
    collect(&root, "row", &mut rows);
    if rows.len() > 4096 {
        return Err(invalid());
    }
    if rows.is_empty() {
        index.unsupported.push(format!(
            "xctrace {name}: no exported rows; table availability/version not established"
        ));
    }
    for (i, r) in rows.iter().enumerate() {
        let r = expand(r, &map, 0, &mut 4096)?;
        let time = value(&r, &["sample-time", "start-time"]);
        let mut observation = json!({"table":name,"fields":json_node(&r),"sampleKind":"xctrace_export","symbolProvenance":"producer labels; external UUID/architecture unverified"});
        observation["thread"] = first(&r, "thread").map(json_node).unwrap_or(Value::Null);
        let (kind, measurement) = table_measurement(name, &r, &mut observation)?;
        emit(
            index,
            e,
            kind,
            name,
            format!("/sections/{ordinal}/text/row/{i}"),
            time,
            measurement,
            observation,
        )?;
    }
    Ok(())
}

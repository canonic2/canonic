//! Bounded function summaries of a saved V8 CPU profile (`v8-cpu-profile`).
//!
//! Self time comes from the sample timeline (`samples`/`timeDeltas`), or from
//! `hitCount` spread over the profile duration when the timeline is absent.
//! Total time counts each function once per call stack, so recursion does not
//! inflate it, matching the DevTools bottom-up view.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;

/// Largest node count a summary accepts.
const MAX_NODES: usize = 1_000_000;
const MAX_NAME: usize = 256;
const FORMAT: &str = "PROFILE_EVIDENCE_FORMAT";

fn function_rows(
    nodes: &[Node],
    referenced: &[bool],
    self_us: &[f64],
    samples: &[u64],
) -> Result<Vec<Row>, &'static str> {
    // Post-order subtree totals, then one pass per root that credits each
    // function only at its outermost frame on the current stack.
    let roots: Vec<usize> = (0..nodes.len()).filter(|&node| !referenced[node]).collect();
    if roots.is_empty() {
        return Err(FORMAT);
    }
    let mut order = Vec::with_capacity(nodes.len());
    let mut stack: Vec<usize> = roots.clone();
    while let Some(node) = stack.pop() {
        order.push(node);
        stack.extend(&nodes[node].children);
    }
    if order.len() != nodes.len() {
        return Err(FORMAT);
    }
    let mut subtree = self_us.to_vec();
    for &node in order.iter().rev() {
        let sum: f64 = nodes[node]
            .children
            .iter()
            .map(|&child| subtree[child])
            .sum();
        subtree[node] += sum;
    }
    let mut rows: HashMap<&Function, Row> = HashMap::new();
    let mut open: HashMap<&Function, usize> = HashMap::new();
    let mut walk: Vec<(usize, bool)> = roots.iter().map(|&root| (root, false)).collect();
    while let Some((node, leaving)) = walk.pop() {
        let function = &nodes[node].function;
        if leaving {
            if let Some(count) = open.get_mut(function) {
                *count -= 1;
            }
            continue;
        }
        let row = rows.entry(function).or_insert_with(|| Row {
            function: function.clone(),
            self_us: 0.0,
            total_us: 0.0,
            samples: 0,
        });
        row.self_us += self_us[node];
        row.samples += samples[node];
        let depth = open.entry(function).or_insert(0);
        if *depth == 0 {
            row.total_us += subtree[node];
        }
        *depth += 1;
        walk.push((node, true));
        walk.extend(nodes[node].children.iter().map(|&child| (child, false)));
    }
    let mut rows: Vec<Row> = rows
        .into_values()
        .filter(|row| row.function.name != "(root)")
        .collect();
    rows.sort_by(|a, b| a.function.cmp(&b.function));
    Ok(rows)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sort {
    #[default]
    #[serde(rename = "self")]
    SelfTime,
    #[serde(rename = "total")]
    Total,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Query {
    Functions {
        #[serde(default, rename = "sortBy")]
        sort_by: Sort,
        #[serde(default)]
        offset: usize,
        #[serde(default = "limit")]
        limit: usize,
    },
}
fn limit() -> usize {
    50
}
impl Query {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        let Self::Functions { offset, limit, .. } = self;
        if *offset > 100_000 || !(1..=100).contains(limit) {
            Err("PROFILE_QUERY_BOUNDS")
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Function {
    name: String,
    url: String,
    line: i64,
    column: i64,
}
#[derive(Debug)]
struct Row {
    function: Function,
    self_us: f64,
    total_us: f64,
    samples: u64,
}
/// One parsed profile, aggregated by function and reusable across queries.
#[derive(Debug)]
pub(crate) struct Summary {
    duration_us: f64,
    sampled_us: f64,
    sample_count: u64,
    timeline: bool,
    rows: Vec<Row>,
}

struct Node {
    function: Function,
    children: Vec<usize>,
    hits: u64,
}

fn number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|value| value.is_finite())
}

impl Summary {
    pub(crate) fn parse(profile: &Value) -> Result<Self, &'static str> {
        let raw = profile["nodes"].as_array().ok_or(FORMAT)?;
        if raw.is_empty() || raw.len() > MAX_NODES {
            return Err(FORMAT);
        }
        let mut index = HashMap::with_capacity(raw.len());
        for (position, node) in raw.iter().enumerate() {
            if index
                .insert(node["id"].as_i64().ok_or(FORMAT)?, position)
                .is_some()
            {
                return Err(FORMAT);
            }
        }
        let mut referenced = vec![false; raw.len()];
        let mut nodes = Vec::with_capacity(raw.len());
        for node in raw {
            let frame = &node["callFrame"];
            let mut children = Vec::new();
            for child in node["children"].as_array().map_or(&[][..], Vec::as_slice) {
                let child = *index.get(&child.as_i64().ok_or(FORMAT)?).ok_or(FORMAT)?;
                if std::mem::replace(&mut referenced[child], true) {
                    return Err(FORMAT);
                }
                children.push(child);
            }
            let name = frame["functionName"].as_str().unwrap_or("");
            let name = if name.is_empty() { "(anonymous)" } else { name };
            nodes.push(Node {
                function: Function {
                    name: name.chars().take(MAX_NAME).collect(),
                    url: crate::browser::diagnostics::url(frame["url"].as_str().unwrap_or("")),
                    line: frame["lineNumber"].as_i64().unwrap_or(-1),
                    column: frame["columnNumber"].as_i64().unwrap_or(-1),
                },
                children,
                hits: node["hitCount"].as_u64().unwrap_or(0),
            });
        }
        let start = number(&profile["startTime"]).ok_or(FORMAT)?;
        let end = number(&profile["endTime"]).ok_or(FORMAT)?;
        let duration_us = (end - start).max(0.0);
        let mut self_us = vec![0.0; nodes.len()];
        let mut samples = vec![0_u64; nodes.len()];
        let ids = profile["samples"].as_array().map_or(&[][..], Vec::as_slice);
        let deltas = profile["timeDeltas"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        let timeline = !ids.is_empty() && ids.len() == deltas.len();
        if timeline {
            // Each sample lasts until the next one; the last until endTime.
            let mut stamps = Vec::with_capacity(ids.len());
            let mut stamp = start;
            for delta in deltas {
                stamp += number(delta).ok_or(FORMAT)?;
                stamps.push(stamp);
            }
            for (position, id) in ids.iter().enumerate() {
                let node = *index.get(&id.as_i64().ok_or(FORMAT)?).ok_or(FORMAT)?;
                let next = stamps.get(position + 1).copied().unwrap_or(end);
                self_us[node] += (next - stamps[position]).max(0.0);
                samples[node] += 1;
            }
        } else {
            let hits: u64 = nodes.iter().map(|node| node.hits).sum();
            for (position, node) in nodes.iter().enumerate() {
                samples[position] = node.hits;
                if hits > 0 {
                    self_us[position] = duration_us * node.hits as f64 / hits as f64;
                }
            }
        }
        let rows = function_rows(&nodes, &referenced, &self_us, &samples)?;
        Ok(Self {
            duration_us,
            sampled_us: self_us.iter().sum(),
            sample_count: samples.iter().sum(),
            timeline,
            rows,
        })
    }

    /// One page of functions in the query's order, with a unique tie-breaker.
    pub(crate) fn page(&self, query: &Query) -> Value {
        let Query::Functions {
            sort_by,
            offset,
            limit,
        } = query;
        let key = |row: &Row| match sort_by {
            Sort::SelfTime => (row.self_us, row.total_us),
            Sort::Total => (row.total_us, row.self_us),
        };
        let mut order: Vec<&Row> = self.rows.iter().collect();
        order.sort_by(|a, b| {
            let (a_first, a_second) = key(a);
            let (b_first, b_second) = key(b);
            b_first
                .total_cmp(&a_first)
                .then(b_second.total_cmp(&a_second))
                .then_with(|| a.function.cmp(&b.function))
        });
        let ms = |us: f64| (us / 10.0).round() / 100.0;
        let percent = |us: f64| {
            if self.sampled_us > 0.0 {
                (us * 10000.0 / self.sampled_us).round() / 100.0
            } else {
                0.0
            }
        };
        let end = (*offset + *limit).min(order.len());
        let items: Vec<Value> = order
            .get(*offset..end)
            .unwrap_or(&[])
            .iter()
            .map(|row| json!({"functionName":row.function.name,"url":row.function.url,"lineNumber":row.function.line,"columnNumber":row.function.column,"selfMs":ms(row.self_us),"selfPercent":percent(row.self_us),"totalMs":ms(row.total_us),"totalPercent":percent(row.total_us),"selfSamples":row.samples}))
            .collect();
        let end_position = offset + items.len();
        json!({"kind":"functions","sortBy":sort_by,"profileDurationMs":ms(self.duration_us),"sampledMs":ms(self.sampled_us),"sampleCount":self.sample_count,"timing":if self.timeline {"sample-timeline"} else {"hit-count-estimate"},"lineBase":0,"total":order.len(),"offset":offset,"endPosition":end_position,"nextOffset":(end_position < order.len()).then_some(end_position),"items":items})
    }
}

pub(crate) fn schema() -> Value {
    json!({"oneOf":[{"type":"object","properties":{"kind":{"const":"functions"},"sortBy":{"enum":["self","total"],"default":"self"},"offset":{"type":"integer","minimum":0,"maximum":100000,"default":0},"limit":{"type":"integer","minimum":1,"maximum":100,"default":50}},"required":["kind"],"additionalProperties":false}]})
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(name: &str) -> Value {
        json!({"functionName":name,"url":"https://example.com/acme.js?token=Acme","lineNumber":3,"columnNumber":1,"scriptId":"1"})
    }

    /// root → main → acme → acme (recursion) and root → (idle); four samples
    /// of 1 ms each, the last running to endTime.
    fn profile() -> Value {
        json!({"nodes":[
            {"id":1,"callFrame":frame("(root)"),"children":[2,5]},
            {"id":2,"callFrame":frame("main"),"children":[3]},
            {"id":3,"callFrame":frame("acme"),"children":[4]},
            {"id":4,"callFrame":frame("acme"),"children":[]},
            {"id":5,"callFrame":frame("(idle)"),"children":[]}
        ],"startTime":0,"endTime":4000,"samples":[2,3,4,5],"timeDeltas":[0,1000,1000,1000]})
    }

    #[test]
    fn self_and_total_time_follow_the_sample_timeline_without_counting_recursion_twice() {
        let summary = Summary::parse(&profile()).unwrap();
        let page = summary.page(&Query::Functions {
            sort_by: Sort::Total,
            offset: 0,
            limit: 10,
        });
        assert_eq!(page["sampledMs"], 4.0);
        assert_eq!(page["sampleCount"], 4);
        assert_eq!(page["timing"], "sample-timeline");
        let items = page["items"].as_array().unwrap();
        // (root) is omitted; main covers itself and both acme frames.
        assert_eq!(page["total"], 3);
        assert_eq!(items[0]["functionName"], "main");
        assert_eq!(items[0]["totalMs"], 3.0);
        assert_eq!(items[0]["selfMs"], 1.0);
        assert_eq!(items[1]["functionName"], "acme");
        assert_eq!(items[1]["selfMs"], 2.0);
        assert_eq!(items[1]["selfSamples"], 2);
        assert_eq!(items[1]["totalMs"], 2.0, "recursion counted once");
        assert_eq!(items[1]["totalPercent"], 50.0);
        assert_eq!(items[1]["url"], "https://example.com/acme.js");
        let by_self = summary.page(&Query::Functions {
            sort_by: Sort::SelfTime,
            offset: 0,
            limit: 1,
        });
        assert_eq!(by_self["items"][0]["functionName"], "acme");
        assert_eq!(by_self["nextOffset"], 1);
        let last = summary.page(&Query::Functions {
            sort_by: Sort::SelfTime,
            offset: 2,
            limit: 5,
        });
        assert_eq!(last["items"].as_array().unwrap().len(), 1);
        assert_eq!(last["nextOffset"], Value::Null);
    }

    #[test]
    fn hit_counts_estimate_self_time_when_the_timeline_is_absent() {
        let mut profile = profile();
        profile.as_object_mut().unwrap().remove("samples");
        profile.as_object_mut().unwrap().remove("timeDeltas");
        profile["nodes"][2]["hitCount"] = json!(3);
        profile["nodes"][4]["hitCount"] = json!(1);
        let page = Summary::parse(&profile).unwrap().page(&Query::Functions {
            sort_by: Sort::SelfTime,
            offset: 0,
            limit: 10,
        });
        assert_eq!(page["timing"], "hit-count-estimate");
        assert_eq!(page["items"][0]["functionName"], "acme");
        assert_eq!(page["items"][0]["selfMs"], 3.0);
    }

    #[test]
    fn malformed_profiles_and_out_of_range_pages_are_refused() {
        let mut cyclic = profile();
        cyclic["nodes"][3]["children"] = json!([2]);
        assert_eq!(
            Summary::parse(&cyclic).unwrap_err(),
            "PROFILE_EVIDENCE_FORMAT"
        );
        let mut unknown = profile();
        unknown["samples"][0] = json!(99);
        assert_eq!(
            Summary::parse(&unknown).unwrap_err(),
            "PROFILE_EVIDENCE_FORMAT"
        );
        assert_eq!(
            Summary::parse(&json!({})).unwrap_err(),
            "PROFILE_EVIDENCE_FORMAT"
        );
        for (offset, limit) in [(0, 0), (0, 101), (100_001, 1)] {
            let query = Query::Functions {
                sort_by: Sort::SelfTime,
                offset,
                limit,
            };
            assert_eq!(query.validate(), Err("PROFILE_QUERY_BOUNDS"));
        }
        let query: Query = serde_json::from_value(json!({"kind":"functions"})).unwrap();
        assert!(query.validate().is_ok());
        crate::schema::validate(
            &schema(),
            &json!({"kind":"functions","sortBy":"total","limit":100}),
        )
        .unwrap();
    }
}

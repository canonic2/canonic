//! Bounded profile.proto reader. Stored sample types/units remain distinct.
use super::*;
use std::collections::BTreeMap;
#[derive(Clone, Copy)]
enum Field<'a> {
    Number(u64),
    Bytes(&'a [u8]),
    Fixed,
}
struct Message<'a>(Vec<(u32, Field<'a>)>);
fn varint(bytes: &[u8], position: &mut usize) -> Result<u64> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let b = *bytes.get(*position).ok_or_else(invalid)?;
        *position += 1;
        if shift == 63 && b > 1 {
            return Err(invalid());
        }
        value |= ((b & 127) as u64) << shift;
        if b & 128 == 0 {
            return Ok(value);
        }
    }
    Err(invalid())
}
impl<'a> Message<'a> {
    fn parse(bytes: &'a [u8]) -> Result<Self> {
        let mut fields = vec![];
        let mut p = 0;
        while p < bytes.len() {
            if fields.len() >= 100000 {
                return Err(invalid());
            }
            let tag = varint(bytes, &mut p)?;
            let id = u32::try_from(tag >> 3)
                .ok()
                .filter(|n| *n > 0 && *n < 1 << 29)
                .ok_or_else(invalid)?;
            let field = match tag & 7 {
                0 => Field::Number(varint(bytes, &mut p)?),
                1 | 5 => {
                    let n = if tag & 7 == 1 { 8 } else { 4 };
                    let end = p.checked_add(n).ok_or_else(invalid)?;
                    bytes.get(p..end).ok_or_else(invalid)?;
                    p = end;
                    Field::Fixed
                }
                2 => {
                    let n = usize::try_from(varint(bytes, &mut p)?).map_err(|_| invalid())?;
                    let end = p.checked_add(n).ok_or_else(invalid)?;
                    let b = bytes.get(p..end).ok_or_else(invalid)?;
                    p = end;
                    Field::Bytes(b)
                }
                _ => return Err(invalid()),
            };
            fields.push((id, field));
        }
        Ok(Self(fields))
    }
    fn number(&self, id: u32) -> Result<u64> {
        let mut values = self.0.iter().filter(|(n, _)| *n == id);
        let value = match values.next() {
            None => 0,
            Some((_, Field::Number(n))) => *n,
            _ => return Err(invalid()),
        };
        if values.next().is_some() {
            return Err(invalid());
        }
        Ok(value)
    }
    fn bytes(&self, id: u32, max: usize) -> Result<Vec<&'a [u8]>> {
        let mut out = vec![];
        for (_, f) in self.0.iter().filter(|(n, _)| *n == id) {
            let Field::Bytes(b) = f else {
                return Err(invalid());
            };
            out.push(*b);
            if out.len() > max {
                return Err(invalid());
            }
        }
        Ok(out)
    }
    fn packed(&self, id: u32, max: usize) -> Result<Vec<u64>> {
        let mut out = vec![];
        for (_, f) in self.0.iter().filter(|(n, _)| *n == id) {
            match f {
                Field::Number(n) => out.push(*n),
                Field::Bytes(b) => {
                    let mut p = 0;
                    while p < b.len() {
                        out.push(varint(b, &mut p)?);
                        if out.len() > max {
                            return Err(invalid());
                        }
                    }
                }
                Field::Fixed => return Err(invalid()),
            }
            if out.len() > max {
                return Err(invalid());
            }
        }
        Ok(out)
    }
}
pub(super) fn decode(bytes: &[u8], index: &mut Index) -> Result<()> {
    let p = Message::parse(bytes)?;
    let mut strings = vec![];
    for b in p.bytes(6, 20000)? {
        let s = std::str::from_utf8(b).map_err(|_| invalid())?;
        if s.len() > 4096 {
            return Err(invalid());
        }
        strings.push(s);
    }
    if strings.first() != Some(&"") {
        return Err(invalid());
    }
    let string = |id: u64| -> Result<&str> {
        strings
            .get(usize::try_from(id).map_err(|_| invalid())?)
            .copied()
            .ok_or_else(invalid)
    };
    let mut types = vec![];
    for raw in p.bytes(1, 16)? {
        let t = Message::parse(raw)?;
        let name = string(t.number(1)?)?;
        let unit = string(t.number(2)?)?;
        label(name)?;
        label(unit)?;
        types.push((name, unit));
    }
    if types.is_empty() {
        return Err(invalid());
    }
    let mut mappings = BTreeMap::new();
    for raw in p.bytes(3, 20000)? {
        let m = Message::parse(raw)?;
        let id = m.number(1)?;
        if id==0||mappings.insert(id,json!({"id":id,"filename":string(m.number(5)?)?,"buildId":string(m.number(6)?)?,"memoryStart":format!("0x{:x}",m.number(2)?),"memoryLimit":format!("0x{:x}",m.number(3)?)})).is_some(){return Err(invalid());}
    }
    let mut functions = BTreeMap::new();
    for raw in p.bytes(5, 20000)? {
        let f = Message::parse(raw)?;
        let id = f.number(1)?;
        if id==0||functions.insert(id,json!({"name":string(f.number(2)?)?,"systemName":string(f.number(3)?)?,"file":string(f.number(4)?)?,"startLine":f.number(5)? as i64})).is_some(){return Err(invalid());}
    }
    let locations = profile_locations(&p, &mappings, &functions)?;
    for (n, raw) in p.bytes(2, 4096)?.into_iter().enumerate() {
        let sample = Message::parse(raw)?;
        let ids = sample.packed(1, 256)?;
        let values = sample.packed(2, 16)?;
        if values.len() != types.len() {
            return Err(invalid());
        }
        let mut frames = vec![];
        for id in ids {
            frames.extend(locations.get(&id).ok_or_else(invalid)?.iter().cloned());
            if frames.len() > 256 {
                return Err(invalid());
            }
        }
        frames.reverse();
        let mut labels = vec![];
        for raw in sample.bytes(3, 32)? {
            let l = Message::parse(raw)?;
            let key = string(l.number(1)?)?;
            label(key)?;
            let str_value = l.number(2)?;
            let numeric = l.0.iter().any(|(id, _)| *id == 3);
            if str_value != 0 && numeric {
                return Err(invalid());
            }
            labels.push(json!({"key":key,"string":if str_value==0{None}else{Some(string(str_value)?)},"number":if numeric{Some(l.number(3)? as i64)}else{None},"unit":string(l.number(4)?)?}));
        }
        for (column, ((name, unit), value)) in types.iter().zip(values).enumerate() {
            let value = value as i64;
            if value.unsigned_abs() > 1 << 53 {
                return Err(Error::new(
                    "ANALYSIS_BOUNDS",
                    "pprof sample cannot be represented exactly in numeric queries; inspect raw evidence",
                ));
            }
            let mut row = decode::row(
                "stack",
                name,
                format!("/sample/{n}/value/{column}"),
                json!({"frames":frames,"labels":labels,"sampleType":name,"profileTimeNanos":(p.number(9)? as i64).to_string(),"durationNanos":(p.number(10)? as i64).to_string(),"period":p.number(12)? as i64,"symbolication":"producer mappings/functions; build strings are not verified native symbols"}),
            );
            row.value = Some(value as f64);
            row.unit = Some((*unit).into());
            row.definition = Some(format!(
                "pprof {name} stored sample value; not a complete heap, CPU percentage or causal guarantee"
            ));
            decode::push(index, row)?;
        }
    }
    index.unsupported.push("pprof selected bounded profile.proto semantics; raw labels/mappings preserved; drop/keep regexes not executed; no Go trace, complete goroutine census, heap dominators or symbol lookup".into());
    Ok(())
}

fn profile_locations(
    p: &Message<'_>,
    mappings: &BTreeMap<u64, Value>,
    functions: &BTreeMap<u64, Value>,
) -> Result<BTreeMap<u64, Vec<Value>>> {
    let mut locations = BTreeMap::new();
    for raw in p.bytes(4, 20000)? {
        let l = Message::parse(raw)?;
        let id = l.number(1)?;
        let mapping = l.number(2)?;
        if id == 0 || mapping != 0 && !mappings.contains_key(&mapping) {
            return Err(invalid());
        }
        let mut frames = vec![];
        for raw in l.bytes(4, 256)? {
            let line = Message::parse(raw)?;
            let mut frame = functions
                .get(&line.number(1)?)
                .cloned()
                .ok_or_else(invalid)?;
            frame["line"] = json!(line.number(2)? as i64);
            frame["address"] = json!(format!("0x{:x}", l.number(3)?));
            frame["mapping"] = mappings.get(&mapping).cloned().unwrap_or(Value::Null);
            frames.push(frame);
        }
        if frames.is_empty() {
            frames.push(json!({"name":format!("0x{:x}",l.number(3)?),"mapping":mappings.get(&mapping),"symbolication":"unresolved"}));
        }
        if locations.insert(id, frames).is_some() {
            return Err(invalid());
        }
    }
    Ok(locations)
}

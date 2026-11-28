//! Bounded Java/Android HPROF 1.0.1–1.0.3 reader; no Android lifecycle heuristics.
use super::*;
#[derive(Clone)]
struct Class {
    super_id: u64,
    size: u32,
    fields: Vec<(u64, u8)>,
    edges: Vec<Edge>,
    offset: usize,
    stack: u32,
}
struct Instance {
    id: u64,
    class: u64,
    data: Vec<u8>,
    offset: usize,
    stack: u32,
}
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    width: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or_else(invalid)?;
        let bytes = self.bytes.get(self.pos..end).ok_or_else(invalid)?;
        self.pos = end;
        Ok(bytes)
    }
    fn num(&mut self, n: usize) -> Result<u64> {
        Ok(self.take(n)?.iter().fold(0, |a, b| (a << 8) | *b as u64))
    }
    fn id(&mut self) -> Result<u64> {
        self.num(self.width)
    }
    fn object_id(&mut self) -> Result<u64> {
        let id = self.id()?;
        if id == 0 {
            return Err(invalid());
        }
        Ok(id)
    }
    fn u1(&mut self) -> Result<u8> {
        Ok(self.num(1)? as u8)
    }
    fn u2(&mut self) -> Result<usize> {
        Ok(self.num(2)? as usize)
    }
    fn u4(&mut self) -> Result<u32> {
        Ok(self.num(4)? as u32)
    }
    fn value(&mut self, ty: u8) -> Result<u64> {
        let n = match ty {
            2 => self.width,
            4 | 8 => 1,
            5 | 9 => 2,
            6 | 10 => 4,
            7 | 11 => 8,
            _ => return Err(invalid()),
        };
        let value = self.num(n)?;
        if ty == 4 && value > 1 {
            return Err(invalid());
        }
        Ok(value)
    }
}
fn id(id: u64) -> String {
    format!("0x{id:x}")
}
fn edge(to: u64, name: String, strength: &str) -> Option<Edge> {
    (to != 0).then(|| Edge {
        to: id(to),
        name,
        strength: strength.into(),
    })
}
fn name(strings: &BTreeMap<u64, String>, key: u64) -> String {
    strings
        .get(&key)
        .filter(|s| !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control))
        .cloned()
        .unwrap_or_else(|| format!("name@{}", id(key)))
}
pub(super) fn decode(bytes: &[u8]) -> Result<Graph> {
    let nul = bytes
        .iter()
        .position(|&b| b == 0)
        .filter(|&n| n <= 32)
        .ok_or_else(invalid)?;
    let header = std::str::from_utf8(&bytes[..nul]).map_err(|_| invalid())?;
    if !matches!(
        header,
        "JAVA PROFILE 1.0.1" | "JAVA PROFILE 1.0.2" | "JAVA PROFILE 1.0.3"
    ) {
        return Err(invalid());
    }
    let mut r = Reader {
        bytes,
        pos: nul + 1,
        width: 4,
    };
    let width = r.u4()? as usize;
    if !matches!(width, 4 | 8) {
        return Err(invalid());
    }
    r.width = width;
    let timestamp = r.num(8)?;
    let mut heap = Heap {
        android: header.ends_with("1.0.3"),
        ..Heap::default()
    };
    while r.pos < bytes.len() {
        heap.records += 1;
        if heap.records > 100000 {
            return Err(Error::new(
                "ANALYSIS_BOUNDS",
                "HPROF exceeds 100000 records",
            ));
        }
        let tag = r.u1()?;
        r.u4()?;
        let len = r.u4()? as usize;
        let start = r.pos;
        let data = r.take(len)?;
        let mut t = Reader {
            bytes: data,
            pos: 0,
            width,
        };
        heap.read_record(tag, &mut t, start)?;
    }
    if (!heap.full && (!heap.ended || heap.segments == 0))
        || heap.objects.len() + heap.instances.len() + heap.classes.len() > MAX_NODES
    {
        return Err(invalid());
    }
    heap.graph(header, width, timestamp)
}

#[derive(Default)]
struct Heap {
    strings: BTreeMap<u64, String>,
    names: BTreeMap<u64, u64>,
    classes: BTreeMap<u64, Class>,
    instances: Vec<Instance>,
    objects: Vec<(Object, u64, u32)>,
    roots: BTreeMap<u64, Vec<String>>,
    frames: BTreeMap<u64, (u64, u64, u32, i32)>,
    traces: BTreeMap<u32, Vec<u64>>,
    serial_classes: BTreeMap<u32, u64>,
    records: usize,
    segments: usize,
    ended: bool,
    full: bool,
    android: bool,
}
impl Heap {
    fn read_record(&mut self, tag: u8, t: &mut Reader<'_>, start: usize) -> Result<()> {
        let len = t.bytes.len();
        match tag {
            0x01 => {
                let key = t.id()?;
                let value =
                    String::from_utf8_lossy(t.take(len.checked_sub(t.width).ok_or_else(invalid)?)?)
                        .into_owned();
                if self.strings.insert(key, value).is_some() {
                    return Err(invalid());
                }
            }
            0x02 => {
                let serial = t.u4()?;
                let class = t.id()?;
                t.u4()?;
                let key = t.id()?;
                if self.names.insert(class, key).is_some()
                    || self.serial_classes.insert(serial, class).is_some()
                {
                    return Err(invalid());
                }
            }
            0x04 => {
                let key = t.id()?;
                let method = t.id()?;
                t.id()?;
                let file = t.id()?;
                let serial = t.u4()?;
                let line = t.u4()? as i32;
                if self
                    .frames
                    .insert(key, (method, file, serial, line))
                    .is_some()
                {
                    return Err(invalid());
                }
            }
            0x05 => {
                let key = t.u4()?;
                t.u4()?;
                let count = t.u4()? as usize;
                if count > 64 {
                    return Err(Error::new(
                        "ANALYSIS_BOUNDS",
                        "HPROF allocation trace exceeds 64 frames",
                    ));
                }
                let mut ids = vec![];
                for _ in 0..count {
                    ids.push(t.id()?);
                }
                if self.traces.insert(key, ids).is_some() {
                    return Err(invalid());
                }
            }
            0x0c | 0x1c => self.read_heap_dump(tag, t, start)?,
            0x2c => {
                if self.ended || len != 0 || self.segments == 0 {
                    return Err(invalid());
                }
                self.ended = true;
            }
            _ => {} // Top-level records are length delimited; no heap edges inferred.
        }
        if matches!(tag, 0x01 | 0x02 | 0x04 | 0x05) && t.pos != t.bytes.len() {
            return Err(invalid());
        }
        Ok(())
    }

    fn read_heap_dump(&mut self, tag: u8, t: &mut Reader<'_>, start: usize) -> Result<()> {
        if self.ended || self.full {
            return Err(invalid());
        }
        if tag == 0x0c {
            self.full = true;
        } else {
            self.segments += 1;
        }
        while t.pos < t.bytes.len() {
            self.records += 1;
            if self.records > 100000
                || self.objects.len() + self.instances.len() + self.classes.len() > MAX_NODES
            {
                return Err(Error::new(
                    "ANALYSIS_BOUNDS",
                    "HPROF object/record budget exceeded",
                ));
            }
            let offset = start + t.pos;
            let sub = t.u1()?;
            self.read_heap_item(sub, t, offset)?;
        }
        Ok(())
    }

    fn read_heap_item(&mut self, sub: u8, t: &mut Reader<'_>, offset: usize) -> Result<()> {
        match sub {
            0xfe => {
                self.android = true;
                t.u4()?;
                t.id()?;
            }
            0xff | 0x01..=0x08 | 0x89..=0x8e | 0x90 => {
                let object = t.id()?;
                match sub {
                    0x01 => {
                        t.id()?;
                    }
                    0x02 | 0x03 | 0x08 | 0x8e => {
                        t.u4()?;
                        t.u4()?;
                    }
                    0x04 | 0x06 => {
                        t.u4()?;
                    }
                    _ => {}
                }
                if matches!(sub, 0x89..=0x8e | 0x90) {
                    self.android = true;
                }
                if sub != 0x90 {
                    let list = self.roots.entry(object).or_default();
                    if list.len() >= 32 {
                        return Err(invalid());
                    }
                    list.push(format!("hprof-root-0x{sub:02x}"));
                }
            }
            0x20 => self.read_class(t, offset)?,
            0x21 => {
                let object = t.object_id()?;
                let stack = t.u4()?;
                let class = t.id()?;
                let len = t.u4()? as usize;
                self.instances.push(Instance {
                    id: object,
                    class,
                    data: t.take(len)?.to_vec(),
                    offset,
                    stack,
                });
            }
            0x22 => self.read_object_array(t, offset)?,
            0x23 => self.read_primitive_array(t, offset)?,
            // A no-data array has no portable payload/identity model.
            _ => {
                return Err(Error::new(
                    "UNSUPPORTED_HPROF_RECORD",
                    format!("Unsupported heap subrecord 0x{sub:02x}; raw evidence preserved"),
                ));
            }
        }
        Ok(())
    }

    fn read_class(&mut self, t: &mut Reader<'_>, offset: usize) -> Result<()> {
        let object = t.object_id()?;
        let stack = t.u4()?;
        let super_id = t.id()?;
        let loader = t.id()?;
        let signers = t.id()?;
        let protection = t.id()?;
        t.id()?;
        t.id()?;
        let size = t.u4()?;
        let mut edges = vec![];
        for (to, label) in [
            (super_id, "superclass"),
            (loader, "classLoader"),
            (signers, "signers"),
            (protection, "protectionDomain"),
        ] {
            if let Some(e) = edge(to, label.into(), "strong") {
                edges.push(e);
            }
        }
        let count = t.u2()?;
        for _ in 0..count {
            let index = t.u2()?;
            let ty = t.u1()?;
            let value = t.value(ty)?;
            if ty == 2
                && let Some(e) = edge(value, format!("constantPool[{index}]"), "strong")
            {
                edges.push(e);
            }
        }
        let count = t.u2()?;
        for _ in 0..count {
            let key = t.id()?;
            let ty = t.u1()?;
            let value = t.value(ty)?;
            if ty == 2
                && let Some(e) = edge(value, format!("static@{}", id(key)), "strong")
            {
                edges.push(e);
            }
        }
        let count = t.u2()?;
        let mut fields = vec![];
        for _ in 0..count {
            let key = t.id()?;
            let ty = t.u1()?;
            if !matches!(ty, 2 | 4..=11) {
                return Err(invalid());
            }
            fields.push((key, ty));
        }
        if self
            .classes
            .insert(
                object,
                Class {
                    super_id,
                    size,
                    fields,
                    edges,
                    offset,
                    stack,
                },
            )
            .is_some()
        {
            return Err(invalid());
        }
        Ok(())
    }

    fn read_object_array(&mut self, t: &mut Reader<'_>, offset: usize) -> Result<()> {
        let object = t.object_id()?;
        let stack = t.u4()?;
        let count = t.u4()? as usize;
        let class = t.id()?;
        if count > 50000 {
            return Err(Error::new(
                "ANALYSIS_BOUNDS",
                "HPROF object array exceeds 50000 entries",
            ));
        }
        let mut edges = vec![];
        if let Some(e) = edge(class, "class".into(), "strong") {
            edges.push(e);
        }
        for i in 0..count {
            if let Some(e) = edge(t.id()?, format!("[{i}]"), "strong") {
                edges.push(e);
            }
        }
        self.objects.push((
            Object {
                id: id(object),
                class_name: format!("array-class@{}", id(class)),
                kind: "object_array".into(),
                self_bytes: (count * t.width) as u64,
                edges,
                source: format!("binary:offset:{offset}"),
                ..Object::default()
            },
            class,
            stack,
        ));
        Ok(())
    }

    fn read_primitive_array(&mut self, t: &mut Reader<'_>, offset: usize) -> Result<()> {
        let object = t.object_id()?;
        let stack = t.u4()?;
        let count = t.u4()? as usize;
        let ty = t.u1()?;
        let size = match ty {
            4 | 8 => 1,
            5 | 9 => 2,
            6 | 10 => 4,
            7 | 11 => 8,
            _ => return Err(invalid()),
        };
        let size = count.checked_mul(size).ok_or_else(invalid)?;
        t.take(size)?;
        self.objects.push((
            Object {
                id: id(object),
                class_name: format!("primitive_array_type_{ty}"),
                kind: "primitive_array".into(),
                self_bytes: size as u64,
                source: format!("binary:offset:{offset}"),
                ..Object::default()
            },
            0,
            stack,
        ));
        Ok(())
    }

    fn class_name(&self, class: u64) -> String {
        self.names
            .get(&class)
            .map(|key| name(&self.strings, *key).replace('/', "."))
            .unwrap_or_else(|| format!("class@{}", id(class)))
    }

    fn allocation(&self, serial: u32) -> Vec<String> {
        self.traces
            .get(&serial)
            .into_iter()
            .flatten()
            .map(|key| {
                if let Some((method, file, class, line)) = self.frames.get(key) {
                    format!(
                        "{}::{} ({}:{line})",
                        self.serial_classes
                            .get(class)
                            .map(|&c| self.class_name(c))
                            .unwrap_or_else(|| "unknown".into()),
                        name(&self.strings, *method),
                        name(&self.strings, *file)
                    )
                } else {
                    format!("unresolved frame {}", id(*key))
                }
            })
            .collect()
    }

    fn class_object(&self, key: u64, class: &Class) -> Object {
        let mut edges = class.edges.clone();
        for e in &mut edges {
            if let Some(key) = e
                .name
                .strip_prefix("static@0x")
                .and_then(|s| u64::from_str_radix(s, 16).ok())
            {
                e.name = format!("static {}", name(&self.strings, key));
            }
        }
        Object {
            id: id(key),
            class_name: format!("java.lang.Class<{}>", self.class_name(key)),
            kind: "class".into(),
            self_bytes: 0,
            edges,
            allocation_stack: self.allocation(class.stack),
            source: format!("binary:offset:{}", class.offset),
            ..Object::default()
        }
    }

    fn instance(&self, instance: Instance, width: usize) -> Result<(Object, bool)> {
        let mut complete = true;
        let mut node = Object {
            id: id(instance.id),
            class_name: self.class_name(instance.class),
            kind: "instance".into(),
            source: format!("binary:offset:{}", instance.offset),
            allocation_stack: self.allocation(instance.stack),
            ..Object::default()
        };
        if let Some(e) = edge(instance.class, "class".into(), "strong") {
            node.edges.push(e);
        }
        let mut class_id = instance.class;
        let mut seen = BTreeSet::new();
        let mut values = Reader {
            bytes: &instance.data,
            pos: 0,
            width,
        };
        let mut layout_complete = true;
        if let Some(c) = self.classes.get(&class_id) {
            node.self_bytes = c.size as u64;
        } else {
            complete = false;
            layout_complete = false;
        }
        while class_id != 0 {
            if !seen.insert(class_id) || seen.len() > 128 {
                return Err(invalid());
            }
            let Some(class) = self.classes.get(&class_id) else {
                complete = false;
                layout_complete = false;
                break;
            };
            let class_label = self.class_name(class_id);
            if class_label.starts_with("class@")
                || class_label.starts_with("name@")
                || class
                    .fields
                    .iter()
                    .any(|(key, _)| !self.strings.contains_key(key))
            {
                complete = false;
            }
            for &(key, ty) in &class.fields {
                let field = name(&self.strings, key);
                let value = values.value(ty)?;
                if ty == 2 {
                    let strength = if class_label.starts_with("class@")
                        || class_label.starts_with("name@")
                        || field.starts_with("name@")
                    {
                        complete = false;
                        "unknown"
                    } else if class_label == "java.lang.ref.Reference" && field == "referent" {
                        "weak"
                    } else {
                        "strong"
                    };
                    if let Some(e) = edge(value, format!("{class_label}.{field}"), strength) {
                        node.edges.push(e);
                    }
                }
                if class_label == "android.app.Activity" && field == "mDestroyed" && ty == 4 {
                    node.attributes
                        .insert("androidActivityDestroyed".into(), json!(value != 0));
                }
                if class_label == "android.app.Activity" && field == "mFinished" && ty == 4 {
                    node.attributes
                        .insert("androidActivityFinished".into(), json!(value != 0));
                }
            }
            class_id = class.super_id;
        }
        if layout_complete && values.pos != instance.data.len() {
            return Err(invalid());
        }
        Ok((node, complete))
    }

    fn graph(mut self, header: &str, width: usize, timestamp: u64) -> Result<Graph> {
        let mut complete = true;
        let mut gaps=vec!["HPROF instance_size and array payload byte weights exclude class-object/header/alignment/native/RSS ownership; imported process/build identity unverified".into()];
        for (&key, class) in &self.classes {
            let object = self.class_object(key, class);
            self.objects.push((object, 0, 0));
        }
        for instance in std::mem::take(&mut self.instances) {
            let (node, layout_complete) = self.instance(instance, width)?;
            complete &= layout_complete;
            self.objects.push((node, 0, 0));
        }
        let mut objects = std::mem::take(&mut self.objects);
        for (o, class, stack) in &mut objects {
            if *class != 0 {
                o.class_name = self.class_name(*class);
            }
            if *stack != 0 {
                o.allocation_stack = self.allocation(*stack);
            }
            if let Some(key) =
                o.id.strip_prefix("0x")
                    .and_then(|s| u64::from_str_radix(s, 16).ok())
            {
                o.roots = self.roots.remove(&key).unwrap_or_default();
            }
        }
        if self.classes.keys().any(|id| {
            self.names
                .get(id)
                .is_none_or(|key| !self.strings.contains_key(key))
        }) {
            complete = false;
            gaps.push("Missing class names: runtime reference semantics are incomplete".into());
        }
        gaps.push("Allocation stack serials can be zero or unavailable; empty stacks do not prove allocation-stack absence".into());
        if !self.roots.is_empty() {
            complete = false;
            gaps.push("Some recorded roots are outside the supplied heap graph".into());
        }
        Ok(Graph{schema_version:1,runtime:if self.android{"android_hprof"}else{"jvm_hprof"}.into(),size_definition:"HPROF modeled bytes: CLASS_DUMP instance_size for instances; element payload bytes for arrays; zero class metadata bytes; excludes native/RSS, headers/alignment and class-object footprint".into(),graph_complete:complete,identity:json!({"header":header,"identifierBytes":width,"creationEpochMillis":timestamp,"nativeAssociation":"unverified"}),objects:objects.into_iter().map(|(o,_,_)|o).collect(),gaps})
    }
}

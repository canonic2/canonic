//! Selected native minidump metadata. No memory execution, unwinding or PDB reads.
use super::*;
fn slice(b: &[u8], at: usize, size: usize) -> Result<&[u8]> {
    b.get(at..at.checked_add(size).ok_or_else(invalid)?)
        .ok_or_else(invalid)
}
fn u16_at(b: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(slice(b, at, 2)?.try_into().unwrap()))
}
fn u32_at(b: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(slice(b, at, 4)?.try_into().unwrap()))
}
fn u64_at(b: &[u8], at: usize) -> Result<u64> {
    Ok(u64::from_le_bytes(slice(b, at, 8)?.try_into().unwrap()))
}
fn name(b: &[u8], at: usize) -> Result<String> {
    let n = u32_at(b, at)? as usize;
    if n > 4096 || !n.is_multiple_of(2) {
        return Err(invalid());
    }
    let units = slice(b, at + 4, n)?
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&pair| u16::from_le_bytes(pair))
        .collect::<Vec<_>>();
    String::from_utf16(&units).map_err(|_| invalid())
}
pub(super) fn decode(bytes: &[u8], index: &mut Index) -> Result<()> {
    if slice(bytes, 0, 4)? != b"MDMP" || u32_at(bytes, 4)? & 0xffff != 0xa793 {
        return Err(invalid());
    }
    let count = u32_at(bytes, 8)? as usize;
    let directory = u32_at(bytes, 12)? as usize;
    if count > 64 {
        return Err(invalid());
    }
    slice(bytes, directory, count * 12)?;
    let mut seen = std::collections::BTreeSet::new();
    for n in 0..count {
        let d = directory + n * 12;
        let kind = u32_at(bytes, d)?;
        let size = u32_at(bytes, d + 4)? as usize;
        let rva = u32_at(bytes, d + 8)? as usize;
        let b = slice(bytes, rva, size)?;
        if !seen.insert(kind) {
            return Err(invalid());
        }
        match kind {
            7 => {
                slice(b, 0, 56)?;
                let architecture = match u16_at(b, 0)? {
                    0 => "x86",
                    9 => "x86_64",
                    12 => "arm64",
                    _ => "unknown",
                };
                decode::push(
                    index,
                    decode::row(
                        "process",
                        "Minidump system metadata",
                        format!("/streams/{n}"),
                        json!({"architecture":architecture,"osVersion":[u32_at(b,8)?,u32_at(b,12)?,u32_at(b,16)?],"processors":b[6],"systemMetadataOnly":true}),
                    ),
                )?;
            }
            6 => {
                slice(b, 0, 168)?;
                let thread = u32_at(b, 0)?;
                let mut row = decode::row(
                    if u32_at(b, 8)? == 0 {
                        "event"
                    } else {
                        "failure"
                    },
                    "Minidump native exception",
                    format!("/streams/{n}/exception"),
                    json!({"threadId":thread,"code":format!("0x{:08x}",u32_at(b,8)?),"flags":u32_at(b,12)?,"address":format!("0x{:x}",u64_at(b,24)?),"symbolication":"unresolved; no stack unwind or PDB acquisition","causeEstablished":false}),
                );
                row.correlations.push(format!("native-thread:{thread}"));
                decode::push(index, row)?;
            }
            3 => {
                let count = u32_at(b, 0)? as usize;
                if count > 1024 {
                    return Err(invalid());
                }
                slice(b, 4, count * 48)?;
                for t in 0..count {
                    let at = 4 + t * 48;
                    decode::push(
                        index,
                        decode::row(
                            "thread",
                            "Minidump thread",
                            format!("/streams/{n}/threads/{t}"),
                            json!({"threadId":u32_at(b,at)?,"suspendCount":u32_at(b,at+4)?,"stackStart":format!("0x{:x}",u64_at(b,at+24)?),"nativeContext":"retained raw only; not unwound"}),
                        ),
                    )?;
                }
            }
            4 => {
                decode_modules(bytes, b, n, index)?;
            }
            _ => index
                .unsupported
                .push(format!("minidump_stream_{kind}_retained_raw_only")),
        }
    }
    index.unsupported.push("minidump selected system/thread/module/exception metadata; no memory dereference, stack unwinding, live process identity, complete dump or PDB match established".into());
    Ok(())
}

fn decode_modules(bytes: &[u8], b: &[u8], n: usize, index: &mut Index) -> Result<()> {
    let count = u32_at(b, 0)? as usize;
    if count > 1024 {
        return Err(invalid());
    }
    slice(b, 4, count * 108)?;
    for m in 0..count {
        let at = 4 + m * 108;
        let cv_size = u32_at(b, at + 76)? as usize;
        let cv_at = u32_at(b, at + 80)? as usize;
        let cv = slice(bytes, cv_at, cv_size)?;
        let rsds = if cv.starts_with(b"RSDS") {
            slice(cv, 0, 24)?;
            Some(
                json!({"guidBytes":cv[4..20].iter().map(|n|format!("{n:02x}")).collect::<String>(),"age":u32_at(cv,20)?,"identity":"CodeView byte identity; PDB not read"}),
            )
        } else {
            None
        };
        decode::push(
            index,
            decode::row(
                "process",
                "Minidump module",
                format!("/streams/{n}/modules/{m}"),
                json!({"name":name(bytes,u32_at(b,at+20)? as usize)?,"baseAddress":format!("0x{:x}",u64_at(b,at)?),"imageSize":u32_at(b,at+8)?,"timestamp":u32_at(b,at+16)?,"codeView":rsds,"symbolication":"unresolved"}),
            ),
        )?;
    }
    Ok(())
}

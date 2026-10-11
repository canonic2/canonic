//! Versioned, bounded storage encoding; callers own paths, atomic writes and permissions.

use super::{InputMode, SavedComposer, SavedDraft};
use crate::editor::{AtomKind, Document, InlineAtom};
use std::{
    collections::BTreeMap,
    io::{self, Read, Write},
};

const MAGIC: &[u8] = b"CANONIC-COMPOSER\x02";
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid composer snapshot")
}
fn number(output: &mut impl Write, value: usize) -> io::Result<()> {
    output.write_all(&u64::try_from(value).map_err(|_| invalid())?.to_le_bytes())
}
fn string(output: &mut impl Write, value: &str) -> io::Result<()> {
    number(output, value.len())?;
    output.write_all(value.as_bytes())
}
fn strings(output: &mut impl Write, values: &[String]) -> io::Result<()> {
    number(output, values.len())?;
    for value in values {
        string(output, value)?;
    }
    Ok(())
}
fn atoms(output: &mut impl Write, values: &BTreeMap<char, InlineAtom>) -> io::Result<()> {
    number(output, values.len())?;
    for (marker, atom) in values {
        output.write_all(&u32::from(*marker).to_le_bytes())?;
        string(output, &atom.label)?;
        string(output, &atom.content)?;
        output.write_all(&[match atom.kind {
            AtomKind::Paste => 0,
            AtomKind::Image => 1,
            AtomKind::File => 2,
        }])?;
        output.write_all(&[u8::from(atom.reference.is_some())])?;
        if let Some(reference) = &atom.reference {
            string(output, reference)?;
        }
    }
    Ok(())
}
fn draft(output: &mut impl Write, value: &SavedDraft) -> io::Result<()> {
    string(output, &value.text)?;
    number(output, value.cursor)?;
    output.write_all(&[u8::from(value.mode == InputMode::Shell)])?;
    output.write_all(&value.age_seconds.to_le_bytes())?;
    atoms(output, &value.atoms)
}
fn drafts(output: &mut impl Write, values: &[SavedDraft]) -> io::Result<()> {
    number(output, values.len())?;
    for value in values {
        draft(output, value)?;
    }
    Ok(())
}
fn optional(output: &mut impl Write, value: Option<&SavedDraft>) -> io::Result<()> {
    output.write_all(&[u8::from(value.is_some())])?;
    if let Some(value) = value {
        draft(output, value)?;
    }
    Ok(())
}

struct Decoder<'a, R> {
    input: &'a mut R,
    limit: usize,
    remaining: usize,
    schema: u8,
}
impl<R: Read> Decoder<'_, R> {
    fn flag(&mut self) -> io::Result<bool> {
        let mut bytes = [0];
        self.input.read_exact(&mut bytes)?;
        match bytes[0] {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(invalid()),
        }
    }
    fn integer(&mut self) -> io::Result<u64> {
        let mut bytes = [0; 8];
        self.input.read_exact(&mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }
    fn count(&mut self, max: usize) -> io::Result<usize> {
        let count = usize::try_from(self.integer()?).map_err(|_| invalid())?;
        if count > max {
            return Err(invalid());
        }
        Ok(count)
    }
    fn string(&mut self, max: usize) -> io::Result<String> {
        let len = self.count(max)?;
        self.remaining = self.remaining.checked_sub(len).ok_or_else(invalid)?;
        let mut bytes = vec![0; len];
        self.input.read_exact(&mut bytes)?;
        String::from_utf8(bytes).map_err(|_| invalid())
    }
    fn strings(&mut self) -> io::Result<Vec<String>> {
        let count = self.count(500)?;
        (0..count).map(|_| self.string(self.limit)).collect()
    }
    fn atoms(&mut self) -> io::Result<BTreeMap<char, InlineAtom>> {
        let count = self.count(256)?;
        let mut values = BTreeMap::new();
        for _ in 0..count {
            let mut bytes = [0; 4];
            self.input.read_exact(&mut bytes)?;
            let marker = char::from_u32(u32::from_le_bytes(bytes)).ok_or_else(invalid)?;
            let label = self.string(256)?;
            let content = self.string(self.limit)?;
            let kind = if self.schema == 2 {
                let mut byte = [0];
                self.input.read_exact(&mut byte)?;
                Some(match byte[0] {
                    0 => AtomKind::Paste,
                    1 => AtomKind::Image,
                    2 => AtomKind::File,
                    _ => return Err(invalid()),
                })
            } else {
                None
            };
            let reference = if self.flag()? {
                Some(self.string(4096)?)
            } else {
                None
            };
            if values
                .insert(
                    marker,
                    InlineAtom {
                        kind: kind.unwrap_or(if reference.is_some() {
                            AtomKind::Image
                        } else {
                            AtomKind::Paste
                        }),
                        label,
                        content,
                        reference,
                    },
                )
                .is_some()
            {
                return Err(invalid());
            }
        }
        Ok(values)
    }
    fn draft(&mut self) -> io::Result<SavedDraft> {
        Ok(SavedDraft {
            text: self.string(self.limit)?,
            cursor: self.count(self.limit)?,
            mode: if self.flag()? {
                InputMode::Shell
            } else {
                InputMode::Message
            },
            age_seconds: self.integer()?,
            atoms: self.atoms()?,
        })
    }
    fn drafts(&mut self, max: usize) -> io::Result<Vec<SavedDraft>> {
        let count = self.count(max)?;
        (0..count).map(|_| self.draft()).collect()
    }
    fn optional(&mut self) -> io::Result<Option<SavedDraft>> {
        if self.flag()? {
            self.draft().map(Some)
        } else {
            Ok(None)
        }
    }
    fn documents(&mut self) -> io::Result<BTreeMap<String, Document>> {
        let count = self.count(1000)?;
        let mut values = BTreeMap::new();
        for _ in 0..count {
            let key = self.string(self.limit)?;
            let document = Document {
                text: self.string(self.limit)?,
                cursor: self.count(self.limit)?,
                atoms: self.atoms()?,
            };
            if values.insert(key, document).is_some() {
                return Err(invalid());
            }
        }
        Ok(values)
    }
    fn origins(&mut self) -> io::Result<BTreeMap<String, String>> {
        let count = self.count(1000)?;
        let mut values = BTreeMap::new();
        for _ in 0..count {
            let key = self.string(self.limit)?;
            let value = self.string(64)?;
            if values.insert(key, value).is_some() {
                return Err(invalid());
            }
        }
        Ok(values)
    }
}

impl SavedComposer {
    /// Encode schema version 2 to a caller-owned sink. No paths are opened.
    /// # Errors
    /// Returns write errors or unrepresentable length errors.
    pub fn write_to(&self, output: &mut impl Write) -> io::Result<()> {
        output.write_all(MAGIC)?;
        draft(output, &self.draft)?;
        drafts(output, &self.queue)?;
        strings(output, &self.history)?;
        strings(output, &self.shell_history)?;
        drafts(output, &self.stash)?;
        number(output, self.history_documents.len())?;
        for (key, document) in &self.history_documents {
            string(output, key)?;
            string(output, &document.text)?;
            number(output, document.cursor)?;
            atoms(output, &document.atoms)?;
        }
        number(output, self.history_origins.len())?;
        for (key, origin) in &self.history_origins {
            string(output, key)?;
            string(output, origin)?;
        }
        optional(output, self.pending.as_ref())?;
        optional(output, self.queued_edit.as_ref())?;
        optional(output, self.editor_return.as_ref())
    }
    /// Decode bounded schema 1 or 2, then validate with `Composer::restore_saved`.
    /// The host supplies the same editor limit used by its composer.
    /// # Errors
    /// Rejects unknown schemas, truncated data, invalid UTF-8 and excessive lengths/counts.
    pub fn read_from(input: &mut impl Read, limit: usize) -> io::Result<Self> {
        let mut magic = vec![0; MAGIC.len()];
        input.read_exact(&mut magic)?;
        let schema = magic[MAGIC.len() - 1];
        if magic[..MAGIC.len() - 1] != MAGIC[..MAGIC.len() - 1] || !matches!(schema, 1 | 2) {
            return Err(invalid());
        }
        if limit > 1_048_576 {
            return Err(invalid());
        }
        let mut decoder = Decoder {
            input,
            limit,
            remaining: 268_435_456,
            schema,
        };
        Ok(Self {
            draft: decoder.draft()?,
            queue: decoder.drafts(128)?,
            history: decoder.strings()?,
            shell_history: decoder.strings()?,
            stash: decoder.drafts(50)?,
            history_documents: decoder.documents()?,
            history_origins: decoder.origins()?,
            pending: decoder.optional()?,
            queued_edit: decoder.optional()?,
            editor_return: decoder.optional()?,
        })
    }
}

//! Command-owned file snapshots and prepared codecs, never used for publication checks.
use crate::{Error, Format, Result, document::Prepared, storage};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    path::{Path, PathBuf},
    rc::Rc,
};

type ParseResult = std::result::Result<Rc<Prepared>, String>;

struct Snapshot {
    source: Option<String>,
    parsed: RefCell<Vec<(Format, ParseResult)>>,
}

#[derive(Default)]
pub(crate) struct Inputs {
    files: RefCell<BTreeMap<PathBuf, Rc<Snapshot>>>,
    #[cfg(test)]
    reads: RefCell<BTreeMap<PathBuf, usize>>,
    #[cfg(test)]
    parses: RefCell<BTreeMap<PathBuf, usize>>,
}

impl Inputs {
    fn snapshot(&self, path: &Path) -> Result<Rc<Snapshot>> {
        if let Some(snapshot) = self.files.borrow().get(path) {
            return Ok(Rc::clone(snapshot));
        }
        #[cfg(test)]
        {
            *self.reads.borrow_mut().entry(path.to_owned()).or_default() += 1;
        }
        let snapshot = Rc::new(Snapshot {
            source: storage::read(path)?,
            parsed: RefCell::default(),
        });
        self.files
            .borrow_mut()
            .insert(path.to_owned(), Rc::clone(&snapshot));
        Ok(snapshot)
    }

    pub(crate) fn read(&self, path: &Path) -> Result<Option<String>> {
        Ok(self.snapshot(path)?.source.clone())
    }

    pub(crate) fn document(&self, path: &Path, format: Format) -> Result<Rc<Prepared>> {
        let snapshot = self.snapshot(path)?;
        let mut parsed = snapshot.parsed.borrow_mut();
        if let Some((_, result)) = parsed.iter().find(|(kind, _)| *kind == format) {
            return result.clone().map_err(Error::invalid);
        }
        let source = snapshot.source.clone().unwrap_or_else(|| {
            if format == Format::Toml {
                String::new()
            } else {
                "{}\n".into()
            }
        });
        #[cfg(test)]
        {
            *self.parses.borrow_mut().entry(path.to_owned()).or_default() += 1;
        }
        let result = Prepared::parse(source, format)
            .map(Rc::new)
            .map_err(|e| e.to_string());
        parsed.push((format, result.clone()));
        result.map_err(Error::invalid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preparation_reuses_large_inputs_but_apply_still_refuses_changed_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let project = root.join("project");
        let home = root.join("home");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let context = crate::Context {
            product: "acme".into(),
            program: root.join("acme"),
            arguments: vec!["serve".into()],
            launch_command: None,
            check_arguments: vec!["--check".into()],
            root: project.clone(),
            home,
            state: root.join("state"),
            agent_homes: BTreeMap::new(),
            tools: vec![crate::Tool::new("observe", "Observe", true)],
        };
        let path = project.join(".mcp.json");
        let source = format!("{{\"history\":\"{}\"}}", "x".repeat(1024 * 1024));
        std::fs::write(&path, &source).unwrap();
        let inputs = Inputs::default();
        // Picker inspection and later preparation share the same snapshot.
        inputs
            .document(&path, Format::Json)
            .unwrap()
            .edit("mcpServers", "acme", None)
            .unwrap();
        let options = crate::Options {
            agents: vec!["claude".into(), "copilot-cli".into(), "claude".into()],
            ..Default::default()
        };
        let plan = crate::workflow::prepare_with_inputs(&context, &options, &inputs).unwrap();
        assert!(
            !plan.review()["registrations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(inputs.reads.borrow()[&path], 1);
        assert_eq!(inputs.parses.borrow()[&path], 1);
        let settings = project.join(".claude/settings.local.json");
        assert_eq!(inputs.parses.borrow()[&settings], 1);
        // The cache must never hide changes from publication's fresh reads.
        std::fs::write(&path, "{\"external\":true}").unwrap();
        assert!(matches!(plan.apply(), Err(Error::Conflict(_))));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\"external\":true}"
        );
    }
}

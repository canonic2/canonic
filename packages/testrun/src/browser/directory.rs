//! Bounded directory fixtures assembled only from verified registered files.
use super::{contract::Reference, session::Error};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub path: String,
    pub fixture: Reference,
}
#[derive(Clone)]
pub(crate) struct Directory {
    pub path: PathBuf,
    pub summary: Value,
    pub sources: Vec<Reference>,
    files: BTreeMap<String, (usize, String)>,
}
pub(crate) fn validate(name: &str, entries: &[Entry]) -> Result<(), &'static str> {
    if name.is_empty()
        || name.len() > 128
        || name.contains(['/', '\\', ':', '\0'])
        || matches!(name, "." | "..")
        || entries.is_empty()
        || entries.len() > 16
    {
        return Err("DIRECTORY_BOUNDS");
    }
    let mut paths = std::collections::BTreeSet::new();
    for entry in entries {
        if entry.path.len() > 512
            || entry.path.split('/').count() > 8
            || entry.path.split('/').any(|part| {
                part.is_empty() || matches!(part, "." | "..") || part.contains(['\\', ':', '\0'])
            })
            || !paths.insert(entry.path.clone())
        {
            return Err("INVALID_DIRECTORY_PATH");
        }
    }
    for path in &paths {
        let mut parent = Path::new(path).parent();
        while let Some(value) = parent {
            if paths.contains(value.to_str().ok_or("INVALID_DIRECTORY_PATH")?) {
                return Err("INVALID_DIRECTORY_PATH");
            }
            parent = value.parent();
        }
    }
    Ok(())
}
pub(crate) async fn create(
    root: PathBuf,
    reference: Reference,
    name: String,
    entries: Vec<(Entry, Value, Vec<u8>)>,
) -> Result<Directory, Error> {
    let sources = entries
        .iter()
        .map(|(entry, _, _)| entry.fixture)
        .collect::<Vec<_>>();
    let total = entries
        .iter()
        .map(|(_, _, bytes)| bytes.len())
        .sum::<usize>();
    if total > 512 * 1024 {
        return Err("FIXTURE_BOUNDS".into());
    }
    tokio::task::spawn_blocking(move||{
        let path=root.join(format!("directory-{}",reference.id)).join(&name);
        crate::artifacts::directory(path.parent().ok_or("FIXTURE_IO")?).map_err(|error|Error::caused("FIXTURE_IO",error))?;
        crate::artifacts::directory(&path).map_err(|error|Error::caused("FIXTURE_IO",error))?;
        let mut files=BTreeMap::new();let mut records=Vec::new();
        for (entry,summary,bytes) in entries {
            let output=path.join(&entry.path);let mut relative=PathBuf::new();
            for component in Path::new(&entry.path).parent().ok_or("FIXTURE_IO")?.components(){relative.push(component);let directory=path.join(&relative);if !directory.exists(){crate::artifacts::directory(&directory).map_err(|error|Error::caused("FIXTURE_IO",error))?;}}
            crate::artifacts::write(&output,&bytes).map_err(|error|Error::caused("FIXTURE_IO",error))?;
            files.insert(entry.path.clone(),(bytes.len(),format!("{:x}",Sha256::digest(&bytes))));
            records.push(json!({"path":entry.path,"fixture":summary}));
        }
        let summary=json!({"reference":reference,"kind":"directory","name":name,"files":records,"bytes":total});
        Ok(Directory{path,summary,sources,files})
    }).await.map_err(|_|Error::from("FIXTURE_IO"))?
}
pub(crate) async fn verify(directory: Directory) -> Result<(), Error> {
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        if !std::fs::symlink_metadata(directory.path.parent().ok_or("DIRECTORY_CHANGED")?)
            .is_ok_and(|metadata| metadata.is_dir())
        {
            return Err(Error::from("DIRECTORY_CHANGED"));
        }
        let mut pending = vec![directory.path.clone()];
        let mut actual = BTreeMap::new();
        let mut visited = 0;
        while let Some(path) = pending.pop() {
            visited += 1;
            if visited > 145 {
                return Err(Error::from("DIRECTORY_CHANGED"));
            }
            let metadata = std::fs::symlink_metadata(&path)
                .map_err(|error| Error::caused("FIXTURE_IO", error))?;
            if metadata.is_dir() {
                for entry in
                    std::fs::read_dir(&path).map_err(|error| Error::caused("FIXTURE_IO", error))?
                {
                    if pending.len() + visited >= 145 {
                        return Err(Error::from("DIRECTORY_CHANGED"));
                    }
                    pending.push(
                        entry
                            .map_err(|error| Error::caused("FIXTURE_IO", error))?
                            .path(),
                    );
                }
            } else if metadata.is_file() {
                let relative = path
                    .strip_prefix(&directory.path)
                    .map_err(|_| Error::from("DIRECTORY_CHANGED"))?
                    .to_str()
                    .ok_or("DIRECTORY_CHANGED")?
                    .to_owned();
                let expected = directory.files.get(&relative).ok_or("DIRECTORY_CHANGED")?;
                if metadata.len() != expected.0 as u64 {
                    return Err("DIRECTORY_CHANGED".into());
                }
                let mut bytes = Vec::new();
                std::fs::File::open(&path)
                    .map_err(|error| Error::caused("FIXTURE_IO", error))?
                    .take(super::transfer::FIXTURE_BYTES as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|error| Error::caused("FIXTURE_IO", error))?;
                actual.insert(
                    relative,
                    (bytes.len(), format!("{:x}", Sha256::digest(&bytes))),
                );
            } else {
                return Err("DIRECTORY_CHANGED".into());
            }
        }
        if actual != directory.files {
            return Err("DIRECTORY_CHANGED".into());
        }
        Ok(())
    })
    .await
    .map_err(|_| Error::from("FIXTURE_IO"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn directory_paths_refuse_traversal_duplicates_and_file_directory_collisions() {
        let reference = Reference {
            connection_id: uuid::Uuid::new_v4(),
            id: uuid::Uuid::new_v4(),
            generation: uuid::Uuid::new_v4(),
        };
        for path in [
            "../private",
            "/private",
            "a//b",
            "C:/private",
            "a\\b",
            "a/./b",
        ] {
            assert!(
                validate(
                    "acme",
                    &[Entry {
                        path: path.into(),
                        fixture: reference
                    }]
                )
                .is_err()
            );
        }
        assert!(
            validate(
                "acme",
                &[
                    Entry {
                        path: "a".into(),
                        fixture: reference
                    },
                    Entry {
                        path: "a/b".into(),
                        fixture: reference
                    }
                ]
            )
            .is_err()
        );
        assert!(
            validate(
                "acme",
                &[Entry {
                    path: "assets/input.txt".into(),
                    fixture: reference
                }]
            )
            .is_ok()
        );
    }
    #[tokio::test]
    async fn directory_verification_refuses_modified_and_extra_files() {
        let root = tempfile::tempdir().unwrap();
        let reference = Reference {
            connection_id: uuid::Uuid::new_v4(),
            id: uuid::Uuid::new_v4(),
            generation: uuid::Uuid::new_v4(),
        };
        let directory = create(
            root.path().into(),
            reference,
            "acme".into(),
            vec![(
                Entry {
                    path: "nested/file.txt".into(),
                    fixture: reference,
                },
                json!({}),
                b"Acme".to_vec(),
            )],
        )
        .await
        .unwrap();
        verify(directory.clone()).await.unwrap();
        std::fs::write(directory.path.join("extra.txt"), b"Must refuse").unwrap();
        assert_eq!(
            verify(directory.clone()).await.unwrap_err().code,
            "DIRECTORY_CHANGED"
        );
        std::fs::remove_file(directory.path.join("extra.txt")).unwrap();
        std::fs::write(directory.path.join("nested/file.txt"), b"Evil").unwrap();
        assert_eq!(
            verify(directory).await.unwrap_err().code,
            "DIRECTORY_CHANGED"
        );
    }
}

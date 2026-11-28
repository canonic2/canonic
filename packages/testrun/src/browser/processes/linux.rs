//! Linux procfs observations of an explicitly owned process tree.

use super::Info;
use std::{fs, io};

pub(super) fn info(pid: u32) -> io::Result<Option<Info>> {
    let stat = match fs::read(format!("/proc/{pid}/stat")) {
        Ok(stat) => stat,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    parse_stat(&stat)
        .map(Some)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid procfs process stat"))
}

fn parse_stat(stat: &[u8]) -> Option<Info> {
    // The command can contain spaces, parentheses and non-UTF-8 bytes.
    let end = stat.iter().rposition(|byte| *byte == b')')?;
    let mut fields = std::str::from_utf8(&stat[end + 1..])
        .ok()?
        .split_whitespace();
    let state = fields.next()?;
    let parent = fields.next()?.parse().ok()?;
    // The start time is field 22. State and parent are fields 3 and 4.
    let start = fields.nth(17)?.parse().ok()?;
    Some(Info {
        parent,
        start,
        zombie: matches!(state, "Z" | "X"),
    })
}

pub(super) fn children(pid: u32) -> io::Result<Vec<u32>> {
    children_at(std::path::Path::new(&format!("/proc/{pid}/task")))
}

fn children_at(directory: &std::path::Path) -> io::Result<Vec<u32>> {
    const MAX_CHILDREN: usize = 1024;
    const MAX_THREADS: usize = 4096;
    let tasks = match fs::read_dir(directory) {
        Ok(tasks) => tasks,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut children = Vec::new();
    for (index, task) in tasks.enumerate() {
        if index == MAX_THREADS {
            return Err(io::Error::other("Owned thread table exceeds capacity"));
        }
        let task = task?.path();
        let listed = match fs::read_to_string(task.join("children")) {
            Ok(listed) => listed,
            Err(error) if error.kind() == io::ErrorKind::NotFound && !task.try_exists()? => {
                continue;
            }
            Err(error) => return Err(error),
        };
        for child in listed.split_whitespace() {
            if children.len() == MAX_CHILDREN {
                return Err(io::Error::other("Owned child table exceeds capacity"));
            }
            children.push(child.parse().map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "Invalid procfs child PID")
            })?);
        }
    }
    Ok(children)
}

#[cfg(test)]
mod tests {
    use super::{children_at, fs, parse_stat};

    #[test]
    fn proc_children_distinguishes_missing_thread_from_missing_inventory() {
        let root = tempfile::tempdir().unwrap();
        assert!(children_at(&root.path().join("absent")).unwrap().is_empty());
        let task = root.path().join("42");
        fs::create_dir(&task).unwrap();
        assert!(children_at(root.path()).is_err());
        fs::write(task.join("children"), "43 44").unwrap();
        assert_eq!(children_at(root.path()).unwrap(), [43, 44]);
    }

    #[test]
    fn proc_stat_tracks_birth_after_parenthesized_command() {
        let stat = format!("80 (worker ) with spaces) Z 14 {}1234", "0 ".repeat(17));
        let info = parse_stat(stat.as_bytes()).unwrap();
        assert_eq!((info.parent, info.start, info.zombie), (14, 1234, true));
        assert!(parse_stat(b"80 (worker) R 14").is_none());
    }
}

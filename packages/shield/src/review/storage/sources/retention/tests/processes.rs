//! Real non-activating checkpoint publication races a separate collector process.
use super::*;
use std::{
    io::{BufRead as _, Read as _},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

struct Owner {
    child: Child,
    reader: Option<thread::JoinHandle<()>>,
}
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            reader.join().unwrap();
        }
    }
}

#[test]
fn storage_reclamation_nonactivating_writer_and_collector_in_separate_processes() {
    let (root, _context) = fixture();
    let store = Store::open(root.path()).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap()).args(["--exact","review::storage::sources::retention::tests::processes::storage_reclamation_writer_child","--ignored","--nocapture"])
        .env("SHIELD_SOURCE_WRITER_ROOT",root.path()).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sent, received) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if let Some(message) = line.strip_prefix("SHIELD_SOURCE:") {
                let _ = sent.send(message.to_owned());
            }
        }
    });
    let mut owner = Owner {
        child,
        reader: Some(reader),
    };
    for round in 0..4 {
        assert_eq!(
            received.recv_timeout(Duration::from_secs(5)).unwrap(),
            "ready"
        );
        for index in 0..130 {
            let path = store
                .directory
                .join("sources")
                .join(format!("{:064x}.json", round * 1000 + index));
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            crate::review::storage::private(&mut options);
            options
                .open(path)
                .unwrap()
                .write_all(b"Acme unreachable fixture")
                .unwrap();
        }
        owner.child.stdin.as_mut().unwrap().write_all(b"x").unwrap();
        let mut collector = Batch::new(&Value::Null, false).unwrap();
        collector.admit(&store, &"f".repeat(64), 1).unwrap();
        drop(collector);
        let id = received.recv_timeout(Duration::from_secs(5)).unwrap();
        let state = store.read_checkpoint().unwrap();
        let proposal = state
            .proposals
            .iter()
            .find(|proposal| proposal.id == id)
            .unwrap();
        assert!(store.captured_candidate(&proposal.candidate).is_ok());
    }
    assert!(owner.child.wait().unwrap().success());
}

#[test]
#[ignore = "owned non-activating writer process"]
fn storage_reclamation_writer_child() {
    let root = std::env::var_os("SHIELD_SOURCE_WRITER_ROOT").unwrap();
    let context = crate::review::open(Path::new(&root)).unwrap();
    for round in 0..4 {
        println!("SHIELD_SOURCE:ready");
        std::io::stdout().flush().unwrap();
        std::io::stdin().read_exact(&mut [0]).unwrap();
        let proposal = crate::review::tests::propose(
            &context,
            &crate::review::tests::input(&format!("acme-writer-{round}")),
            &|| Ok(()),
        )
        .unwrap();
        println!(
            "SHIELD_SOURCE:{}",
            proposal["proposal_id"].as_str().unwrap()
        );
        std::io::stdout().flush().unwrap();
    }
}

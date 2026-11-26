//! Actual process death releases the execution lease, without replaying preparation.
use super::*;
use std::time::Duration;
use std::{
    io::{BufRead as _, Read as _, Write as _},
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
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
fn preparation_tasks_process_death_recovers_the_durable_handle_without_reexecution() {
    let (root, context) = crate::review::tests::enrolled();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "review::preparation_tasks::execution::process_tests::preparation_tasks_owner_child",
            "--ignored",
            "--nocapture",
        ])
        .env("SHIELD_TASK_OWNER_ROOT", root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (sent, received) = mpsc::sync_channel(1);
    let reader = thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            let line = line.unwrap();
            if let Some(id) = line.strip_prefix("SHIELD_TASK_READY:") {
                sent.send(id.to_owned()).unwrap();
                break;
            }
        }
    });
    let mut owner = Owner {
        child,
        reader: Some(reader),
    };
    let id = received.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        Runtime::new(&context)
            .store()
            .unwrap()
            .get(&id)
            .unwrap()
            .status,
        "working"
    );
    owner.child.kill().unwrap();
    owner.child.wait().unwrap();
    drop(owner);
    let reopened = Runtime::new(&context);
    assert_eq!(reopened.store().unwrap().get(&id).unwrap().status, "failed");
    let state = storage::Store::open(root.path())
        .unwrap()
        .read_checkpoint()
        .unwrap();
    assert!(state.proposals.is_empty());
}

#[test]
#[ignore = "owned helper for the process-death fixture"]
fn preparation_tasks_owner_child() {
    let root = std::env::var_os("SHIELD_TASK_OWNER_ROOT").expect("owned task fixture root");
    let context = crate::review::open(Path::new(&root)).unwrap();
    let runtime = Runtime::new(&context);
    let (record, _owner) = runtime
        .store()
        .unwrap()
        .reserve(&super::tests::input("acme-process"), &|| Ok(()))
        .unwrap()
        .unwrap();
    println!("SHIELD_TASK_READY:{}", record.id);
    std::io::stdout().flush().unwrap();
    std::io::stdin().read_to_end(&mut Vec::new()).unwrap();
}

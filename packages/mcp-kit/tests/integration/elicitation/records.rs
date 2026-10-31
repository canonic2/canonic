//! A consumer-owned file record fixture; validates persistence separately from protocol input.
use super::{Host, Script, text};
use canonic_mcp_kit::{
    ToolResult,
    stdio::{Options, Outcome, Request},
    task::{Snapshot, State, TaskRequest, TaskResult, Timestamp},
};
use serde_json::Value;
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};

pub(super) fn save(path: &Path, snapshot: &Snapshot) {
    let next = path.with_extension("next");
    let mut file = File::create(&next).expect("create private next record");
    file.write_all(&serde_json::to_vec(snapshot).expect("encode snapshot"))
        .expect("write record");
    rustix::fs::fsync(&file).expect("flush record");
    fs::rename(&next, path).expect("replace record");
    rustix::fs::fsync(File::open(path.parent().expect("record parent")).expect("open parent"))
        .expect("flush parent");
}

pub(super) fn open(path: &Path) -> Snapshot {
    let bytes = fs::read(path).expect("read retained record");
    Snapshot::restore(&serde_json::from_slice::<Value>(&bytes).expect("record JSON"))
        .expect("validate retained questions")
}

pub(super) fn start(path: PathBuf, options: Options) -> Host {
    let retained = Mutex::new(open(&path));
    let now = Timestamp::parse("2026-10-31T12:00:00Z").expect("fixture clock");
    Host::start(
        Script::new(move |request, context| {
            let mut snapshot = retained.lock().expect("record owner");
            let Request::Task(request) = request else {
                return text("No creation in this fixture");
            };
            match &request {
                TaskRequest::Get { .. } => {
                    Outcome::Task(Ok(TaskResult::Snapshot(snapshot.clone())))
                }
                TaskRequest::Update { .. } => match snapshot.input_answers(
                    &request,
                    context.request_context().expect("modern context"),
                    &now,
                ) {
                    Ok(answers) => {
                        if let Some(answer) = answers.get("report") {
                            let content = answer.content.clone().expect("accepted form data");
                            let done = Snapshot::new(
                                snapshot.metadata().clone(),
                                State::Completed(ToolResult::structured(content, false)),
                            )
                            .expect("terminal record");
                            snapshot
                                .validate_successor(&done, &now)
                                .expect("valid settlement");
                            save(&path, &done);
                            *snapshot = done;
                        }
                        Outcome::Task(Ok(TaskResult::Updated))
                    }
                    Err(error) => Outcome::Task(Err(error)),
                },
                TaskRequest::Cancel { .. } => Outcome::Task(Ok(TaskResult::Cancelled)),
            }
        }),
        options,
    )
}

//! A fixed per-checkout recovery owner, independent of policy/grant authority.
use super::{Error, storage};
use std::{fs, path::Path};

pub(super) struct Lease {
    directory: storage::objects::Directory,
}
impl Lease {
    pub(super) fn acquire_cancellable(
        root: &str,
        checkpoint: super::super::Checkpoint<'_>,
    ) -> Result<Self, Error> {
        loop {
            checkpoint()?;
            match Self::acquire(root) {
                Err(Error::Locked) => {}
                result => return result,
            }
        }
    }

    pub(super) fn acquire(root: &str) -> Result<Self, Error> {
        let review = Path::new(root).join(".shield/review");
        storage::private_files::directory(&review, false)?;
        let path = review.join("cache-preparation");
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if !metadata.is_dir() || metadata.file_type().is_symlink() {
                    return Err(Error::Invalid(
                        "cache preparation namespace cannot be a link or file",
                    ));
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::MetadataExt;
                    if metadata.uid() != fs::metadata(&review)?.uid() {
                        return Err(Error::Invalid("cache preparation namespace owner differs"));
                    }
                }
                storage::private_directory(&path)?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                storage::private_files::directory(&path, true)?;
            }
            Err(error) => return Err(error.into()),
        }
        storage::private_files::directory(&path, false)?;
        let directory = storage::objects::Directory::enter(&path)?;
        if fs::metadata(path.join(".objects.lock"))?.len() != 0
            || fs::read_dir(&path)?.take(2).any(|entry| match entry {
                Ok(entry) => entry.file_name() != ".objects.lock",
                Err(_) => true,
            })
        {
            return Err(Error::Invalid(
                "cache preparation namespace contains unexpected data",
            ));
        }
        Ok(Self { directory })
    }
    pub(super) fn check(&self) -> Result<(), Error> {
        self.directory.check_owner()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::storage::Store;
    struct Owner(std::process::Child, Option<std::thread::JoinHandle<()>>);
    impl Drop for Owner {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
            if let Some(reader) = self.1.take() {
                reader.join().unwrap();
            }
        }
    }
    #[test]
    fn recovery_waits_past_the_admission_window_and_observes_cancellation() {
        let (_root, context) = super::super::tests::fixture();
        let lease = Lease::acquire(&context.project_root).unwrap();
        std::thread::scope(|scope| {
            scope.spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(750));
                drop(lease);
            });
            super::super::prepare_committed(&context, &|| Ok(())).unwrap();
        });
        let _lease = Lease::acquire(&context.project_root).unwrap();
        let calls = std::cell::Cell::new(0);
        assert!(matches!(
            Lease::acquire_cancellable(&context.project_root, &|| {
                calls.set(calls.get() + 1);
                if calls.get() > 1 {
                    Err(crate::policy::Error::Interrupted)
                } else {
                    Ok(())
                }
            }),
            Err(Error::Policy(crate::policy::Error::Interrupted))
        ));
    }

    #[test]
    fn recovery_owner_is_bounded_per_project_and_never_holds_authority() {
        let (root, context, evidence, resource) = crate::review::read_approval::tests::fixture();
        let (other, other_context) = super::super::tests::fixture();
        let lease = Lease::acquire(&context.project_root).unwrap();
        assert!(matches!(
            Lease::acquire(&context.project_root),
            Err(Error::Locked)
        ));
        let other_lease = Lease::acquire(&other_context.project_root).unwrap();
        let decision = crate::review::read_approval::gate(
            &context,
            "acme-preparation",
            &evidence,
            &resource,
            None,
            || true,
        )
        .unwrap();
        assert_eq!(decision["code"], "approval-required");
        lease.check().unwrap();
        assert_eq!(
            fs::read_dir(root.path().join(".shield/review/cache-preparation"))
                .unwrap()
                .count(),
            1
        );
        drop(lease);
        Lease::acquire(&context.project_root)
            .unwrap()
            .check()
            .unwrap();
        other_lease.check().unwrap();
        drop(other);
    }
    #[cfg(unix)]
    #[test]
    fn recovery_owner_refuses_links_and_detects_namespace_replacement() {
        let (root, context) = super::super::tests::fixture();
        let lease = Lease::acquire(&context.project_root).unwrap();
        let path = root.path().join(".shield/review/cache-preparation");
        let retained = path.with_file_name("retained-preparation");
        fs::rename(&path, &retained).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(lease.check().is_err());
        fs::remove_dir(&path).unwrap();
        std::os::unix::fs::symlink(&retained, &path).unwrap();
        assert!(Lease::acquire(&context.project_root).is_err());
        assert!(retained.join(".objects.lock").is_file());
    }
    #[test]
    fn already_published_exact_plan_skips_hydration_and_compilation_but_keeps_maintenance() {
        use clap::Parser as _;
        let (root, context) = super::super::tests::fixture();
        let log_root = assert_fs::TempDir::new().unwrap();
        let cli = crate::cli::Cli::parse_from([
            "shield",
            "--debug-log-dir",
            log_root.path().to_str().unwrap(),
            "status",
        ]);
        let log = std::sync::Arc::new(crate::diagnostics::Log::open(&cli));
        let trace = log.request("command", "cache_prepare", None, None).unwrap();
        let before = Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .active
            .unwrap();
        {
            let _scope = trace.enter();
            super::super::prepare(&context, None).unwrap();
            assert_eq!(trace.recorded_count("policy.preparations"), 0);
            assert_eq!(trace.recorded_count("source.hydrated"), 0);
        }
        trace.finish("success");
        let after = Store::open(root.path())
            .unwrap()
            .read_checkpoint()
            .unwrap()
            .active
            .unwrap();
        assert_eq!(before.activation_id(), after.activation_id());
        assert_eq!(before.compiled.digest, after.compiled.digest);
    }
    #[test]
    fn changed_activation_after_recovery_admission_refuses_stale_work_before_hydration() {
        let (root, context) = super::super::tests::fixture();
        let state = Store::open(root.path()).unwrap().read_checkpoint().unwrap();
        let before = state.active.unwrap().activation_id().to_owned();
        let calls = std::cell::Cell::new(0);
        let checkpoint = || {
            let next = calls.get() + 1;
            calls.set(next);
            if next == 2 {
                crate::review::runtime::publish_fixture(
                    &context,
                    &state.policy_revision,
                    &storage::sources::map_digest(&state.candidates),
                    Some(&before),
                    &|| Ok(()),
                )
                .expect("competing activation");
            }
            Ok(())
        };
        assert!(matches!(
            super::super::prepare_committed(&context, &checkpoint),
            Err(Error::StaleSnapshot)
        ));
        assert_ne!(
            Store::open(root.path())
                .unwrap()
                .read_checkpoint()
                .unwrap()
                .active
                .unwrap()
                .activation_id(),
            before
        );
    }
    #[test]
    fn process_exit_releases_recovery_owner_without_lease_cleanup() {
        use std::{
            io::{BufRead as _, Write as _},
            process::{Command, Stdio},
            sync::mpsc,
            time::Duration,
        };
        let (_root, context) = super::super::tests::fixture();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "review::cache::preparation::tests::recovery_preparation_owner_child",
                "--ignored",
                "--nocapture",
            ])
            .env("SHIELD_CACHE_PREPARATION_ROOT", &context.project_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sent, received) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines() {
                if line.unwrap().contains("SHIELD_CACHE_PREPARATION:ready") {
                    let _ = sent.send(());
                }
            }
        });
        let mut owner = Owner(child, Some(reader));
        received.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(
            Lease::acquire(&context.project_root),
            Err(Error::Locked)
        ));
        owner.0.stdin.as_mut().unwrap().write_all(b"x").unwrap();
        assert!(owner.0.wait().unwrap().success());
        Lease::acquire(&context.project_root)
            .unwrap()
            .check()
            .unwrap();
    }
    #[test]
    #[ignore = "owned preparation lease process"]
    fn recovery_preparation_owner_child() {
        use std::io::{Read as _, Write as _};
        let root = std::env::var("SHIELD_CACHE_PREPARATION_ROOT").unwrap();
        let _lease = Lease::acquire(&root).unwrap();
        println!("SHIELD_CACHE_PREPARATION:ready");
        std::io::stdout().flush().unwrap();
        std::io::stdin().read_exact(&mut [0]).unwrap();
        // Deliberately skips Rust destructors, as a killed compiler does.
        std::process::exit(0);
    }
}

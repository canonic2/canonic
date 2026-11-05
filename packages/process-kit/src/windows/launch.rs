//! Detached launch with an explicit inherited-handle list: only stdin and
//! the caller-opened log, never unrelated pipes, leases or sockets.

use super::check;
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::ExitStatusExt,
    },
    process::{Command, ExitStatus},
    ptr,
};
use windows_sys::Win32::{
    Foundation::{DUPLICATE_SAME_ACCESS, DuplicateHandle, HANDLE, WAIT_OBJECT_0},
    Globalization::CompareStringOrdinal,
    System::{
        JobObjects::{
            CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectExtendedLimitInformation, SetInformationJobObject,
        },
        Threading::{
            CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_SUSPENDED,
            CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DETACHED_PROCESS,
            DeleteProcThreadAttributeList, EXTENDED_STARTUPINFO_PRESENT, GetCurrentProcess,
            GetExitCodeProcess, INFINITE, InitializeProcThreadAttributeList,
            LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
            PROC_THREAD_ATTRIBUTE_JOB_LIST, PROC_THREAD_ATTRIBUTE_PARENT_PROCESS,
            PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess,
            UpdateProcThreadAttribute, WaitForSingleObject,
        },
    },
};

pub(crate) struct Child(OwnedHandle);

impl Child {
    pub(crate) fn wait(&mut self) -> io::Result<ExitStatus> {
        // SAFETY: this guard owns a live process handle until the wait and query end.
        if unsafe { WaitForSingleObject(self.0.as_raw_handle(), INFINITE) } != WAIT_OBJECT_0 {
            return Err(io::Error::last_os_error());
        }
        let mut code = 0;
        // SAFETY: the process handle and output pointer are valid.
        check(unsafe { GetExitCodeProcess(self.0.as_raw_handle(), &raw mut code) })?;
        Ok(ExitStatus::from_raw(code))
    }
}

/// A never-resumed process owns temporary inheritable handles. Keeping them
/// outside the caller prevents unrelated `Command::spawn` calls inheriting them.
struct Parent {
    process: OwnedHandle,
    _job: OwnedHandle,
}

impl Parent {
    fn new(program: &[u16]) -> io::Result<Self> {
        let job = helper_job()?;
        let job_handle = job.as_raw_handle();
        let attributes = Attributes::job(&job_handle)?;
        let startup = STARTUPINFOEXW {
            StartupInfo: windows_sys::Win32::System::Threading::STARTUPINFOW {
                cb: u32::try_from(size_of::<STARTUPINFOEXW>())
                    .map_err(|_| io::ErrorKind::InvalidInput)?,
                ..Default::default()
            },
            lpAttributeList: attributes.as_ptr(),
        };
        let mut info = PROCESS_INFORMATION::default();
        // SAFETY: program is terminated; outputs are live. No handles are
        // inherited and the initial thread never executes application code.
        check(unsafe {
            CreateProcessW(
                program.as_ptr(),
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                0,
                DETACHED_PROCESS | CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT,
                ptr::null(),
                ptr::null(),
                &raw const startup.StartupInfo,
                &raw mut info,
            )
        })?;
        // SAFETY: successful creation transfers these two owned handles.
        let process = unsafe { OwnedHandle::from_raw_handle(info.hProcess) };
        drop(unsafe { OwnedHandle::from_raw_handle(info.hThread) });
        drop(attributes);
        Ok(Self { process, _job: job })
    }

    fn duplicate(&self, file: &File) -> io::Result<HANDLE> {
        let mut handle = ptr::null_mut();
        // SAFETY: the file and parent remain open. The resulting handle belongs
        // to the suspended parent's handle table, not this process's table.
        check(unsafe {
            let process = GetCurrentProcess();
            DuplicateHandle(
                process,
                file.as_raw_handle(),
                self.process.as_raw_handle(),
                &raw mut handle,
                0,
                1,
                DUPLICATE_SAME_ACCESS,
            )
        })?;
        Ok(handle)
    }
}

impl Drop for Parent {
    fn drop(&mut self) {
        // SAFETY: we own this never-resumed helper; termination closes all
        // remote duplicates, including on preparation or launch errors.
        unsafe {
            TerminateProcess(self.process.as_raw_handle(), 0);
            WaitForSingleObject(self.process.as_raw_handle(), INFINITE);
        }
    }
}

fn helper_job() -> io::Result<OwnedHandle> {
    // SAFETY: unnamed job with default security, returned handle owned here.
    let raw = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
    if raw.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful CreateJobObjectW returned an owned handle.
    let job = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    // The helper dies if the client dies, but its manager leaves this private
    // job automatically. Any outer caller job still enforces its own limits.
    limits.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
    // SAFETY: live job and correctly sized initialized limits structure.
    check(unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            ptr::from_ref(&limits).cast(),
            u32::try_from(size_of_val(&limits)).map_err(|_| io::ErrorKind::InvalidInput)?,
        )
    })?;
    Ok(job)
}

struct Attributes<'a> {
    storage: Vec<usize>,
    _handles: std::marker::PhantomData<(&'a [HANDLE], &'a HANDLE)>,
}

impl<'a> Attributes<'a> {
    fn allocate(count: u32) -> io::Result<Self> {
        let mut needed = 0;
        // SAFETY: this first call only obtains the required allocation size.
        unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), count, 0, &raw mut needed) };
        if needed == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut storage = vec![0usize; needed.div_ceil(size_of::<usize>())];
        // SAFETY: storage is pointer-aligned and writable for needed bytes.
        check(unsafe {
            InitializeProcThreadAttributeList(
                storage.as_mut_ptr().cast(),
                count,
                0,
                &raw mut needed,
            )
        })?;
        Ok(Self {
            storage,
            _handles: std::marker::PhantomData,
        })
    }

    fn job(job: &'a HANDLE) -> io::Result<Self> {
        let attributes = Self::allocate(1)?;
        // SAFETY: the caller retains the one job handle through creation.
        check(unsafe {
            UpdateProcThreadAttribute(
                attributes.as_ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                ptr::from_ref(job).cast(),
                size_of::<HANDLE>(),
                ptr::null_mut(),
                ptr::null(),
            )
        })?;
        Ok(attributes)
    }

    fn new(handles: &'a [HANDLE], parent: &'a HANDLE) -> io::Result<Self> {
        let attributes = Self::allocate(2)?;
        // SAFETY: the caller retains the handle slice and its owned handles
        // through CreateProcessW; this attribute contains exactly that slice.
        check(unsafe {
            UpdateProcThreadAttribute(
                attributes.as_ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_ptr().cast(),
                size_of_val(handles),
                ptr::null_mut(),
                ptr::null(),
            )
        })?;
        // SAFETY: parent is an owned process with PROCESS_CREATE_PROCESS
        // access and remains alive through the attribute list's lifetime.
        check(unsafe {
            UpdateProcThreadAttribute(
                attributes.as_ptr(),
                0,
                PROC_THREAD_ATTRIBUTE_PARENT_PROCESS as usize,
                ptr::from_ref(parent).cast(),
                size_of::<HANDLE>(),
                ptr::null_mut(),
                ptr::null(),
            )
        })?;
        Ok(attributes)
    }

    fn as_ptr(&self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_ptr().cast_mut().cast()
    }
}

impl Drop for Attributes<'_> {
    fn drop(&mut self) {
        // SAFETY: initialization succeeded and the allocation remains alive.
        unsafe { DeleteProcThreadAttributeList(self.as_ptr()) };
    }
}

fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut units: Vec<_> = value.encode_wide().collect();
    if units.contains(&0) {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    units.push(0);
    Ok(units)
}

/// Current directories need ordinary Win32 paths. Remove a verbatim prefix
/// only when Win32 normalization leaves the resulting path exactly unchanged.
fn directory(value: &OsStr) -> io::Result<Vec<u16>> {
    let original = wide(value)?;
    let drive_prefix: Vec<_> = r"\\?\".encode_utf16().collect();
    let unc_prefix: Vec<_> = r"\\?\UNC\".encode_utf16().collect();
    let candidate: Vec<u16> = if let Some(rest) = original.strip_prefix(unc_prefix.as_slice()) {
        r"\\".encode_utf16().chain(rest.iter().copied()).collect()
    } else if let Some(rest) = original.strip_prefix(drive_prefix.as_slice()) {
        rest.to_vec()
    } else {
        return Ok(original);
    };
    let path = OsString::from_wide(&candidate[..candidate.len() - 1]);
    if std::path::absolute(&path).is_ok_and(|absolute| absolute.as_os_str() == path.as_os_str()) {
        Ok(candidate)
    } else {
        Ok(original)
    }
}

/// Quote one CRT argument, including empty strings, quotes and trailing slashes.
fn argument(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut units = wide(value)?;
    units.pop();
    let mut out = vec![u16::from(b'"')];
    let mut slashes = 0;
    for unit in units {
        if unit == u16::from(b'\\') {
            slashes += 1;
            continue;
        }
        let quoted = unit == u16::from(b'"');
        out.extend(std::iter::repeat_n(
            u16::from(b'\\'),
            if quoted { slashes * 2 + 1 } else { slashes },
        ));
        out.push(unit);
        slashes = 0;
    }
    out.extend(std::iter::repeat_n(u16::from(b'\\'), slashes * 2));
    out.push(u16::from(b'"'));
    Ok(out)
}

fn environment(command: &Command) -> io::Result<Vec<u16>> {
    let mut entries = Vec::new();
    for (name, value) in command.get_envs() {
        let Some(value) = value else { continue };
        let name = wide(name)?;
        if name.len() == 1 || name.contains(&u16::from(b'=')) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        entries.push((name, wide(value)?));
    }
    entries.sort_by(|(left, _), (right, _)| {
        // SAFETY: both slices are live, NUL-terminated UTF-16 strings; -1
        // selects their terminators. Ordinal comparison is locale-independent.
        unsafe { CompareStringOrdinal(left.as_ptr(), -1, right.as_ptr(), -1, 1) }.cmp(&2)
    });
    let mut out = Vec::new();
    for (name, value) in entries {
        out.extend_from_slice(&name[..name.len() - 1]);
        out.push(u16::from(b'='));
        out.extend(value);
    }
    if out.is_empty() {
        out.push(0);
    }
    out.push(0);
    Ok(out)
}

/// Re-execute the prepared command without inheriting its caller's pipes.
pub(crate) fn spawn_detached(command: &Command, log: &File) -> io::Result<Child> {
    spawn_prepared(command, log, || {})
}

fn spawn_prepared(command: &Command, log: &File, prepared: impl FnOnce()) -> io::Result<Child> {
    let program = wide(command.get_program())?;
    let mut line = argument(command.get_program())?;
    for arg in command.get_args() {
        line.push(u16::from(b' '));
        line.extend(argument(arg)?);
    }
    line.push(0);
    let environment = environment(command)?;
    let directory = command
        .get_current_dir()
        .map(|dir| directory(dir.as_os_str()))
        .transpose()?;
    let parent = Parent::new(&program)?;
    let handles = [
        parent.duplicate(&File::open(r"\\.\NUL")?)?,
        parent.duplicate(log)?,
    ];
    let parent_handle = parent.process.as_raw_handle();
    let attributes = Attributes::new(&handles, &parent_handle)?;
    prepared();
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb =
        u32::try_from(size_of::<STARTUPINFOEXW>()).map_err(|_| io::ErrorKind::InvalidInput)?;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = handles[0];
    startup.StartupInfo.hStdOutput = handles[1];
    startup.StartupInfo.hStdError = handles[1];
    startup.lpAttributeList = attributes.as_ptr();
    let flags = DETACHED_PROCESS
        | CREATE_NEW_PROCESS_GROUP
        | CREATE_UNICODE_ENVIRONMENT
        | EXTENDED_STARTUPINFO_PRESENT;
    for breakaway in [CREATE_BREAKAWAY_FROM_JOB, 0] {
        let mut mutable_line = line.clone();
        let mut info = PROCESS_INFORMATION::default();
        // SAFETY: all strings, the environment block, attribute storage and
        // inherited handles stay alive through this call. Only listed handles
        // can be inherited. Success transfers the two returned handles here.
        let created = unsafe {
            CreateProcessW(
                program.as_ptr(),
                mutable_line.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                1,
                flags | breakaway,
                environment.as_ptr().cast(),
                directory.as_ref().map_or(ptr::null(), Vec::as_ptr),
                &raw const startup.StartupInfo,
                &raw mut info,
            )
        };
        if created != 0 {
            // SAFETY: CreateProcessW succeeded and returned two owned handles.
            let (process, thread) = unsafe {
                (
                    OwnedHandle::from_raw_handle(info.hProcess),
                    OwnedHandle::from_raw_handle(info.hThread),
                )
            };
            drop(thread);
            return Ok(Child(process));
        }
        let error = io::Error::last_os_error();
        if breakaway == 0 || error.raw_os_error() != Some(5) {
            return Err(error);
        }
    }
    unreachable!("both launch attempts return on success or refusal")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "child process for the detached launch regressions"]
    fn launch_child() {
        if let Some(expected) = std::env::var_os("ACME_EXPECTED_DIRECTORY") {
            assert_eq!(
                std::env::current_dir().unwrap().canonicalize().unwrap(),
                std::path::PathBuf::from(expected).canonicalize().unwrap()
            );
            std::fs::write("acme.cwd", b"expected working directory").unwrap();
        }
        if std::env::var_os("ACME_LAUNCH_HOLD").is_some() {
            use std::io::Read;
            let mut byte = [0];
            std::io::stdin().read_exact(&mut byte).unwrap();
        }
        println!("acme log output");
        eprintln!("acme log error");
    }

    #[test]
    fn canonical_directory_is_used_by_the_detached_child() {
        let temporary = tempfile::tempdir().unwrap();
        let selected = temporary.path().join("Acme working directory é😀");
        std::fs::create_dir(&selected).unwrap();
        let canonical = selected.canonicalize().unwrap();
        assert!(
            canonical
                .as_os_str()
                .as_encoded_bytes()
                .starts_with(br"\\?\")
        );
        let spawn = crate::launch::Spawn {
            args: [
                "--exact",
                "windows::launch::tests::launch_child",
                "--ignored",
                "--nocapture",
            ]
            .map(OsString::from)
            .to_vec(),
            environment: vec![(
                "ACME_EXPECTED_DIRECTORY".into(),
                canonical.clone().into_os_string(),
            )],
            log: File::create(temporary.path().join("acme.log")).unwrap(),
            directory: canonical,
        };
        assert!(
            spawn
                .start_reaped()
                .unwrap()
                .join()
                .unwrap()
                .unwrap()
                .success()
        );
        assert_eq!(
            std::fs::read(selected.join("acme.cwd")).unwrap(),
            b"expected working directory"
        );
    }

    #[test]
    fn directory_prefix_changes_preserve_win32_resolution() {
        for (input, expected) in [
            (r"\\?\C:\Acme\project", r"C:\Acme\project"),
            (r"\\?\UNC\server\share\Acme", r"\\server\share\Acme"),
            (r"C:\Acme\project", r"C:\Acme\project"),
            (r"\\?\C:\Acme\..\project", r"\\?\C:\Acme\..\project"),
            (r"\\?\C:\Acme\project.", r"\\?\C:\Acme\project."),
            (r"\\?\relative", r"\\?\relative"),
            (r"\\?\Volume{acme}\project", r"\\?\Volume{acme}\project"),
        ] {
            assert_eq!(
                directory(OsStr::new(input)).unwrap(),
                wide(OsStr::new(expected)).unwrap(),
                "{input}"
            );
        }
        assert!(directory(OsStr::new("bad\0directory")).is_err());
    }

    #[test]
    fn concurrent_ordinary_child_does_not_retain_the_manager_log() {
        use std::{os::windows::fs::OpenOptionsExt, process::Stdio};
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("acme.log");
        let log = File::create(&path).unwrap();
        let executable = std::env::current_exe().unwrap();
        let mut command = Command::new(&executable);
        command
            .args([
                "--exact",
                "windows::launch::tests::launch_child",
                "--ignored",
                "--nocapture",
            ])
            .env_clear();
        let mut ordinary = None;
        let manager = spawn_prepared(&command, &log, || {
            // Launch on another thread while every temporary inheritable
            // duplicate exists; no timing-dependent stress loop is needed.
            ordinary = Some(
                std::thread::spawn(move || {
                    Command::new(executable)
                        .args([
                            "--exact",
                            "windows::launch::tests::launch_child",
                            "--ignored",
                            "--nocapture",
                        ])
                        .env("ACME_LAUNCH_HOLD", "1")
                        .stdin(Stdio::piped())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .unwrap()
                })
                .join()
                .unwrap(),
            );
        });
        let status = manager.and_then(|mut child| child.wait());
        drop(log);
        // An exclusive open must succeed while the unrelated child still
        // waits on stdin. Any inherited log handle would deny this open.
        let exclusive = File::options().read(true).share_mode(0).open(&path);
        let mut ordinary = ordinary.unwrap();
        let alive = ordinary.try_wait().unwrap().is_none();
        let killed = ordinary.kill();
        let reaped = ordinary.wait();
        killed.unwrap();
        reaped.unwrap();
        assert!(alive);
        assert!(status.unwrap().success());
        drop(exclusive.unwrap());
        let text = std::fs::read_to_string(path).unwrap();
        assert!(text.contains("acme log output"));
        assert!(text.contains("acme log error"));
    }

    #[test]
    fn failed_manager_creation_releases_helper_log_handles() {
        use std::os::windows::fs::OpenOptionsExt;
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("acme.log");
        let log = File::create(&path).unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .current_dir(temporary.path().join("absent"))
            .env_clear();
        let result = spawn_detached(&command, &log);
        drop(log);
        assert!(result.is_err());
        File::options().read(true).share_mode(0).open(path).unwrap();
    }

    #[test]
    fn quoted_arguments_round_trip_through_the_windows_parser() {
        use windows_sys::Win32::{Foundation::LocalFree, UI::Shell::CommandLineToArgvW};
        let args = [
            "acme.exe",
            "",
            "a b",
            "a\"b",
            "ends\\",
            "slashes\\\\\"quote",
            "é😀",
        ];
        let mut line = Vec::new();
        for (index, arg) in args.iter().enumerate() {
            if index != 0 {
                line.push(u16::from(b' '));
            }
            line.extend(argument(OsStr::new(arg)).unwrap());
        }
        line.push(0);
        let mut count = 0;
        // SAFETY: line is NUL-terminated and count is a live output pointer.
        let parsed = unsafe { CommandLineToArgvW(line.as_ptr(), &raw mut count) };
        assert!(!parsed.is_null());
        let mut actual = Vec::new();
        for index in 0..usize::try_from(count).unwrap() {
            // SAFETY: successful parsing returned count NUL-terminated strings.
            let arg = unsafe { *parsed.add(index) };
            let mut length = 0;
            // SAFETY: each returned argument has a readable NUL terminator.
            while unsafe { *arg.add(length) } != 0 {
                length += 1;
            }
            // SAFETY: this slice ends at that terminator, within the allocation.
            actual.push(std::ffi::OsString::from_wide(unsafe {
                std::slice::from_raw_parts(arg, length)
            }));
        }
        // SAFETY: CommandLineToArgvW's one allocation is released after copying.
        unsafe { LocalFree(parsed.cast()) };
        assert_eq!(actual, args.map(std::ffi::OsString::from));
        assert_eq!(
            argument(OsStr::new("bad\0argument")).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn environment_is_explicit_sorted_and_nul_terminated() {
        let mut command = Command::new("acme.exe");
        command
            .env_clear()
            .env("z", "é")
            .env("Alpha", "old")
            .env("ALPHA", "last");
        let block = environment(&command).unwrap();
        let text = String::from_utf16(&block).unwrap();
        assert_eq!(text.to_lowercase(), "alpha=last\0z=é\0\0");
        for (name, value) in [
            ("", "value"),
            ("bad=name", "value"),
            ("bad\0name", "value"),
            ("name", "bad\0value"),
        ] {
            let mut command = Command::new("acme.exe");
            command.env_clear().env(name, value);
            assert_eq!(
                environment(&command).unwrap_err().kind(),
                io::ErrorKind::InvalidInput,
                "name={name:?}, value={value:?}"
            );
        }
    }
}

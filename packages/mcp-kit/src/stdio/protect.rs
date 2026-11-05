//! Keeps a server's protocol streams to itself.
//!
//! On Unix, [`protect`] moves the protocol to private, close-on-exec copies of
//! descriptors 0 and 1, then points descriptor 0 at `/dev/null` and descriptor
//! 1 at stderr: code that prints, and children that inherit the standard
//! streams, can neither consume protocol input nor corrupt protocol output,
//! and prompts find no terminal. On Windows it returns copies of the standard
//! handles and leaves them in place, so a stray print there still reaches the
//! protocol.

use std::{fs::File, io};

/// The protocol's input and output, for [`serve`](super::serve).
#[derive(Debug)]
pub struct Protocol {
    /// Where frames arrive.
    pub input: File,
    /// Where frames go.
    pub output: File,
}

/// Takes the standard streams for the protocol. Call it once, before anything
/// else reads standard input or writes standard output.
///
/// # Errors
/// Returns the operating system's error when a stream cannot be copied or, on
/// Unix, redirected.
#[cfg(unix)]
pub fn protect() -> io::Result<Protocol> {
    use std::os::fd::AsFd;
    // `try_clone_to_owned` duplicates with close-on-exec set.
    let input = io::stdin().as_fd().try_clone_to_owned()?;
    let output = io::stdout().as_fd().try_clone_to_owned()?;
    let null = File::open("/dev/null")?;
    rustix::stdio::dup2_stdin(&null)?;
    rustix::stdio::dup2_stdout(io::stderr())?;
    Ok(Protocol {
        input: File::from(input),
        output: File::from(output),
    })
}

/// Takes the standard streams for the protocol. Call it once, before anything
/// else reads standard input or writes standard output.
///
/// # Errors
/// Returns the operating system's error when a stream cannot be copied.
#[cfg(windows)]
pub fn protect() -> io::Result<Protocol> {
    use std::os::windows::io::AsHandle;
    // Copies are not inherited by children.
    let input = io::stdin().as_handle().try_clone_to_owned()?;
    let output = io::stdout().as_handle().try_clone_to_owned()?;
    Ok(Protocol {
        input: File::from(input),
        output: File::from(output),
    })
}

#[cfg(test)]
mod tests {
    use super::protect;
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};

    const CHILD: &str = "ACME_PROTECT_CHILD";

    /// Run as a child of [`stray_output_never_reaches_the_protocol`]: echoes the
    /// protocol's input to its output after printing to standard output.
    #[test]
    fn child() {
        if std::env::var_os(CHILD).is_none() {
            return;
        }
        let mut protocol = protect().expect("the streams");
        println!("stray print");
        let mut input = String::new();
        protocol.input.read_to_string(&mut input).expect("input");
        // On Unix this is `/dev/null`; on Windows the protocol has read it all.
        let mut stdin = String::new();
        std::io::stdin()
            .read_to_string(&mut stdin)
            .expect("standard input");
        write!(protocol.output, "echo:{input}:{}", stdin.len()).expect("output");
    }

    #[test]
    fn stray_output_never_reaches_the_protocol() {
        let mut child = Command::new(std::env::current_exe().expect("this test binary"))
            .args(["--exact", "stdio::protect::tests::child", "--nocapture"])
            .env(CHILD, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the child");
        child
            .stdin
            .take()
            .expect("its input")
            .write_all(b"frame")
            .expect("a frame");
        let output = child.wait_with_output().expect("the child's output");
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        // The child's own input is the protocol's, and standard input reads empty.
        assert!(stdout.contains("echo:frame:0"), "{stdout}");
        if cfg!(unix) {
            assert!(!stdout.contains("stray print"), "{stdout}");
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("stray print"),
                "{output:?}"
            );
        }
    }
}

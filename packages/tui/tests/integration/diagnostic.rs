//! Permanent command feedback is safe, complete, bounded and independent of terminal state.
use canonic_tui::{
    ratatui::{Terminal, backend::TestBackend},
    status_panels::{Diagnostic, Severity},
    theme::{ColorMode, Presentation},
};
use std::io;
use unicode_width::UnicodeWidthStr;

fn fixture<'a>(context: &'a [(&'a str, &'a str)]) -> Diagnostic<'a> {
    Diagnostic {
        severity: Severity::Error,
        title: "Configuration needs review",
        message: "Acme could not use the selected file.",
        context,
        help: &["Run acme setup after review."],
    }
}
#[test]
fn permanent_monochrome_output_preserves_context_recovery_and_safe_unicode() {
    let context = [
        ("File", "café/👩‍💻.toml"),
        ("Details", "bad\u{1b}[2J\u{202e} value"),
    ];
    let diagnostic = fixture(&context);
    let p = Presentation {
        colors: ColorMode::NoColor,
        ..Presentation::default()
    };
    let mut output = Vec::new();
    diagnostic.write(&mut output, 32, p).expect("memory writer");
    let output = String::from_utf8(output).expect("UTF8 feedback");
    assert!(output.contains("File: café/👩‍💻.toml"));
    assert!(output.contains("Details:"));
    assert!(output.contains("Next"));
    assert!(output.contains("acme setup"));
    assert!(!output.contains('\u{1b}'));
    assert!(!output.contains('\u{202e}'));
    assert!(output.lines().all(|line| line.width() <= 32));
    for width in [1, 2, 8] {
        assert!(
            diagnostic
                .lines(width, p)
                .iter()
                .all(|line| line.width() <= usize::from(width))
        );
    }
    let mut terminal = Terminal::new(TestBackend::new(32, 30)).expect("backend");
    terminal
        .draw(|f| diagnostic.render(f, f.area(), p))
        .expect("render");
    let screen = super::components::dump(terminal.backend().buffer());
    assert!(screen.contains("Details:"));
    assert!(screen.contains("Next"));
    insta::assert_snapshot!("command_diagnostic_monochrome", screen);
}
#[test]
fn long_fields_are_marked_and_empty_tiny_diagnostics_remain_safe() {
    let huge = "a".repeat(70_000);
    let context = [("Path", huge.as_str())];
    let diagnostic = fixture(&context);
    let mut bytes = vec![];
    diagnostic
        .write(
            &mut bytes,
            80,
            Presentation {
                colors: ColorMode::NoColor,
                ..Presentation::default()
            },
        )
        .expect("writer");
    assert!(
        String::from_utf8(bytes)
            .expect("text")
            .contains("[truncated]")
    );
    let empty = Diagnostic {
        severity: Severity::Info,
        title: "Cancelled",
        message: "No pending change confirmed.",
        context: &[],
        help: &[],
    };
    for (width, height) in [(0, 0), (1, 1), (12, 3)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("backend");
        terminal
            .draw(|f| empty.render(f, f.area(), Presentation::default()))
            .expect("safe clipping");
    }
    let mut bytes = vec![];
    empty
        .write(
            &mut bytes,
            80,
            Presentation {
                colors: ColorMode::NoColor,
                ..Presentation::default()
            },
        )
        .expect("writer");
    let output = String::from_utf8(bytes).expect("text");
    assert!(output.starts_with("Info: Cancelled"));
    assert!(!output.contains("Next"));
}
#[test]
fn styled_output_restores_styles_and_reports_writer_failure() {
    struct Failing;
    impl io::Write for Failing {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "fixture failure"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let diagnostic = fixture(&[]);
    let mut output = vec![];
    diagnostic
        .write(&mut output, 80, Presentation::default())
        .expect("writer");
    assert!(output.windows(2).any(|v| v == b"\x1b["));
    assert!(output.ends_with(b"\x1b[0m\n"));
    assert_eq!(
        diagnostic
            .write(&mut Failing, 80, Presentation::default())
            .expect_err("failed writer")
            .kind(),
        io::ErrorKind::BrokenPipe
    );
}

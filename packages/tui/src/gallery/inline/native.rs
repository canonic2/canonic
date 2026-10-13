//! Owns the explicit native inline gallery loop, plain fallback and foreground release demonstration.

mod plain;
use super::{InlineDemo, NAMES};
use crate::{
    event::{Event, KeyCode, KeyEventKind, KeyModifiers},
    runtime::{Capabilities, InlineOptions, InlineSession},
    theme::Presentation,
};
use anyhow::Result;
use std::{
    io::{self, IsTerminal},
    time::{Duration, Instant},
};

pub(crate) fn run(scenario: Option<&str>, p: Presentation) -> Result<()> {
    let name = scenario.unwrap_or("main");
    let index = NAMES
        .iter()
        .position(|n| *n == name)
        .ok_or_else(|| anyhow::anyhow!("unknown inline scenario: {name}"))?;
    let mut demo = InlineDemo::new(index);
    for line in &demo.printed {
        println!("{line}");
    }
    let term = std::env::var("TERM").unwrap_or_default();
    let capable = io::stdin().is_terminal()
        && io::stdout().is_terminal()
        && term != "dumb"
        && !term.is_empty()
        && !demo.plain();
    let options = InlineOptions {
        height: demo.height(),
        capabilities: Capabilities {
            cursor: capable,
            alternate: capable && !demo.no_alternate(),
        },
    };
    let mut session = match InlineSession::open(options) {
        Ok(s) => s,
        Err(e) => {
            println!("inline unavailable: {e}");
            return plain::run(&demo);
        }
    };
    // These records were printed before acquisition; only future host lines are emitted through the lease.
    demo.take_output();
    if let Some(result) = demo.result.take() {
        session.finish(&result)?;
        return Ok(());
    }
    let mut last = Instant::now();
    loop {
        if let Err(error) = session.draw(|f| demo.render_live(f, f.area(), p)) {
            let cleanup = session.finish("inline → couldn't draw · nothing confirmed");
            return Err(anyhow::anyhow!("{error}; cleanup: {cleanup:?}"));
        }
        if let Some(event) = session.poll(Duration::from_millis(20))? {
            if matches!(&event,Event::Key(k) if k.kind==KeyEventKind::Press && k.code==KeyCode::F(11) && k.modifiers==KeyModifiers::NONE)
            {
                let paused = session.pause()?;
                println!("foreground host output · terminal released");
                session = paused.resume()?;
                last = Instant::now();
                continue;
            }
            if matches!(&event,Event::Key(k) if k.kind==KeyEventKind::Press && k.code==KeyCode::Char('l') && k.modifiers==KeyModifiers::CONTROL)
            {
                // The fixed viewport is reconstructed only through a verified owned-row redraw.
                session.redraw()?;
                continue;
            }
            if !session.confirmation_available()
                && matches!(&event, Event::Key(k) if k.code == KeyCode::Enter || matches!(k.code, KeyCode::Char('0'..='9')))
            {
                continue;
            }
            if matches!(&event,Event::Key(k) if k.kind==KeyEventKind::Press && k.code==KeyCode::F(4) && k.modifiers==KeyModifiers::NONE)
            {
                demo.open_inspection();
            } else {
                demo.update(&event);
            }
            for text in demo.take_output() {
                session.print_before(&text)?;
            }
            if demo.alternate() && !session.alternate() {
                if let Err(e) = session.enter_alternate() {
                    demo.overlay = None;
                    demo.notice = e.to_string();
                }
            } else if !demo.alternate() && session.alternate() {
                session.leave_alternate()?;
            }
            if let Some(result) = demo.result.take() {
                session.finish(&result)?;
                println!("host output after inline result");
                return Ok(());
            }
        }
        let elapsed = last.elapsed();
        demo.tick(elapsed);
        last = Instant::now();
    }
}

//! One active foreground operation owns cooperative Windows console cancellation.
use std::{
    io,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU8, Ordering},
    },
};

const IDLE: u8 = 0;
const ACTIVE: u8 = 1;
const CANCELLED: u8 = 2;

fn registration() -> io::Result<Arc<AtomicU8>> {
    static REGISTRATION: OnceLock<Result<Arc<AtomicU8>, String>> = OnceLock::new();
    REGISTRATION
        .get_or_init(|| {
            let state = Arc::new(AtomicU8::new(IDLE));
            let handler = state.clone();
            ctrlc::try_set_handler(move || {
                // Ctrl-C and Ctrl-Break cancel the current owner. Further events
                // during its cleanup do not bypass terminal restoration. Outside
                // a foreground operation the CLI remains interruptible.
                match handler.compare_exchange(
                    ACTIVE,
                    CANCELLED,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                ) {
                    Ok(_) | Err(CANCELLED) => {}
                    Err(_) => std::process::exit(130),
                }
            })
            .map_err(|error| error.to_string())?;
            Ok(state)
        })
        .as_ref()
        .cloned()
        .map_err(|error| io::Error::other(error.clone()))
}

/// A process-wide lease, released after the operation restores owned resources.
pub(crate) struct Signals {
    state: Arc<AtomicU8>,
}

impl Signals {
    pub(crate) fn new() -> io::Result<Self> {
        let state = registration()?;
        state
            .compare_exchange(IDLE, ACTIVE, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| io::Error::other("Windows console cancellation already has an owner"))?;
        Ok(Self { state })
    }

    pub(crate) fn cancelled(&self) -> bool {
        self.state.load(Ordering::SeqCst) == CANCELLED
    }
}

impl Drop for Signals {
    fn drop(&mut self) {
        self.state.store(IDLE, Ordering::SeqCst);
    }
}

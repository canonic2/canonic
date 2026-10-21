//! Opt-in request phase timing. Task-local ownership keeps concurrent requests
//! isolated without passing diagnostic state through domain APIs.
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

tokio::task_local! {
    pub(crate) static REQUEST: Timings;
}

#[derive(Debug)]
pub(crate) enum Mode {
    Disabled,
    Enabled,
}

impl Mode {
    pub(crate) fn enabled(&self) -> bool {
        matches!(self, Self::Enabled)
    }
}

#[derive(Clone, Default)]
pub(crate) struct Timings(Arc<Mutex<BTreeMap<&'static str, f64>>>);

impl Timings {
    pub(crate) fn values(&self) -> BTreeMap<&'static str, f64> {
        let mut values = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        // Highlighting is nested inside Markdown rendering; report exclusive work.
        let highlighting = values.get("highlight").copied().unwrap_or_default();
        if let Some(markdown) = values.get_mut("markdown") {
            *markdown = (*markdown - highlighting).max(0.0);
        }
        values
    }
}

pub(crate) struct Phase(Option<(Timings, &'static str, Instant)>);

/// No clock read or allocation when no traced request owns this task.
pub(crate) fn phase(name: &'static str) -> Phase {
    Phase(
        REQUEST
            .try_with(|timings| (timings.clone(), name, Instant::now()))
            .ok(),
    )
}

impl Drop for Phase {
    fn drop(&mut self) {
        if let Some((timings, name, started)) = &self.0 {
            *timings
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .entry(name)
                .or_default() += started.elapsed().as_secs_f64() * 1000.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn phases_are_owned_by_the_request_and_accumulate() {
        let timings = Timings::default();
        REQUEST
            .scope(timings.clone(), async {
                {
                    let _phase = phase("highlight");
                }
                {
                    let _phase = phase("highlight");
                }
                let other = tokio::spawn(async {
                    let _phase = phase("unrelated");
                });
                other.await.unwrap();
            })
            .await;
        assert!(timings.values()["highlight"] >= 0.0);
        assert!(!timings.values().contains_key("unrelated"));
    }
}

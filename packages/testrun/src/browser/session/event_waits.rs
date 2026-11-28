//! Metadata-only page event history and cancellable waits outside the mutation lane.
use super::*;
use crate::browser::activity::{Condition, Event};

impl Host {
    pub(super) fn record_activity(
        &self,
        page: Reference,
        event: Event,
        resource: Reference,
    ) -> Result<(), Error> {
        let state = self.state.lock().expect("Browser owner");
        let context = state
            .retained_ancestor(page, Kind::Context)
            .ok_or("STALE_REFERENCE")?;
        self.activity
            .lock()
            .expect("Browser event history")
            .push(context, page, event, resource);
        self.changed.notify_waiters();
        Ok(())
    }

    pub(super) async fn events_read(
        &self,
        context: Reference,
        cursor: Option<String>,
        limit: usize,
        deadline: Instant,
    ) -> Result<Value, Error> {
        self.connection()?.event_barrier(deadline).await?;
        {
            let state = self.state.lock().expect("Browser owner");
            if state.phase != Phase::Ready {
                return Err("SESSION_NOT_READY".into());
            }
            state.resource(context, Some(Kind::Context))?;
        }
        self.activity
            .lock()
            .expect("Browser event history")
            .page(context, cursor.as_deref(), limit)
            .map_err(Error::from)
    }

    pub(super) async fn events_wait(
        &self,
        id: Uuid,
        context: Reference,
        after: u64,
        conditions: Vec<Condition>,
        deadline: Instant,
    ) -> Result<Value, Error> {
        let mut cursor = after;
        let result = async {
            self.connection()?.event_barrier(deadline).await?;
            loop {
                let changed = self.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if Instant::now() >= deadline { return Err(Error::from("WAIT_EXPIRED")); }
                self.admissible(id, deadline)?;
                {
                    let state = self.state.lock().expect("Browser owner");
                    if state.phase != Phase::Ready { return Err("SESSION_NOT_READY".into()); }
                    state.resource(context, Some(Kind::Context))?;
                    for condition in &conditions {
                        if let Some(page) = condition.page {
                            match state.resource(page, Some(Kind::Page)) {
                                Ok(_) if state.ancestor(page, Kind::Context) == Some(context) => {},
                                Ok(_) => return Err("FOREIGN_PAGE".into()),
                                Err(error) => {
                                    // A close event can arrive before its wait is
                                    // submitted. Exact retained witnesses still
                                    // authorize filtering, never new mutations.
                                    if !self.activity.lock().expect("Browser event history").knows_page(context, page) { return Err(Error::from(error)); }
                                }
                            }
                        }
                    }
                }
                let read = self.activity.lock().expect("Browser event history").wait(context, after, cursor, &conditions)?;
                if read["matched"] == true { return Ok(read); }
                cursor = read["nextCursor"].as_u64().expect("Event cursor");
                if let Some(operation) = self.state.lock().expect("Browser owner").operations.get_mut(&id) {
                    operation.result = Some(read);
                }
                tokio::select! {
                    () = &mut changed => {},
                    () = tokio::time::sleep_until(deadline) => return Err(Error::from("WAIT_EXPIRED")),
                }
            }
        }.await;
        result
            .map_err(|error| crate::browser::activity::wait_failure(context, after, cursor, error))
    }
}

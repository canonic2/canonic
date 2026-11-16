//! Admission-time ownership for the opt-in process adapter. Ordinary drivers
//! retain the zero-allocation in-process path.
use super::{Context, Handler, Options, Outcome, Request, ServeError};
use crate::Catalog;

#[derive(Clone, Default)]
pub(crate) struct Binding {
    #[cfg(all(feature = "worker-process", any(unix, windows)))]
    reservation: Option<std::sync::Arc<crate::worker::Reservation>>,
    #[cfg(all(feature = "worker-process", any(unix, windows)))]
    remote: bool,
}
impl Binding {
    #[cfg(all(feature = "worker-process", any(unix, windows)))]
    pub(crate) fn remote() -> Self {
        Self {
            remote: true,
            reservation: None,
        }
    }
    pub(crate) fn retain(&self, outcome: Outcome) -> Outcome {
        #[cfg(all(feature = "worker-process", any(unix, windows)))]
        if self.reservation.is_some() {
            return match outcome {
                Outcome::Confirm(confirm) => {
                    let retained = Retained {
                        callback: confirm.resume,
                        binding: self.clone(),
                    };
                    Outcome::Confirm(super::Confirm::new(
                        confirm.message,
                        move |decision, context| retained.confirm(decision, context),
                    ))
                }
                Outcome::Elicit(step) => {
                    let retained = Retained {
                        callback: step.resume,
                        binding: self.clone(),
                    };
                    Outcome::Elicit(super::Elicit::new(step.question, move |answer, context| {
                        retained.elicit(answer, context)
                    }))
                }
                outcome => outcome,
            };
        }
        outcome
    }
    pub(crate) fn call(
        &self,
        handler: &impl Handler,
        request: Request,
        context: &Context,
    ) -> Outcome {
        #[cfg(all(feature = "worker-process", any(unix, windows)))]
        if let Some(reservation) = &self.reservation {
            return reservation.call(request, context);
        }
        handler.call(request, context)
    }
    #[cfg(all(feature = "worker-process", any(unix, windows)))]
    pub(crate) fn hold(&self) -> Result<Option<crate::worker::Hold>, crate::worker::Error> {
        if self.remote {
            // A remote process cannot create a frontend reservation locally.
            // Consumers need an explicit resource protocol before resources escape.
            return Err(crate::worker::Error::Contract);
        }
        self.reservation
            .as_ref()
            .map(|reservation| reservation.hold())
            .transpose()
    }
}

// Drop the proxy callback before releasing the original process reservation.
// The scheduler's Meta may be dropped first on cancellation/EOF/late outcomes.
#[cfg(all(feature = "worker-process", any(unix, windows)))]
struct Retained<F> {
    callback: F,
    binding: Binding,
}
#[cfg(all(feature = "worker-process", any(unix, windows)))]
impl Retained<super::Resume> {
    fn confirm(self, decision: crate::Decision, context: &Context) -> Outcome {
        let Self { callback, binding } = self;
        let outcome = binding
            .reservation
            .as_ref()
            .expect("retained reservation")
            .resume(|| callback(decision, context));
        drop(binding);
        outcome
    }
}
#[cfg(all(feature = "worker-process", any(unix, windows)))]
impl Retained<super::ElicitResume> {
    fn elicit(self, answer: crate::elicitation::Answer, context: &Context) -> Outcome {
        let Self { callback, binding } = self;
        let outcome = binding
            .reservation
            .as_ref()
            .expect("retained reservation")
            .resume(|| callback(answer, context));
        drop(binding);
        outcome
    }
}

pub(crate) struct Execution {
    #[cfg(all(feature = "worker-process", any(unix, windows)))]
    frontend: Option<std::sync::Arc<crate::worker::Frontend>>,
    #[cfg(all(feature = "worker-process", any(unix, windows)))]
    _attachment: Option<crate::worker::Attachment>,
}
impl Execution {
    pub(crate) fn new(options: &Options, catalog: Option<&Catalog>) -> Result<Self, ServeError> {
        #[cfg(all(feature = "worker-process", any(unix, windows)))]
        {
            let attachment = options
                .worker
                .as_ref()
                .map(|frontend| {
                    frontend
                        .attach(
                            catalog.ok_or(ServeError::Options("missing process worker catalog"))?,
                            options,
                        )
                        .map_err(ServeError::Worker)
                })
                .transpose()?;
            Ok(Self {
                frontend: options.worker.clone(),
                _attachment: attachment,
            })
        }
        #[cfg(not(all(feature = "worker-process", any(unix, windows))))]
        {
            let _ = (options, catalog);
            Ok(Self {})
        }
    }
    pub(crate) fn reserve(&self, request: &Request) -> Result<Binding, &'static str> {
        #[cfg(all(feature = "worker-process", any(unix, windows)))]
        if let Some(frontend) = &self.frontend {
            if matches!(request, Request::Resource(_)) {
                return Ok(Binding::default());
            }
            let reservation = frontend
                .reserve(request)
                .map_err(|error| match error {
                    crate::worker::Error::Busy
                    | crate::worker::Error::Process(canonic_process_kit::Error::Capacity) =>
                        "Server request capacity reached",
                    crate::worker::Error::Process(canonic_process_kit::Error::Draining) =>
                        "Worker admission is paused or closed",
                    crate::worker::Error::Contract => "Cannot encode process assignment",
                    crate::worker::Error::Process(_) => "Worker generation is unavailable; reconcile cleanup, then replace the worker or restart the frontend",
                })?;
            return Ok(Binding {
                reservation: Some(reservation),
                remote: false,
            });
        }
        let _ = request;
        Ok(Binding::default())
    }
    pub(crate) fn close(&self) {
        #[cfg(all(feature = "worker-process", any(unix, windows)))]
        if let Some(frontend) = &self.frontend {
            frontend.close();
        }
    }
    pub(crate) fn publish_next(
        &self,
        receiver: &super::ChangeReceiver,
        session: &mut crate::Session,
    ) -> Option<Result<Vec<serde_json::Value>, crate::ConfigurationError>> {
        #[cfg(all(feature = "worker-process", any(unix, windows)))]
        if let Some(frontend) = &self.frontend {
            return frontend.publish_next(receiver, session);
        }
        receiver
            .try_recv()
            .ok()
            .map(|pending| session.publish(pending.change))
    }
    pub(crate) fn finish(&self) -> Result<(), ServeError> {
        #[cfg(all(feature = "worker-process", any(unix, windows)))]
        if let Some(frontend) = &self.frontend {
            frontend.finish()?;
        }
        Ok(())
    }
}

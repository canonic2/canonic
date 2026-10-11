//! Owns modal stack generations, scoped focus and dismissal; bodies stay caller-owned.

mod confirmation;
mod timing;
mod view;
pub use confirmation::{Caution, Confirmation, Decision};
pub use timing::{Arrival, Attention};
pub use view::DialogView;

use crate::{
    event::{Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind},
    focus::Focus,
};
use ratatui::layout::Rect;
use std::{fmt, sync::Arc};

/// Opaque instance identity. Clones refer to the same opening, never a replacement.
#[derive(Clone, Debug)]
pub struct Ticket(Arc<()>);
impl PartialEq for Ticket {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for Ticket {}

/// Preferred panel width and compatibility-render height, clamped to the viewport.
#[derive(Clone, Copy, Debug)]
pub struct Size {
    /// Preferred cell width.
    pub width: u16,
    /// Preferred row height for `Stack::render`; `render_with` measures content.
    pub height: u16,
}
impl Default for Size {
    fn default() -> Self {
        Self {
            width: crate::theme::tokens::spacing::DIALOG_WIDTH,
            height: crate::theme::tokens::spacing::DIALOG_HEIGHT,
        }
    }
}

/// Explicit dismissal policy. Neither behavior confirms a body action.
#[derive(Clone, Copy, Debug)]
pub struct ClosePolicy {
    /// Escape closes after the body declines to consume it.
    pub escape: bool,
    /// An outside left-button press dismisses; defaults off.
    pub outside_click: bool,
}
impl Default for ClosePolicy {
    fn default() -> Self {
        Self {
            escape: true,
            outside_click: false,
        }
    }
}

/// Caller-supplied body and its own focus order; contains no terminal session.
pub struct Dialog<B, T> {
    /// Stable host name, unique among open layers.
    pub id: String,
    /// Safe rendered heading.
    pub title: String,
    /// Enabled visible body targets, in Tab order.
    pub targets: Vec<T>,
    /// State retained while a child overlays this layer.
    pub body: B,
    /// Preferred geometry.
    pub size: Size,
    /// Dismissal policy.
    pub close: ClosePolicy,
    /// Caller-owned presentation labels.
    pub view: DialogView,
}

struct Layer<B> {
    id: String,
    title: String,
    body: B,
    ticket: Ticket,
    size: Size,
    close: ClosePolicy,
    view: DialogView,
    rendered_height: Option<u16>,
}

/// Why a layer ended. Hosts cancel outstanding work when they receive this record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// User cancelled.
    Cancelled,
    /// Host accepted a body result.
    Submitted,
    /// Host replaced the top layer.
    Replaced,
    /// Host cleared the stack.
    Cleared,
    /// Opt-in outside press.
    Outside,
}

/// Ownership returns to the caller on every explicit dismissal.
pub struct Dismissed<B> {
    /// Host name.
    pub id: String,
    /// Closed opening, invalid for future results.
    pub ticket: Ticket,
    /// Reason for closure.
    pub reason: Reason,
    /// Body state for caller cleanup or recovery.
    pub body: B,
}

/// Validation error; failed operations preserve the old stack and focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invalid {
    /// Dialog name already open.
    DuplicateId,
    /// Focus order repeats a target.
    DuplicateTarget,
    /// Sixteen layers are already open.
    DepthLimit,
    /// Replacement requires an open layer.
    Empty,
    /// Sixteen asynchronous arrivals are already waiting.
    QueueLimit,
}
impl fmt::Display for Invalid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid dialog operation: {self:?}")
    }
}
impl std::error::Error for Invalid {}

/// Modal dispatch never leaks unhandled input to the background.
pub enum Dispatch<B> {
    /// No dialog; caller may route to its base view.
    Background,
    /// Active scope consumed the event.
    Consumed,
    /// Exactly one layer closed.
    Dismissed(Dismissed<B>),
}

/// Stack and its base focus order. One instance owns every scope it opens.
pub struct Stack<B, T> {
    layers: Vec<Layer<B>>,
    focus: Focus<T>,
    timing: Option<timing::Timing<B, T>>,
}
impl<B, T: Clone + Eq> Stack<B, T> {
    /// Create the base focus order.
    /// # Errors
    /// Rejects duplicate targets.
    pub fn new(targets: Vec<T>) -> Result<Self, Invalid> {
        Ok(Self {
            layers: Vec::new(),
            focus: Focus::new(targets).ok_or(Invalid::DuplicateTarget)?,
            timing: None,
        })
    }
    /// Current target; only the top scope is active.
    #[must_use]
    pub fn focused(&self) -> Option<&T> {
        self.focus.active()
    }
    /// Advance within the current scope.
    pub fn advance(&mut self, reverse: bool) {
        self.focus.advance(reverse);
    }
    /// Repair active and saved return focus after a host destroys a target.
    pub fn remove_target(&mut self, target: &T) {
        self.focus.remove(target);
    }
    /// Open layer count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.layers.len()
    }
    /// Whether background input is available.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }
    #[cfg(feature = "gallery")]
    pub(crate) fn panel_area(&self, viewport: Rect) -> Option<Rect> {
        self.layers
            .last()
            .map(|layer| view::outer(viewport, layer.geometry()))
    }
    /// Current opening identity; clone it when starting host work.
    #[must_use]
    pub fn ticket(&self) -> Option<&Ticket> {
        self.layers.last().map(|l| &l.ticket)
    }
    /// Active dismissal policy, for host activity hints.
    #[must_use]
    pub fn close_policy(&self) -> Option<ClosePolicy> {
        self.layers.last().map(|l| l.close)
    }
    /// True only for the active opening of this stack. Closed/replaced/foreign tickets fail.
    #[must_use]
    pub fn accepts(&self, ticket: &Ticket) -> bool {
        self.ticket() == Some(ticket)
    }
    /// Apply host data only to the active opening; stale/obscured results are rejected.
    pub fn body_mut(&mut self, ticket: &Ticket) -> Option<&mut B> {
        self.layers
            .last_mut()
            .filter(|l| &l.ticket == ticket)
            .map(|l| &mut l.body)
    }
    /// Read only the active opening; obscured/stale/foreign tickets are inert.
    pub fn body(&self, ticket: &Ticket) -> Option<&B> {
        self.layers
            .last()
            .filter(|layer| &layer.ticket == ticket)
            .map(|layer| &layer.body)
    }
    fn validate(&self, dialog: &Dialog<B, T>, replace: bool) -> Result<(), Invalid> {
        if replace && self.is_empty() {
            return Err(Invalid::Empty);
        }
        if !replace && self.len() >= 16 {
            return Err(Invalid::DepthLimit);
        }
        let count = self.len().saturating_sub(usize::from(replace));
        if self.layers[..count].iter().any(|l| l.id == dialog.id) {
            return Err(Invalid::DuplicateId);
        }
        if Focus::new(dialog.targets.clone()).is_none() {
            return Err(Invalid::DuplicateTarget);
        }
        Ok(())
    }
    fn insert(&mut self, dialog: Dialog<B, T>) -> Ticket {
        let ticket = Ticket(Arc::new(()));
        let pushed = self.focus.push(dialog.targets);
        debug_assert!(pushed, "validated focus scope");
        self.layers.push(Layer {
            id: dialog.id,
            title: crate::text::clean(&dialog.title, 128, false),
            body: dialog.body,
            ticket: ticket.clone(),
            size: dialog.size,
            close: dialog.close,
            view: dialog.view.sanitized(),
            rendered_height: None,
        });
        self.opened(&ticket);
        ticket
    }
    /// Open a layer without changing parent body state.
    /// # Errors
    /// Rejects duplicate IDs/targets and depth above sixteen, without mutation.
    pub fn push(&mut self, dialog: Dialog<B, T>) -> Result<Ticket, Invalid> {
        self.validate(&dialog, false)?;
        Ok(self.insert(dialog))
    }
    fn pop(&mut self, reason: Reason) -> Option<Dismissed<B>> {
        let layer = self.layers.pop()?;
        self.focus.pop();
        self.returned();
        Some(Dismissed {
            id: layer.id,
            ticket: layer.ticket,
            reason,
            body: layer.body,
        })
    }
    /// Close only the named active opening; repeated or stale results are inert.
    pub fn close(&mut self, ticket: &Ticket, reason: Reason) -> Option<Dismissed<B>> {
        if !self.accepts(ticket) {
            return None;
        }
        self.pop(reason)
    }
    /// Replace the top body with a fresh opening and report its old dismissal.
    /// # Errors
    /// Same validation as push; empty stacks cannot replace.
    pub fn replace(&mut self, dialog: Dialog<B, T>) -> Result<(Ticket, Dismissed<B>), Invalid> {
        self.validate(&dialog, true)?;
        let title = self
            .layers
            .last()
            .map(|layer| layer.title.clone())
            .unwrap_or_default();
        let old = self.pop(Reason::Replaced).ok_or(Invalid::Empty)?;
        let ticket = self.insert(dialog);
        self.notice(&format!("! dialog · replaced {title}"));
        Ok((ticket, old))
    }
    /// Clear top to bottom, returning all bodies for host cancellation/cleanup.
    pub fn clear(&mut self) -> Vec<Dismissed<B>> {
        let mut closed = Vec::new();
        while let Some(layer) = self.pop(Reason::Cleared) {
            closed.push(layer);
        }
        if let Some(timing) = &mut self.timing {
            for waiting in timing.queue.drain(..) {
                closed.push(Dismissed {
                    id: waiting.id,
                    ticket: Ticket(Arc::new(())),
                    reason: Reason::Cleared,
                    body: waiting.body,
                });
            }
            timing.notice = None;
        }
        closed
    }
    /// Route to exactly the top body. Its handler returns whether it consumed the
    /// event; Escape closes only when it declines. Tab stays in the active scope.
    /// Outside presses never reach a body or the background.
    pub fn dispatch(
        &mut self,
        event: &Event,
        area: Rect,
        handler: impl FnOnce(&mut B, Option<&T>, &Event) -> bool,
    ) -> Dispatch<B> {
        if let Some(result) = self.timed_dispatch(event) {
            return result;
        }
        let Some(layer) = self.layers.last_mut() else {
            return Dispatch::Background;
        };
        if let Event::Key(key) = event {
            if key.kind == KeyEventKind::Release {
                return Dispatch::Consumed;
            }
            if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
                self.focus.advance(
                    key.code == KeyCode::BackTab
                        || key.modifiers.contains(crate::event::KeyModifiers::SHIFT),
                );
                return Dispatch::Consumed;
            }
        }
        if let Event::Mouse(mouse) = event
            && !view::outer(area, layer.geometry()).contains((mouse.column, mouse.row).into())
        {
            if layer.close.outside_click && mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                return self
                    .pop(Reason::Outside)
                    .map_or(Dispatch::Consumed, Dispatch::Dismissed);
            }
            return Dispatch::Consumed;
        }
        let consumed = handler(&mut layer.body, self.focus.active(), event);
        let escape = layer.close.escape;
        if !consumed
            && !escape
            && matches!(event, Event::Key(key) if key.code == KeyCode::Esc && key.kind == KeyEventKind::Press)
        {
            self.notice("! answer to continue");
        }
        if !consumed
            && escape
            && matches!(event,Event::Key(key) if key.code==KeyCode::Esc && key.kind==KeyEventKind::Press)
        {
            return self
                .pop(Reason::Cancelled)
                .map_or(Dispatch::Consumed, Dispatch::Dismissed);
        }
        Dispatch::Consumed
    }
}

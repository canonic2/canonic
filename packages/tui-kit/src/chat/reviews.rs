//! Composes reviews without mutating Composer documents or dropping unread transcript work.

use super::{Anchor, Chat, Role, Turn};
use crate::review_queue::{Review, ReviewQueue};

pub(super) struct Saved {
    pub anchor: Option<Anchor>,
    pub follow: bool,
}
impl Chat {
    /// Append a review and save the current anchor before concurrent transcript updates.
    /// # Errors
    /// Invalid queue identity/order/capacity leaves the current opening intact.
    pub fn push_review(&mut self, review: Review) -> Result<(), crate::review_queue::Invalid> {
        self.reviews.push(review)?;
        self.sync_reviews();
        Ok(())
    }
    /// Shared review owner. Host snapshots, order, withdrawal and acknowledgement stay explicit.
    pub fn reviews_mut(&mut self) -> &mut ReviewQueue {
        &mut self.reviews
    }
    /// Inspect current review focus and queue count.
    #[must_use]
    pub fn reviews(&self) -> &ReviewQueue {
        &self.reviews
    }
    pub(super) fn sync_reviews(&mut self) {
        for review in self.reviews.take_finished() {
            match review {
                Review::Permission(panel) => {
                    let choice = panel.receipt().and_then(|response| {
                        panel
                            .request()
                            .choices
                            .iter()
                            .find(|c| c.id == response.choice)
                    });
                    let label = format!(
                        "{} · {}{}",
                        receipt_title(&panel.request().title, 48),
                        panel.outcome(),
                        choice.map_or_else(String::new, |c| format!(
                            " · {}",
                            receipt_title(&c.label, 32)
                        ))
                    );
                    let body = panel
                        .receipt()
                        .map_or_else(String::new, |r| r.scope.clone());
                    self.review_serial = self.review_serial.saturating_add(1);
                    self.push_record(Turn::new(
                        &format!("review-receipt-{}", self.review_serial),
                        Role::User,
                        &label,
                        &body,
                    ));
                }
                Review::Questions(form) => {
                    let label = if let Some(response) = form.receipt() {
                        format!(
                            "{} · {} answered{}",
                            receipt_title(&form.request().title, 48),
                            response.answers.len(),
                            if response.partial { " · partial" } else { "" }
                        )
                    } else {
                        format!(
                            "{} · cancelled · nothing sent",
                            receipt_title(&form.request().title, 48)
                        )
                    };
                    self.review_serial = self.review_serial.saturating_add(1);
                    let body = if form.receipt().is_some() {
                        form.answer_text()
                    } else {
                        String::new()
                    };
                    self.push_record(Turn::new(
                        &format!("review-receipt-{}", self.review_serial),
                        Role::User,
                        &label,
                        &body,
                    ));
                }
            }
        }
        if self.reviews.active().is_some() && self.review_saved.is_none() {
            self.review_saved = Some(Saved {
                anchor: self.anchor.clone(),
                follow: self.follow,
            });
            self.follow = false;
            self.leader = None;
        } else if self.reviews.active().is_none()
            && let Some(saved) = self.review_saved.take()
        {
            self.anchor = saved.anchor;
            self.follow = saved.follow;
            // A closed review restores follow without treating concurrent output as read.
            let unread = self.unread.clone();
            self.refresh();
            self.unread = unread;
        }
    }
}
fn receipt_title(text: &str, limit: usize) -> String {
    let title = crate::text::clean(text, limit, false);
    if title.len() < text.len() {
        format!("{title}...")
    } else {
        title
    }
}

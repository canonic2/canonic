//! Full, frozen administrative reviews split into bounded human forms. Page
//! acceptance never applies a prefix of a plan or exposes private contents to
//! the model; each next page gets a fresh connection-owned confirmation binding.
use super::Error;
use std::collections::VecDeque;

pub(super) struct Pages {
    fingerprint: String,
    pub(super) context: String,
    pub(super) remaining: VecDeque<String>,
    pub(super) total: usize,
    pub(super) accepted: usize,
    pub(super) awaiting: bool,
}

impl Pages {
    pub(super) fn new(message: &str) -> Result<Self, Error> {
        const CHUNK: usize = 40 * 1024;
        if message.len() > 512 * CHUNK {
            return Err(Error::Invalid(
                "administrative review exceeds total review capacity; select explicit agents and review one integration at a time",
            ));
        }
        let mut remaining = VecDeque::new();
        let mut start = 0;
        while start < message.len() {
            let mut end = (start + CHUNK).min(message.len());
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            remaining.push_back(message[start..end].to_owned());
            start = end;
        }
        Ok(Self {
            fingerprint: crate::policy::fingerprint(message.as_bytes()),
            context: String::new(),
            total: remaining.len(),
            remaining,
            accepted: 0,
            awaiting: false,
        })
    }

    pub(super) fn present(&mut self) -> Result<String, Error> {
        if self.awaiting {
            return Err(Error::StaleSnapshot);
        }
        let text = self.remaining.pop_front().ok_or(Error::StaleSnapshot)?;
        self.awaiting = true;
        let context = if self.context.is_empty() {
            String::new()
        } else {
            format!("{}\n", self.context)
        };
        let message = format!(
            "Shield administrative review — page {} of {}.\nReview ID: {}.\n{context}This is one frozen change. Accepting this page continues its review; nothing applies until ALL pages are accepted. Decline or cancel any page to discard the review. The final acceptance revalidates the exact plan before applying it.\n\n{text}",
            self.accepted + 1,
            self.total,
            self.fingerprint
        );
        if message.len() > 48 * 1024 {
            return Err(Error::Invalid(
                "administrative review page context exceeds the form bound",
            ));
        }
        Ok(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn administrative_review_pages_preserve_every_byte_and_utf8_boundary() {
        let text = "αβγ🙂\n".repeat(20_000);
        let mut pages = Pages::new(&text).unwrap();
        assert!(pages.total > 1);
        let mut recovered = String::new();
        while !pages.remaining.is_empty() {
            let page = pages.present().unwrap();
            assert!(page.len() < 48 * 1024);
            recovered.push_str(page.split_once("\n\n").unwrap().1);
            assert!(
                pages.present().is_err(),
                "one presentation per accepted page"
            );
            pages.awaiting = false;
            pages.accepted += 1;
        }
        assert!(
            text == recovered,
            "review bytes changed: {} input, {} recovered",
            text.len(),
            recovered.len()
        );
    }
}

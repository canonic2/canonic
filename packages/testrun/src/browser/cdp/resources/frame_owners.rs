//! Explicit frame-lifetime authority; document references are never refreshed.
use super::*;

impl Index {
    pub(crate) fn retain_frame(&mut self, frame: Reference) -> Result<Reference, ResourceError> {
        let native = self.native(frame, Kind::Frame)?;
        let page = self.owning_page(frame)?;
        if let Some(owner) = self.nodes.values().find(|node| {
            node.kind == Kind::FrameOwner && node.parent == Some(page) && node.native == native
        }) {
            self.resolve(owner.reference, Some(Kind::FrameOwner))?;
            return Ok(owner.reference);
        }
        self.issue(Kind::FrameOwner, &native, Some(page), None)
    }
    pub(crate) fn owner_frame(&self, owner: Reference) -> Result<Reference, ResourceError> {
        let owner = self.resolve(owner, Some(Kind::FrameOwner))?;
        let page = owner.parent.ok_or(ResourceError::Scope)?;
        self.resolve(page, Some(Kind::Page))?;
        let frame = self
            .frames
            .get(&(page.id, owner.native.clone()))
            .and_then(|id| self.nodes.get(id))
            .ok_or(ResourceError::Stale)?;
        self.resolve(frame.reference, Some(Kind::Frame))?;
        if self.ancestor(frame.reference.id, Kind::Page) != Some(page) {
            return Err(ResourceError::Scope);
        }
        Ok(frame.reference)
    }
    pub(crate) fn release_frame(&mut self, owner: Reference) -> Result<(), ResourceError> {
        self.resolve(owner, Some(Kind::FrameOwner))?;
        self.retire(owner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Index, Reference, Reference) {
        let mut index = Index::new(Uuid::new_v4());
        let context = index.register_context("context").unwrap();
        let page = index
            .issue(Kind::Page, "page", Some(context), None)
            .unwrap();
        let frame = index.issue(Kind::Frame, "frame", Some(page), None).unwrap();
        index.frames.insert((page.id, "frame".into()), frame.id);
        (index, page, frame)
    }
    #[test]
    fn native_frame_owner_survives_document_rotation_without_reviving_document_authority() {
        let (mut index, _, frame) = fixture();
        let owner = index.retain_frame(frame).unwrap();
        let count = index.nodes.len();
        for iteration in 0..5 {
            index
                .document_changed(frame.id, Some(format!("loader-{iteration}")))
                .unwrap();
            let current = index.owner_frame(owner).unwrap();
            assert_ne!(current, frame);
            assert_eq!(current.id, frame.id);
            assert_eq!(index.native(frame, Kind::Frame), Err(ResourceError::Stale));
            assert_eq!(index.retain_frame(current).unwrap(), owner);
            assert_eq!(index.nodes.len(), count);
        }
    }
    #[test]
    fn native_frame_owner_release_is_not_a_native_scope_close_witness() {
        let (mut index, _, frame) = fixture();
        let owner = index.retain_frame(frame).unwrap();
        let world = index
            .issue(Kind::World, "world", Some(frame), Some("session".into()))
            .unwrap();
        index.release_frame(owner).unwrap();
        assert_eq!(index.owner_frame(owner), Err(ResourceError::Stale));
        assert_eq!(index.native(frame, Kind::Frame).unwrap(), "frame");
        assert_eq!(index.native(world, Kind::World).unwrap(), "world");
        assert_ne!(index.retain_frame(frame).unwrap(), owner);
    }
    #[test]
    fn native_frame_owner_ends_on_actual_frame_or_page_retirement_and_preserves_foreign_fences() {
        let (mut index, page, frame) = fixture();
        let owner = index.retain_frame(frame).unwrap();
        let mut foreign = owner;
        foreign.connection_id = Uuid::new_v4();
        assert_eq!(index.owner_frame(foreign), Err(ResourceError::Foreign));
        let mut forged = owner;
        forged.generation = Uuid::new_v4();
        assert_eq!(index.owner_frame(forged), Err(ResourceError::Stale));
        assert_eq!(index.owner_frame(frame), Err(ResourceError::Scope));
        index.retire(frame).unwrap();
        assert_eq!(index.owner_frame(owner), Err(ResourceError::Stale));
        let frame = index
            .issue(Kind::Frame, "replacement", Some(page), None)
            .unwrap();
        index
            .frames
            .insert((page.id, "replacement".into()), frame.id);
        let owner = index.retain_frame(frame).unwrap();
        index.retire(page).unwrap();
        assert_eq!(index.owner_frame(owner), Err(ResourceError::Stale));
    }
    #[test]
    fn native_frame_owner_obeys_global_capacity_and_frame_closing_admission() {
        let (mut index, page, frame) = fixture();
        while index.nodes.len() < MAX_RESOURCES {
            index
                .issue(
                    Kind::World,
                    &format!("world-{}", index.nodes.len()),
                    Some(frame),
                    None,
                )
                .unwrap();
        }
        assert_eq!(index.retain_frame(frame), Err(ResourceError::Capacity));
        assert_eq!(index.nodes.len(), MAX_RESOURCES);
        let (mut index, _, frame) = fixture();
        let owner = index.retain_frame(frame).unwrap();
        index.mark_closing(frame).unwrap();
        assert_eq!(index.owner_frame(owner), Err(ResourceError::Stale));
        assert!(index.resolve(owner, Some(Kind::FrameOwner)).is_ok());
        index.release_frame(owner).unwrap();
        assert!(index.nodes.contains_key(&frame.id));
        let _ = page;
    }
}

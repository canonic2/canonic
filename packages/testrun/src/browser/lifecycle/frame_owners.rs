//! Retained logical frames share a wire object without changing document authority.
use super::*;

impl State {
    /// Retain `frame`'s identity across its document changes; repeated retention
    /// of the same frame returns the existing owner.
    pub(crate) fn retain_frame(&mut self, frame: Reference) -> Result<Reference, &'static str> {
        let frame = self.resource(frame, Some(Kind::Frame))?.clone();
        let page = self
            .ancestor(frame.reference, Kind::Page)
            .ok_or("INVALID_SCOPE")?;
        self.issue(Kind::FrameOwner, frame.wire, Some(page))
    }
    /// The owner's current document frame.
    pub(crate) fn owner_frame(&self, owner: Reference) -> Result<Resource, &'static str> {
        let owner = self.resource(owner, Some(Kind::FrameOwner))?;
        let frame = self.by_wire(&owner.wire).ok_or("STALE_REFERENCE")?;
        self.resource(frame.reference, Some(Kind::Frame))?;
        if self.ancestor(frame.reference, Kind::Page) != owner.parent {
            return Err("INVALID_SCOPE");
        }
        Ok(frame)
    }
    /// A frame reference, or a frame owner's current document frame.
    pub(crate) fn document_frame(&self, frame: Reference) -> Result<Resource, &'static str> {
        if self.resource(frame, None)?.kind == Kind::FrameOwner {
            self.owner_frame(frame)
        } else {
            self.resource(frame, Some(Kind::Frame)).cloned()
        }
    }
    /// Release retained authority without closing the underlying frame.
    pub(crate) fn release_frame(&mut self, owner: Reference) -> Result<(), &'static str> {
        self.resource(owner, Some(Kind::FrameOwner))?;
        self.retire(owner);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::objects::ObjectTable;
    fn fixture() -> (State, ObjectTable, Reference, Reference, ObjectRef) {
        let mut state = State::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            &LaunchOptions {
                media_fixtures: Default::default(),
                engine: super::super::super::contract::Engine::Firefox,
                mode: super::super::super::contract::Mode::Headless,
                proxy: None,
                context: Default::default(),
            },
        );
        let mut objects = ObjectTable::default();
        let root = objects.lookup("", "Root").unwrap();
        let page = state
            .issue(
                Kind::Page,
                objects.create(&root, "page", "Page").unwrap(),
                None,
            )
            .unwrap();
        let wire = objects.create(&root, "frame", "Frame").unwrap();
        let frame = state.issue(Kind::Frame, wire.clone(), Some(page)).unwrap();
        (state, objects, page, frame, wire)
    }
    #[test]
    fn sdk_frame_owner_survives_navigation_without_changing_wire_discovery_or_document_fences() {
        let (mut state, mut objects, _, frame, wire) = fixture();
        let root = objects.lookup("", "Root").unwrap();
        let handle = state
            .issue(
                Kind::Handle,
                objects.create(&root, "handle", "JSHandle").unwrap(),
                Some(frame),
            )
            .unwrap();
        let owner = state.retain_frame(frame).unwrap();
        let count = state.resources.len() - 1;
        for _ in 0..5 {
            state.navigate(&wire);
            let current = state.owner_frame(owner).unwrap();
            assert_ne!(current.reference, frame);
            assert_eq!(current.reference.id, frame.id);
            assert_eq!(state.resource(frame, None).err(), Some("STALE_REFERENCE"));
            assert_eq!(state.resource(handle, None).err(), Some("STALE_REFERENCE"));
            assert_eq!(state.by_wire(&wire).unwrap().kind, Kind::Frame);
            assert_eq!(state.retain_frame(current.reference).unwrap(), owner);
            assert_eq!(state.resources.len(), count);
        }
    }
    #[test]
    fn sdk_frame_owner_release_preserves_the_underlying_live_frame_and_native_wire() {
        let (mut state, _, _, frame, wire) = fixture();
        let owner = state.retain_frame(frame).unwrap();
        state.release_frame(owner).unwrap();
        assert_eq!(state.owner_frame(owner).err(), Some("STALE_REFERENCE"));
        assert_eq!(state.by_wire(&wire).unwrap().reference, frame);
        assert!(state.resource(frame, Some(Kind::Frame)).is_ok());
        assert_ne!(state.retain_frame(frame).unwrap(), owner);
    }
    #[test]
    fn sdk_frame_owner_actual_frame_and_page_close_retire_aliases_with_foreign_and_closing_checks()
    {
        let (mut state, _, _, frame, _) = fixture();
        let owner = state.retain_frame(frame).unwrap();
        let mut foreign = owner;
        foreign.connection_id = Uuid::new_v4();
        assert_eq!(state.owner_frame(foreign).err(), Some("FOREIGN_REFERENCE"));
        let mut forged = owner;
        forged.generation = Uuid::new_v4();
        assert_eq!(state.owner_frame(forged).err(), Some("STALE_REFERENCE"));
        assert_eq!(state.owner_frame(frame).err(), Some("INVALID_SCOPE"));
        state.closing.insert(frame);
        assert_eq!(state.owner_frame(owner).err(), Some("SCOPE_CLOSING"));
        state.closing.remove(&frame);
        state.retire(frame);
        assert_eq!(state.owner_frame(owner).err(), Some("STALE_REFERENCE"));
        let (mut state, _, _, frame, _) = fixture();
        let owner = state.retain_frame(frame).unwrap();
        let page = state.ancestor(frame, Kind::Page).unwrap();
        state.retire(page);
        assert_eq!(state.owner_frame(owner).err(), Some("STALE_REFERENCE"));
    }
    #[test]
    fn sdk_frame_owner_uses_existing_resource_capacity_without_partially_publishing() {
        let (mut state, mut objects, _, frame, _) = fixture();
        let root = objects.lookup("", "Root").unwrap();
        while state.resources.len() < MAX_RESOURCES {
            let count = state.resources.len();
            state
                .issue(
                    Kind::Handle,
                    objects
                        .create(&root, &format!("handle-{count}"), "JSHandle")
                        .unwrap(),
                    Some(frame),
                )
                .unwrap();
        }
        assert_eq!(state.retain_frame(frame), Err("RESOURCE_CAPACITY"));
        assert_eq!(state.resources.len(), MAX_RESOURCES);
    }
}

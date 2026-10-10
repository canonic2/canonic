//! Owns an instance-local focus order and nested modal return scopes, without I/O.

#[derive(Clone)]
struct Scope<T> {
    targets: Vec<T>,
    active: Option<T>,
}

/// Explicit focus state for stable caller-defined target IDs.
pub struct Focus<T> {
    scope: Scope<T>,
    parents: Vec<Scope<T>>,
}

impl<T: Clone + Eq> Focus<T> {
    /// Create a focus order. Duplicate target IDs are rejected; empty scopes are valid.
    #[must_use]
    pub fn new(targets: Vec<T>) -> Option<Self> {
        if targets
            .iter()
            .enumerate()
            .any(|(i, t)| targets[..i].contains(t))
        {
            return None;
        }
        Some(Self {
            scope: Scope {
                active: targets.first().cloned(),
                targets,
            },
            parents: Vec::new(),
        })
    }

    /// Current visible, enabled target; hosts omit disabled/hidden targets from the order.
    #[must_use]
    pub fn active(&self) -> Option<&T> {
        self.scope.active.as_ref()
    }

    /// Whether a modal scope is active.
    #[must_use]
    pub fn is_modal(&self) -> bool {
        !self.parents.is_empty()
    }

    /// Move cyclically within the current scope.
    pub fn advance(&mut self, reverse: bool) {
        let count = self.scope.targets.len();
        if count == 0 {
            self.scope.active = None;
            return;
        }
        let current = self
            .scope
            .active
            .as_ref()
            .and_then(|a| self.scope.targets.iter().position(|t| t == a))
            .unwrap_or(0);
        let next = if reverse {
            (current + count - 1) % count
        } else {
            (current + 1) % count
        };
        self.scope.active = Some(self.scope.targets[next].clone());
    }

    /// Save current focus and activate modal targets, rejecting duplicate identities.
    pub fn push(&mut self, targets: Vec<T>) -> bool {
        let Some(next) = Self::new(targets) else {
            return false;
        };
        self.parents
            .push(std::mem::replace(&mut self.scope, next.scope));
        true
    }

    /// Close one modal and restore its surviving return target. No change without a modal.
    pub fn pop(&mut self) -> bool {
        let Some(parent) = self.parents.pop() else {
            return false;
        };
        self.scope = parent;
        true
    }

    /// Remove a hidden/destroyed target from every scope and repair return focus.
    pub fn remove(&mut self, target: &T) {
        for scope in self
            .parents
            .iter_mut()
            .chain(std::iter::once(&mut self.scope))
        {
            scope.targets.retain(|t| t != target);
            if scope.active.as_ref() == Some(target) {
                scope.active = scope.targets.first().cloned();
            }
        }
    }
}

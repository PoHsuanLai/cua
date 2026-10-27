//! The leases cuad holds on casement: one per window, numbered by the compositor.

use cua_desktop::{DesktopError, LeaseId, ToplevelIdent};
use std::collections::BTreeMap;

/// Where a window's lease stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Claim {
    /// The compositor has been asked and has not answered.
    Asking,
    /// The compositor granted it.
    Held(LeaseId),
}

/// The windows that are leased or being leased.
#[derive(Debug, Default)]
pub(super) struct Leases {
    claims: BTreeMap<ToplevelIdent, Claim>,
}

impl Leases {
    /// Takes `window` before the compositor is asked; a window already claimed is `Busy`.
    pub(super) fn claim(&mut self, window: &ToplevelIdent) -> Result<(), DesktopError> {
        if self.claims.contains_key(window) {
            return Err(DesktopError::Busy);
        }
        self.claims.insert(window.clone(), Claim::Asking);
        Ok(())
    }

    /// The compositor granted `lease` on a claimed window.
    pub(super) fn grant(&mut self, window: &ToplevelIdent, lease: LeaseId) {
        self.claims.insert(window.clone(), Claim::Held(lease));
    }

    /// The compositor refused: the window is free again.
    pub(super) fn abandon(&mut self, window: &ToplevelIdent) {
        self.claims.remove(window);
    }

    /// Ends `lease`; whether it was held. An unknown lease is a lease that already ended.
    pub(super) fn end(&mut self, lease: LeaseId) -> bool {
        let before = self.claims.len();
        self.claims.retain(|_, claim| *claim != Claim::Held(lease));
        self.claims.len() != before
    }

    /// `Ok` when `lease` is held.
    pub(super) fn check(&self, lease: LeaseId) -> Result<(), DesktopError> {
        self.claims
            .values()
            .any(|claim| *claim == Claim::Held(lease))
            .then_some(())
            .ok_or(DesktopError::NoSuchWindow)
    }

    /// The window closed: its lease, if it held one, ended with it.
    pub(super) fn closed(&mut self, window: &ToplevelIdent) -> Option<LeaseId> {
        match self.claims.remove(window)? {
            Claim::Held(lease) => Some(lease),
            Claim::Asking => None,
        }
    }

    /// Every held lease, ended because the connection to the compositor went away.
    pub(super) fn drain(&mut self) -> Vec<LeaseId> {
        std::mem::take(&mut self.claims)
            .into_values()
            .filter_map(|claim| match claim {
                Claim::Held(lease) => Some(lease),
                Claim::Asking => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(text: &str) -> ToplevelIdent {
        ToplevelIdent::parse(text).unwrap()
    }

    #[test]
    fn a_window_has_one_lease_at_a_time() {
        let mut leases = Leases::default();
        assert_eq!(leases.claim(&window("w1")), Ok(()));
        assert_eq!(leases.claim(&window("w1")), Err(DesktopError::Busy));
        leases.grant(&window("w1"), LeaseId(7));
        assert_eq!(leases.claim(&window("w1")), Err(DesktopError::Busy));
        assert_eq!(leases.claim(&window("w2")), Ok(()));
        leases.abandon(&window("w2"));
        assert_eq!(leases.claim(&window("w2")), Ok(()));
    }

    #[test]
    fn ending_a_lease_frees_its_window_and_an_unknown_lease_is_not_an_error() {
        let mut leases = Leases::default();
        leases.claim(&window("w1")).unwrap();
        leases.grant(&window("w1"), LeaseId(7));
        assert_eq!(leases.check(LeaseId(7)), Ok(()));
        assert!(leases.end(LeaseId(7)));
        assert!(!leases.end(LeaseId(7)));
        assert!(!leases.end(LeaseId(99)));
        assert_eq!(leases.check(LeaseId(7)), Err(DesktopError::NoSuchWindow));
        assert_eq!(leases.claim(&window("w1")), Ok(()));
    }

    #[test]
    fn a_closed_window_takes_its_lease_and_a_lost_connection_takes_all() {
        let mut leases = Leases::default();
        leases.claim(&window("w1")).unwrap();
        leases.grant(&window("w1"), LeaseId(1));
        leases.claim(&window("w2")).unwrap();
        leases.grant(&window("w2"), LeaseId(2));
        leases.claim(&window("w3")).unwrap();
        assert_eq!(leases.closed(&window("w1")), Some(LeaseId(1)));
        assert_eq!(leases.closed(&window("w1")), None);
        assert_eq!(leases.drain(), vec![LeaseId(2)]);
        assert_eq!(leases.claim(&window("w3")), Ok(()));
    }
}

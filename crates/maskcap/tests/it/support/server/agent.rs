//! The agent side of the fake compositor: `quire_agent_manager_v1` grants or refuses a lease,
//! the lease hands out a capture source of its window and records the mask hints it is sent.

use super::Compositor;
use casement_protocols::server::quire_agent_v1::quire_agent_lease_v1::{
    self, QuireAgentLeaseV1, RevokeReason,
};
use casement_protocols::server::quire_agent_v1::quire_agent_manager_v1::{
    self, QuireAgentManagerV1,
};
use wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource};

/// What the compositor answers to `create_lease`.
#[derive(Debug, Clone, Copy)]
pub enum Stance {
    /// `granted`.
    Grant,
    /// `revoked` straight away, for this reason.
    Refuse(RevokeReason),
}

impl GlobalDispatch<QuireAgentManagerV1, ()> for Compositor {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<QuireAgentManagerV1>,
        _: &(),
        init: &mut DataInit<'_, Self>,
    ) {
        init.init(resource, ());
    }
}

impl Dispatch<QuireAgentManagerV1, ()> for Compositor {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &QuireAgentManagerV1,
        request: quire_agent_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let quire_agent_manager_v1::Request::CreateLease { id, toplevel, .. } = request {
            let top = toplevel.data::<usize>().copied().unwrap_or(usize::MAX);
            let lease = init.init(id, top);
            match state.stance {
                Stance::Grant => lease.granted(),
                Stance::Refuse(reason) => lease.revoked(reason),
            }
        }
    }
}

impl Dispatch<QuireAgentLeaseV1, usize> for Compositor {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &QuireAgentLeaseV1,
        request: quire_agent_lease_v1::Request,
        top: &usize,
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        use quire_agent_lease_v1::Request;
        match request {
            Request::GetCaptureSource { source } => {
                init.init(source, *top);
            }
            Request::SetMaskHints { hints } => state.hints.push(hints),
            _ => {}
        }
    }
}

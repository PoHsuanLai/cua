//! What every wayland state in this workspace shares: how it names objects and how it counts
//! round trips. Neither holds a global: a key is the wire's own object id, so it belongs to the
//! connection that made the object.

use wayland_client::protocol::wl_callback::WlCallback;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};

/// Names one object (a toplevel, a workspace, a capture session) in a state's tables. It is the
/// object's id on the connection, unique while the object lives; a state drops its record when
/// the compositor closes the object, before the id can be reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectKey(u32);

impl ObjectKey {
    /// The key of `object`.
    pub fn of(object: &impl Proxy) -> Self {
        Self(object.id().protocol_id())
    }
}

/// Tags a `wl_display.sync` callback.
#[derive(Debug)]
pub struct SyncTag;

/// Counts the round trips asked for and the ones that came back.
#[derive(Debug, Default)]
pub struct Rounds {
    issued: u64,
    done: u64,
}

impl Rounds {
    /// Asks for a round trip; the returned number is reached once every request before it has
    /// been answered.
    pub fn ask<S>(&mut self, connection: &Connection, qh: &QueueHandle<S>) -> u64
    where
        S: Dispatch<WlCallback, SyncTag> + 'static,
    {
        self.issued += 1;
        connection.display().sync(qh, SyncTag);
        self.issued
    }

    /// One round trip came back.
    pub fn answered(&mut self) {
        self.done += 1;
    }

    /// Whether round trip `n` has come back.
    pub fn reached(&self, n: u64) -> bool {
        self.done >= n
    }
}

//! `org.quire.Compositor1` on the session bus: casement's door for the agent tier.

use std::os::fd::OwnedFd;
use zbus::zvariant;

/// The calls cuad makes. The role is the compositor's to check: a connection that is not
/// allowed the agent role gets an access-denied error, which surfaces as the call failing.
#[zbus::proxy(
    interface = "org.quire.Compositor1",
    default_service = "org.quire.Compositor1",
    default_path = "/org/quire/Compositor1"
)]
pub(super) trait Compositor1 {
    /// A Wayland connection with `role`: the client end of the socket.
    fn connect(&self, role: &str) -> zbus::Result<zvariant::OwnedFd>;

    /// Leases `window` (its toplevel identifier) for the `devices` bit set (1 keyboard, 2 pointer,
    /// 4 touch): the lease number and the client end of an EIS socket.
    fn lease_window(&self, window: &str, devices: u32) -> zbus::Result<(u64, zvariant::OwnedFd)>;

    /// Makes the window `lease` holds the key window (the run is about to send keys).
    fn focus_lease(&self, lease: u64) -> zbus::Result<()>;

    /// Ends a lease and closes its socket.
    fn end_lease(&self, lease: u64) -> zbus::Result<()>;
}

/// The agent role's name in `Connect`.
const AGENT: &str = "agent";

/// Whether casement is on the bus, and how to reach it.
#[derive(Debug, Clone)]
pub struct Door {
    proxy: Compositor1Proxy<'static>,
}

impl Door {
    /// The door when `org.quire.Compositor1` has an owner on `connection`; `None` when no
    /// compositor of ours is running (or the bus cannot say).
    pub async fn find(connection: &zbus::Connection) -> Option<Door> {
        let bus = zbus::fdo::DBusProxy::new(connection).await.ok()?;
        let name = zbus::names::BusName::try_from("org.quire.Compositor1").ok()?;
        if !bus.name_has_owner(name).await.ok()? {
            return None;
        }
        let proxy = Compositor1Proxy::new(connection).await.ok()?;
        Some(Door { proxy })
    }

    /// A new Wayland connection with the agent role.
    pub async fn agent_connection(&self) -> Result<OwnedFd, zbus::Error> {
        Ok(self.proxy.connect(AGENT).await?.into())
    }

    pub(super) fn proxy(&self) -> &Compositor1Proxy<'static> {
        &self.proxy
    }
}

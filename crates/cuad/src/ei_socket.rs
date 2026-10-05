//! `com.system76.CosmicComp.Ei.GetSenderSocket` over D-Bus: where stock cosmic-comp hands out the
//! EIS socket of its shared seat.
//!
//! Stock allows the call only to a peer that owns `org.freedesktop.impl.portal.desktop.cosmic` or
//! `com.system76.CosmicOSK` (`COSMIC_ENFORCE_DBUS_OWNERS`, on by default; Spike A). cuad takes the
//! second name, once, before its first call: the workaround FINDINGS records. It is no security
//! boundary against another process of the same user, which can take the name as well.

use cua_desktop::{DesktopError, DeviceTypes, SenderSocket, SocketFuture};
use std::os::fd::OwnedFd;
use tokio::sync::OnceCell;
use zbus::fdo::RequestNameFlags;
use zbus::zvariant::OwnedFd as BusFd;

/// The compositor's bus name.
pub const EI_BUS_NAME: &str = "com.system76.CosmicComp";
/// The object that serves the call.
pub const EI_PATH: &str = "/com/system76/CosmicComp/Ei";
/// Its interface.
pub const EI_INTERFACE: &str = "com.system76.CosmicComp.Ei";
/// The name stock lets call it.
pub const OSK_NAME: &str = "com.system76.CosmicOSK";

/// The sender socket of stock cosmic-comp, asked for over a session-bus connection.
#[derive(Debug)]
pub struct CosmicEiSockets {
    connection: zbus::Connection,
    named: OnceCell<()>,
}

impl CosmicEiSockets {
    /// Sockets asked for over `connection`; nothing is called and no name is taken until the
    /// first lease.
    pub fn over(connection: zbus::Connection) -> Self {
        Self {
            connection,
            named: OnceCell::new(),
        }
    }

    /// Takes the OSK name, once. Failing to is not fatal: the compositor may allow the caller
    /// some other way (the portal's name, or `COSMIC_ENFORCE_DBUS_OWNERS=0`), and if it does not
    /// the call says so.
    async fn take_name(&self) {
        self.named
            .get_or_init(|| async {
                let _ = self
                    .connection
                    .request_name_with_flags(OSK_NAME, RequestNameFlags::DoNotQueue.into())
                    .await;
            })
            .await;
    }

    async fn ask(&self, devices: DeviceTypes) -> Result<OwnedFd, DesktopError> {
        self.take_name().await;
        let refused = |what: &str, error: zbus::Error| {
            DesktopError::Protocol(format!("GetSenderSocket {what}: {error}"))
        };
        let reply = self
            .connection
            .call_method(
                Some(EI_BUS_NAME),
                EI_PATH,
                Some(EI_INTERFACE),
                "GetSenderSocket",
                &(devices.0,),
            )
            .await
            .map_err(|e| refused("failed", e))?;
        let fd: BusFd = reply
            .body()
            .deserialize()
            .map_err(|e| refused("answered something else", e.into()))?;
        Ok(fd.into())
    }
}

impl SenderSocket for CosmicEiSockets {
    fn sender_socket(&self, devices: DeviceTypes) -> SocketFuture<'_> {
        Box::pin(self.ask(devices))
    }
}

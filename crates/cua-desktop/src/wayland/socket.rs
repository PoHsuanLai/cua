//! Where the stock lease's EIS socket comes from.
//!
//! Stock cosmic-comp hands an EIS sender socket to whoever calls
//! `com.system76.CosmicComp.Ei.GetSenderSocket` on the session bus. That call needs a bus, which
//! this crate does not link: cuad implements [`SenderSocket`] over zbus and passes it in.

use crate::DesktopError;
use std::fmt;
use std::future::Future;
use std::os::fd::OwnedFd;
use std::pin::Pin;

/// Which kinds of device a sender socket may carry: the bitmask of `GetSenderSocket`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceTypes(pub u32);

impl DeviceTypes {
    /// Keyboards.
    pub const KEYBOARD: DeviceTypes = DeviceTypes(1);
    /// Pointers.
    pub const POINTER: DeviceTypes = DeviceTypes(2);
    /// Touch.
    pub const TOUCH: DeviceTypes = DeviceTypes(4);

    /// What an agent seat needs: all three (Spike A asked for 7).
    pub const ALL: DeviceTypes = DeviceTypes(1 | 2 | 4);
}

/// What a [`SenderSocket`] answers with.
pub type SocketFuture<'a> =
    Pin<Box<dyn Future<Output = Result<OwnedFd, DesktopError>> + Send + 'a>>;

/// Opens an EIS sender socket on the compositor.
pub trait SenderSocket: Send + Sync + fmt::Debug {
    /// A new connection to the compositor's EIS server, for devices of `devices`.
    fn sender_socket(&self, devices: DeviceTypes) -> SocketFuture<'_>;
}

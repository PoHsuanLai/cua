//! `org.quire.Cua1.Manager` at `/org/quire/Cua1`: starting runs and the kill switch.

use std::collections::HashMap;
use zbus::fdo;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedObjectPath;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Cua1.Manager",
    default_service = "org.quire.Cua1",
    default_path = "/org/quire/Cua1"
)]
pub trait Manager {
    /// Starts a run (`CuaEnvelope<CuaTask>` JSON); answers its object. The router and the planner
    /// only.
    fn start(&self, task: &str) -> zbus::Result<OwnedObjectPath>;

    /// The runs now.
    fn runs(&self) -> zbus::Result<Vec<OwnedObjectPath>>;

    /// The control-centre kill switch: every run is cancelled.
    fn stop_all(&self) -> zbus::Result<()>;

    /// The backend: `cosmic_fork`, `cosmic_stock`, `kwin_stock` or `none`.
    #[zbus(property)]
    fn backend(&self) -> zbus::Result<String>;

    /// Mode slug to `full`, `hands_off` or `unsupported:<gap>`.
    #[zbus(property)]
    fn modes(&self) -> zbus::Result<HashMap<String, String>>;

    /// A run began.
    #[zbus(signal)]
    fn run_added(&self, run: OwnedObjectPath) -> zbus::Result<()>;

    /// A run's object went away.
    #[zbus(signal)]
    fn run_removed(&self, run: OwnedObjectPath) -> zbus::Result<()>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct ManagerSkeleton;

#[zbus::interface(name = "org.quire.Cua1.Manager")]
impl ManagerSkeleton {
    #[zbus(out_args("run"))]
    fn start(&self, task: String) -> fdo::Result<OwnedObjectPath> {
        let _ = task;
        Err(crate::introspect::frozen())
    }

    fn runs(&self) -> fdo::Result<Vec<OwnedObjectPath>> {
        Err(crate::introspect::frozen())
    }

    fn stop_all(&self) -> fdo::Result<()> {
        Err(crate::introspect::frozen())
    }

    #[zbus(property)]
    fn backend(&self) -> fdo::Result<String> {
        Err(crate::introspect::frozen())
    }

    #[zbus(property)]
    fn modes(&self) -> fdo::Result<HashMap<String, String>> {
        Err(crate::introspect::frozen())
    }

    #[zbus(signal)]
    async fn run_added(emitter: &SignalEmitter<'_>, run: OwnedObjectPath) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn run_removed(emitter: &SignalEmitter<'_>, run: OwnedObjectPath) -> zbus::Result<()>;
}

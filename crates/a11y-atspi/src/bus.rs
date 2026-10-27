//! The accessibility bus itself: `org.a11y.atspi` objects through zbus proxies.

use super::tree::{AtspiTree, Extents, RawNode};
use ::atspi::CoordType;
use ::atspi::object_ref::ObjectRefOwned;
use ::atspi::proxy::accessible::{AccessibleProxy, ObjectRefExt};
use ::atspi::proxy::proxy_ext::ProxyExt;
use a11y_tree::A11yError;
use cua_desktop::Pid;
use std::fmt::Display;
use zbus::names::BusName;
use zbus::proxy::CacheProperties;

/// The registry of the accessibility bus: its root lists the applications.
const REGISTRY: &str = "org.a11y.atspi.Registry";
const REGISTRY_ROOT: &str = "/org/a11y/atspi/accessible/root";

/// A bus error as a plain error: an object that is no longer there is `Gone`, anything else is
/// the bus not following the protocol.
pub(super) fn lift(error: impl Display) -> A11yError {
    let text = error.to_string();
    let vanished = ["UnknownObject", "ServiceUnknown", "NameHasNoOwner"]
        .iter()
        .any(|word| text.contains(word));
    if vanished {
        A11yError::Gone
    } else {
        A11yError::Protocol(text)
    }
}

/// The accessibility bus, over one connection to it.
#[derive(Debug, Clone)]
pub(super) struct BusTree {
    connection: zbus::Connection,
}

impl BusTree {
    pub(super) fn over(connection: zbus::Connection) -> Self {
        Self { connection }
    }
}

impl AtspiTree for BusTree {
    type Key = ObjectRefOwned;

    async fn applications(&self) -> Result<Vec<ObjectRefOwned>, A11yError> {
        let registry = AccessibleProxy::builder(&self.connection)
            .destination(REGISTRY)
            .map_err(lift)?
            .path(REGISTRY_ROOT)
            .map_err(lift)?
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(lift)?;
        let apps = registry.get_children().await.map_err(lift)?;
        Ok(apps.into_iter().filter(|a| !a.is_null()).collect())
    }

    async fn process_of(&self, app: &ObjectRefOwned) -> Result<Pid, A11yError> {
        let name = app
            .name()
            .ok_or_else(|| A11yError::Protocol("an application with no bus name".to_owned()))?
            .clone();
        let bus = zbus::fdo::DBusProxy::new(&self.connection)
            .await
            .map_err(lift)?;
        let pid = bus
            .get_connection_unix_process_id(BusName::from(name))
            .await
            .map_err(lift)?;
        Ok(Pid(pid))
    }

    async fn read(&self, node: &ObjectRefOwned) -> Result<RawNode<ObjectRefOwned>, A11yError> {
        let proxy = node
            .as_accessible_proxy(&self.connection)
            .await
            .map_err(lift)?;
        let name = proxy.name().await.map_err(lift)?;
        let role = proxy.get_role().await.map_err(lift)?;
        let states = proxy.get_state().await.map_err(lift)?;
        let children = proxy.get_children().await.map_err(lift)?;
        let interfaces = proxy.proxies().await.map_err(lift)?;
        // A missing interface or a failed call is an object without extents or actions.
        let extents = match interfaces.component().await {
            Ok(component) => match component.get_extents(CoordType::Screen).await {
                Ok((x, y, w, h)) => Extents::Screen { x, y, w, h },
                Err(_) => Extents::Unknown,
            },
            Err(_) => Extents::Unknown,
        };
        let actions = match interfaces.action().await {
            Ok(action) => action
                .get_actions()
                .await
                .map(|all| all.into_iter().map(|a| a.name).collect())
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        Ok(RawNode {
            role,
            name,
            states,
            extents,
            actions,
            children: children.into_iter().filter(|c| !c.is_null()).collect(),
        })
    }
}

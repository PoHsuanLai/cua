//! cuad's links to the daemons it works with, over one session-bus connection: the router
//! (`Intents1`) for the gate and the message path, memoryd (`Memory1`) for the event log and
//! inferd (`Inference1`) for the model.

use crate::model::InferdCuaModel;
use almanac_client::Memory;
use cua_host::DaemonLinks;
use cua_host::IntentsGate;
use docket_client::Intents;
use porter_capability::Tier;
use zbus::Connection;

/// The links of the running daemon, as `SystemSeams` names their types.
#[derive(Debug)]
pub struct Links {
    /// The router's gate.
    pub gate: IntentsGate<docket_client::DbusTransport>,
    /// The event log and the message path.
    pub events: DaemonLinks<almanac_client::DbusTransport, docket_client::DbusTransport>,
    /// The model, over inferd on the same bus.
    pub model: InferdCuaModel<inferd_client::DbusTransport>,
}

impl Links {
    /// Links that reach intentd, memoryd and inferd through `connection`. Nothing is called
    /// yet: a daemon that is not there is found when it is first asked, and each seam says so
    /// in its own words (`LinkError::Unavailable`, `Transient`).
    pub fn over(connection: &Connection, tier: Tier) -> Links {
        Links {
            gate: IntentsGate::over(docket_client::DbusTransport::new(connection.clone())),
            events: DaemonLinks::over(
                Memory::over(almanac_client::DbusTransport::new(connection.clone())),
                Intents::over(docket_client::DbusTransport::new(connection.clone())),
            ),
            model: InferdCuaModel::over(inferd_client::DbusTransport::over(connection.clone()))
                .on_tier(tier),
        }
    }
}

//! The real bundle: the seams of the running daemon.

use crate::backend::{CaptureBackend, SeatBackend};
use crate::clock::SystemClock;
use crate::gate::IntentsGate;
use crate::links::DaemonLinks;
use crate::model::InferdCuaModel;
use a11y_tree::AtspiSource;
use cua_desktop::CosmicDesktop;
use cua_run::Seams;

/// The daemon's seams: COSMIC (stock or the fork) for the desktop, EI for input, masked capture,
/// AT-SPI for the tree, inferd for the model, `Intents1.Gate` for the router, memoryd and the
/// router's message path for records and reports, and the system clock.
#[derive(Debug)]
pub struct SystemSeams;

impl Seams for SystemSeams {
    type Desktop = CosmicDesktop;
    type Seat = SeatBackend;
    type Capture = CaptureBackend;
    type A11y = AtspiSource;
    type Model = InferdCuaModel<porter_client::AnyTransport>;
    type Gate = IntentsGate<docket_client::DbusTransport>;
    type Events = DaemonLinks<almanac_client::DbusTransport, docket_client::DbusTransport>;
    type Time = SystemClock;
}

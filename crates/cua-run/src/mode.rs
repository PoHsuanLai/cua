//! What each run mode needs of the backend.

use crate::config::DefaultMode;
use cua_bus::{ModeGap, ModeSupport};
use cua_desktop::{BackendCaps, HiddenCapture, RunMode, SeatIsolation};

/// What `caps` can do for `mode`.
///
/// - In place: full on an agent seat; on a shared seat the run works while the person keeps their
///   hands off (`HandsOff`): physical input pauses it.
/// - Agent workspace: needs a hidden window that keeps rendering *and* input that reaches it.
///   Stock cosmic-comp renders hidden windows (measured) but delivers them no input and has no
///   agent seat, so it is unsupported there (`NoHiddenInput`); a compositor that stops rendering
///   them is `HiddenCaptureWithheld`.
/// - Nested session: needs a nested compositor host; no backend ships one yet.
pub fn mode_support(caps: &BackendCaps, mode: RunMode) -> ModeSupport {
    match mode {
        RunMode::InPlace => match caps.seat {
            SeatIsolation::AgentSeat => ModeSupport::Full,
            SeatIsolation::SharedSeat => ModeSupport::HandsOff,
        },
        RunMode::AgentWorkspace => match (caps.hidden, caps.seat) {
            (HiddenCapture::Withheld, _) => {
                ModeSupport::Unsupported(ModeGap::HiddenCaptureWithheld)
            }
            (HiddenCapture::Renders, SeatIsolation::SharedSeat) => {
                ModeSupport::Unsupported(ModeGap::NoHiddenInput)
            }
            (HiddenCapture::Renders, SeatIsolation::AgentSeat) => ModeSupport::Full,
        },
        RunMode::NestedSession => ModeSupport::Unsupported(ModeGap::NoNestedHost),
    }
}

/// The mode a run gets: the setting's own when it names one, and for `auto` the agent workspace
/// where it is fully supported, otherwise in place. `None` when the named mode is unsupported.
pub fn choose_mode(default: DefaultMode, caps: &BackendCaps) -> Option<RunMode> {
    let supported = |mode| !matches!(mode_support(caps, mode), ModeSupport::Unsupported(_));
    match default {
        DefaultMode::Auto => {
            if mode_support(caps, RunMode::AgentWorkspace) == ModeSupport::Full {
                Some(RunMode::AgentWorkspace)
            } else {
                Some(RunMode::InPlace)
            }
        }
        DefaultMode::InPlace => supported(RunMode::InPlace).then_some(RunMode::InPlace),
        DefaultMode::AgentWorkspace => {
            supported(RunMode::AgentWorkspace).then_some(RunMode::AgentWorkspace)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ModeSupport::{Full, HandsOff, Unsupported};
    use cua_bus::ModeGap::{HiddenCaptureWithheld, NoHiddenInput, NoNestedHost};
    use cua_desktop::{BackendKind, ConsentGuard, GlowDraw, MaskWhere};

    struct Row {
        name: &'static str,
        caps: BackendCaps,
        mode: RunMode,
        want: ModeSupport,
    }

    /// A compositor with an agent seat of its own; no backend is one until casement lands.
    fn agent_seat() -> BackendCaps {
        BackendCaps {
            seat: SeatIsolation::AgentSeat,
            masking: MaskWhere::Compositor,
            consent: ConsentGuard::CompositorOrigin,
            glow: GlowDraw::Compositor,
            ..BackendCaps::of(BackendKind::CosmicStock)
        }
    }

    fn withheld() -> BackendCaps {
        BackendCaps {
            hidden: HiddenCapture::Withheld,
            ..agent_seat()
        }
    }

    #[test]
    fn mode_support_table() {
        let fork = agent_seat();
        let stock = BackendCaps::of(BackendKind::CosmicStock);
        let kwin = BackendCaps::of(BackendKind::KwinStock);
        let rows = [
            Row {
                name: "agent seat in place",
                caps: fork,
                mode: RunMode::InPlace,
                want: Full,
            },
            Row {
                name: "agent seat workspace",
                caps: fork,
                mode: RunMode::AgentWorkspace,
                want: Full,
            },
            Row {
                name: "agent seat nested",
                caps: fork,
                mode: RunMode::NestedSession,
                want: Unsupported(NoNestedHost),
            },
            Row {
                name: "stock in place",
                caps: stock,
                mode: RunMode::InPlace,
                want: HandsOff,
            },
            Row {
                name: "stock workspace: renders, no input",
                caps: stock,
                mode: RunMode::AgentWorkspace,
                want: Unsupported(NoHiddenInput),
            },
            Row {
                name: "stock nested",
                caps: stock,
                mode: RunMode::NestedSession,
                want: Unsupported(NoNestedHost),
            },
            Row {
                name: "kwin in place",
                caps: kwin,
                mode: RunMode::InPlace,
                want: HandsOff,
            },
            Row {
                name: "kwin workspace",
                caps: kwin,
                mode: RunMode::AgentWorkspace,
                want: Unsupported(NoHiddenInput),
            },
            Row {
                name: "withheld workspace",
                caps: withheld(),
                mode: RunMode::AgentWorkspace,
                want: Unsupported(HiddenCaptureWithheld),
            },
            Row {
                name: "withheld in place",
                caps: withheld(),
                mode: RunMode::InPlace,
                want: Full,
            },
        ];
        for row in rows {
            assert_eq!(mode_support(&row.caps, row.mode), row.want, "{}", row.name);
        }
    }

    #[test]
    fn auto_means_the_workspace_where_supported_else_in_place() {
        let fork = agent_seat();
        let stock = BackendCaps::of(BackendKind::CosmicStock);
        assert_eq!(
            choose_mode(DefaultMode::Auto, &fork),
            Some(RunMode::AgentWorkspace)
        );
        assert_eq!(
            choose_mode(DefaultMode::Auto, &stock),
            Some(RunMode::InPlace)
        );
        assert_eq!(
            choose_mode(DefaultMode::InPlace, &stock),
            Some(RunMode::InPlace)
        );
        assert_eq!(choose_mode(DefaultMode::AgentWorkspace, &stock), None);
        assert_eq!(
            choose_mode(DefaultMode::AgentWorkspace, &fork),
            Some(RunMode::AgentWorkspace)
        );
    }
}

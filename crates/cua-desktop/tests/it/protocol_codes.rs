//! The glow, revoke, suspend and mode tables equal the generated `quire_agent_v1` enums.

use casement_protocols::client::quire_agent_v1::quire_agent_lease_v1::{
    GlowState as ProtocolGlow, RevokeReason as ProtocolRevoke, SuspendReason as ProtocolSuspend,
};
use casement_protocols::client::quire_agent_v1::quire_agent_manager_v1::LeaseMode;
use cua_desktop::{GlowState, RevokeReason, RunMode, Suspend, WireCode, mode_code};

#[test]
fn the_wire_tables_equal_the_protocol_enums() {
    assert_eq!(GlowState::Off.code(), WireCode(ProtocolGlow::Off as u32));
    assert_eq!(
        GlowState::Working.code(),
        WireCode(ProtocolGlow::Working as u32)
    );
    let at = cua_action::Rect {
        origin: cua_action::Point::new(cua_action::Coord(0), cua_action::Coord(0)),
        size: cua_action::Size::new(cua_action::Coord(1), cua_action::Coord(1)),
    };
    assert_eq!(
        GlowState::Acting { at }.code(),
        WireCode(ProtocolGlow::Acting as u32)
    );
    assert_eq!(
        GlowState::WaitingForYou.code(),
        WireCode(ProtocolGlow::WaitingForYou as u32)
    );
    let revoke = [
        (RevokeReason::KillChord, ProtocolRevoke::KillChord as u32),
        (
            RevokeReason::WindowClosed,
            ProtocolRevoke::WindowClosed as u32,
        ),
        (
            RevokeReason::SpaceChanged,
            ProtocolRevoke::SpaceChanged as u32,
        ),
        (RevokeReason::Compositor, ProtocolRevoke::Compositor as u32),
    ];
    for (reason, want) in revoke {
        assert_eq!(reason.code(), WireCode(want), "{reason:?}");
    }
    let suspend = [
        (Suspend::TakeOver, ProtocolSuspend::TakeOver as u32),
        (Suspend::Consent, ProtocolSuspend::Consent as u32),
        (Suspend::Paused, ProtocolSuspend::Paused as u32),
    ];
    for (why, want) in suspend {
        assert_eq!(why.code(), WireCode(want), "{why:?}");
    }
    assert_eq!(
        mode_code(RunMode::InPlace),
        WireCode(LeaseMode::InPlace as u32)
    );
    assert_eq!(
        mode_code(RunMode::AgentWorkspace),
        WireCode(LeaseMode::AgentWorkspace as u32)
    );
    assert_eq!(
        mode_code(RunMode::NestedSession),
        WireCode(LeaseMode::NestedSession as u32)
    );
}

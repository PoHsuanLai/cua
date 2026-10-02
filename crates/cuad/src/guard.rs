//! The checks every control call passes before it reaches a run.

use cua_bus::{ControlRefusal, CuaCaller, CuaMember, permits};
use cua_run::Enabled;

/// `Ok` when `caller` may call `member` now: it is on the allow-list for the member
/// (`NotPermitted` otherwise), and computer use is on for `Start` (`Disabled` otherwise). The
/// caller check comes first, so a stranger learns nothing about the setting.
pub fn guard(caller: CuaCaller, member: CuaMember, enabled: Enabled) -> Result<(), ControlRefusal> {
    if !permits(caller, member) {
        Err(ControlRefusal::NotPermitted)
    } else if member == CuaMember::Start && enabled == Enabled::Off {
        Err(ControlRefusal::Disabled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_refused_for_non_planner_caller_and_while_disabled() {
        struct Row {
            name: &'static str,
            caller: CuaCaller,
            member: CuaMember,
            enabled: Enabled,
            want: Result<(), ControlRefusal>,
        }
        let rows = [
            Row {
                name: "router starts",
                caller: CuaCaller::Router,
                member: CuaMember::Start,
                enabled: Enabled::On,
                want: Ok(()),
            },
            Row {
                name: "planner starts",
                caller: CuaCaller::Companion,
                member: CuaMember::Start,
                enabled: Enabled::On,
                want: Ok(()),
            },
            Row {
                name: "shell may not start",
                caller: CuaCaller::Shell,
                member: CuaMember::Start,
                enabled: Enabled::On,
                want: Err(ControlRefusal::NotPermitted),
            },
            Row {
                name: "a stranger may not start",
                caller: CuaCaller::Other,
                member: CuaMember::Start,
                enabled: Enabled::On,
                want: Err(ControlRefusal::NotPermitted),
            },
            Row {
                name: "off refuses the router",
                caller: CuaCaller::Router,
                member: CuaMember::Start,
                enabled: Enabled::Off,
                want: Err(ControlRefusal::Disabled),
            },
            Row {
                name: "off tells a stranger nothing",
                caller: CuaCaller::Other,
                member: CuaMember::Start,
                enabled: Enabled::Off,
                want: Err(ControlRefusal::NotPermitted),
            },
            Row {
                name: "stop-all works while off",
                caller: CuaCaller::Shell,
                member: CuaMember::StopAll,
                enabled: Enabled::Off,
                want: Ok(()),
            },
            Row {
                name: "only the shell answers",
                caller: CuaCaller::Companion,
                member: CuaMember::Answer,
                enabled: Enabled::On,
                want: Err(ControlRefusal::NotPermitted),
            },
        ];
        for row in rows {
            assert_eq!(
                guard(row.caller, row.member, row.enabled),
                row.want,
                "{}",
                row.name
            );
        }
    }
}

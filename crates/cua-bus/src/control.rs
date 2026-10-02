//! Who may call what on `Cua1`, and how a call is refused.

use serde::{Deserialize, Serialize};

/// Why a control call is refused. Each maps 1:1 to an error name
/// `org.quire.Cua1.Error.<Variant>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlRefusal {
    /// The caller is not on the allow-list for this member.
    NotPermitted,
    /// The run is not in a state where this applies.
    WrongState,
    /// No such question.
    UnknownQuestion,
    /// `cua.enabled` is off.
    Disabled,
    /// No such window.
    WindowUnknown,
    /// Another run holds the window.
    LeaseHeld,
    /// The backend cannot run this mode.
    ModeUnsupported,
    /// The task body is malformed or in a vocabulary this build does not read.
    BadTask,
}

impl ControlRefusal {
    /// Every refusal.
    pub const ALL: [ControlRefusal; 8] = [
        ControlRefusal::NotPermitted,
        ControlRefusal::WrongState,
        ControlRefusal::UnknownQuestion,
        ControlRefusal::Disabled,
        ControlRefusal::WindowUnknown,
        ControlRefusal::LeaseHeld,
        ControlRefusal::ModeUnsupported,
        ControlRefusal::BadTask,
    ];

    /// The D-Bus error name.
    pub fn error_name(self) -> &'static str {
        match self {
            ControlRefusal::NotPermitted => "org.quire.Cua1.Error.NotPermitted",
            ControlRefusal::WrongState => "org.quire.Cua1.Error.WrongState",
            ControlRefusal::UnknownQuestion => "org.quire.Cua1.Error.UnknownQuestion",
            ControlRefusal::Disabled => "org.quire.Cua1.Error.Disabled",
            ControlRefusal::WindowUnknown => "org.quire.Cua1.Error.WindowUnknown",
            ControlRefusal::LeaseHeld => "org.quire.Cua1.Error.LeaseHeld",
            ControlRefusal::ModeUnsupported => "org.quire.Cua1.Error.ModeUnsupported",
            ControlRefusal::BadTask => "org.quire.Cua1.Error.BadTask",
        }
    }
}

/// Who is calling, derived by cuad from the connection (never sent in a body).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CuaCaller {
    /// intentd: the router, acting for a `cua.run.start` call.
    Router,
    /// companiond: the planner.
    Companion,
    /// sill: the launcher, the orb and the control centre.
    Shell,
    /// Anything else on the bus.
    Other,
}

/// A member of `Cua1.Manager` or `Cua1.Run` a caller may try. Reading a property is not a
/// member: properties and signals are open to any caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CuaMember {
    /// `Manager.Start`.
    Start,
    /// `Manager.Runs`.
    Runs,
    /// `Manager.StopAll`.
    StopAll,
    /// `Run.Cancel`.
    Cancel,
    /// `Run.Pause`.
    Pause,
    /// `Run.Resume`.
    Resume,
    /// `Run.Answer`.
    Answer,
    /// `Run.TakeOver`.
    TakeOver,
    /// `Run.HandBack`.
    HandBack,
}

impl CuaMember {
    /// Every member.
    pub const ALL: [CuaMember; 9] = [
        CuaMember::Start,
        CuaMember::Runs,
        CuaMember::StopAll,
        CuaMember::Cancel,
        CuaMember::Pause,
        CuaMember::Resume,
        CuaMember::Answer,
        CuaMember::TakeOver,
        CuaMember::HandBack,
    ];
}

/// Whether `caller` may call `member`. `Start` is the router's and the planner's; stopping,
/// pausing, resuming and taking over are the router's, the planner's and the shell's; `Answer`
/// is the shell's alone (a question is for the person); `Runs` is open. A confirmation never
/// comes through here: it is the router's, answered through `Confirm1`.
pub fn permits(caller: CuaCaller, member: CuaMember) -> bool {
    use CuaCaller::{Companion, Router, Shell};
    match member {
        CuaMember::Runs => true,
        CuaMember::Start => matches!(caller, Router | Companion),
        CuaMember::StopAll
        | CuaMember::Cancel
        | CuaMember::Pause
        | CuaMember::Resume
        | CuaMember::TakeOver
        | CuaMember::HandBack => matches!(caller, Router | Companion | Shell),
        CuaMember::Answer => matches!(caller, Shell),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn who_may_call_what() {
        use CuaCaller::{Companion, Other, Router, Shell};
        use CuaMember::*;
        struct Row {
            member: CuaMember,
            allowed: &'static [CuaCaller],
        }
        let table = [
            Row {
                member: Start,
                allowed: &[Router, Companion],
            },
            Row {
                member: Runs,
                allowed: &[Router, Companion, Shell, Other],
            },
            Row {
                member: StopAll,
                allowed: &[Router, Companion, Shell],
            },
            Row {
                member: Cancel,
                allowed: &[Router, Companion, Shell],
            },
            Row {
                member: Pause,
                allowed: &[Router, Companion, Shell],
            },
            Row {
                member: Resume,
                allowed: &[Router, Companion, Shell],
            },
            Row {
                member: Answer,
                allowed: &[Shell],
            },
            Row {
                member: TakeOver,
                allowed: &[Router, Companion, Shell],
            },
            Row {
                member: HandBack,
                allowed: &[Router, Companion, Shell],
            },
        ];
        assert_eq!(table.len(), CuaMember::ALL.len());
        for row in &table {
            for caller in [Router, Companion, Shell, Other] {
                assert_eq!(
                    permits(caller, row.member),
                    row.allowed.contains(&caller),
                    "{caller:?} {:?}",
                    row.member
                );
            }
        }
    }

    #[test]
    fn only_the_shell_answers_a_question_and_nobody_unknown_controls_anything() {
        for caller in [CuaCaller::Router, CuaCaller::Companion, CuaCaller::Other] {
            assert!(!permits(caller, CuaMember::Answer), "{caller:?}");
        }
        for member in CuaMember::ALL {
            if member != CuaMember::Runs {
                assert!(!permits(CuaCaller::Other, member), "{member:?}");
            }
        }
    }

    #[test]
    fn every_refusal_has_its_error_name_and_the_names_are_distinct() {
        let mut names: Vec<&str> = ControlRefusal::ALL.iter().map(|r| r.error_name()).collect();
        for name in &names {
            assert!(name.starts_with("org.quire.Cua1.Error."), "{name}");
        }
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ControlRefusal::ALL.len());
        for refusal in ControlRefusal::ALL {
            let variant = format!("{refusal:?}");
            assert_eq!(
                refusal.error_name(),
                format!("org.quire.Cua1.Error.{variant}")
            );
        }
    }

    #[test]
    fn the_all_lists_cover_every_variant() {
        // A wildcard-free match: adding a variant breaks this until ALL is updated.
        for refusal in ControlRefusal::ALL {
            let _count = match refusal {
                ControlRefusal::NotPermitted
                | ControlRefusal::WrongState
                | ControlRefusal::UnknownQuestion
                | ControlRefusal::Disabled
                | ControlRefusal::WindowUnknown
                | ControlRefusal::LeaseHeld
                | ControlRefusal::ModeUnsupported
                | ControlRefusal::BadTask => 1,
            };
        }
        for member in CuaMember::ALL {
            let _count = match member {
                CuaMember::Start
                | CuaMember::Runs
                | CuaMember::StopAll
                | CuaMember::Cancel
                | CuaMember::Pause
                | CuaMember::Resume
                | CuaMember::Answer
                | CuaMember::TakeOver
                | CuaMember::HandBack => 1,
            };
        }
    }
}

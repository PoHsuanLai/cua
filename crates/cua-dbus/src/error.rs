//! `org.quire.Cua1.Error.<Variant>`: a control call refused. Mapped 1:1 from `ControlRefusal`.

use cua_bus::ControlRefusal;

/// The errors the control interfaces return.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.quire.Cua1.Error")]
pub enum CuaError {
    /// zbus's own failures.
    #[zbus(error)]
    ZBus(zbus::Error),
    /// The caller is not on the allow-list for this member.
    NotPermitted(String),
    /// The run is not in a state where this applies.
    WrongState(String),
    /// No such question.
    UnknownQuestion(String),
    /// `cua.enabled` is off.
    Disabled(String),
    /// No such window.
    WindowUnknown(String),
    /// Another run holds the window.
    LeaseHeld(String),
    /// The backend cannot run this mode.
    ModeUnsupported(String),
    /// The task body is malformed or in a vocabulary this build does not read.
    BadTask(String),
    /// No computer-use model can serve a run.
    ModelUnavailable(String),
}

impl CuaError {
    /// The refusal this error carries, or `None` for a bus failure.
    pub fn refusal(&self) -> Option<ControlRefusal> {
        match self {
            CuaError::ZBus(_) => None,
            CuaError::NotPermitted(_) => Some(ControlRefusal::NotPermitted),
            CuaError::WrongState(_) => Some(ControlRefusal::WrongState),
            CuaError::UnknownQuestion(_) => Some(ControlRefusal::UnknownQuestion),
            CuaError::Disabled(_) => Some(ControlRefusal::Disabled),
            CuaError::WindowUnknown(_) => Some(ControlRefusal::WindowUnknown),
            CuaError::LeaseHeld(_) => Some(ControlRefusal::LeaseHeld),
            CuaError::ModeUnsupported(_) => Some(ControlRefusal::ModeUnsupported),
            CuaError::BadTask(_) => Some(ControlRefusal::BadTask),
            CuaError::ModelUnavailable(_) => Some(ControlRefusal::ModelUnavailable),
        }
    }
}

impl From<ControlRefusal> for CuaError {
    fn from(refusal: ControlRefusal) -> Self {
        let text = String::new();
        match refusal {
            ControlRefusal::NotPermitted => CuaError::NotPermitted(text),
            ControlRefusal::WrongState => CuaError::WrongState(text),
            ControlRefusal::UnknownQuestion => CuaError::UnknownQuestion(text),
            ControlRefusal::Disabled => CuaError::Disabled(text),
            ControlRefusal::WindowUnknown => CuaError::WindowUnknown(text),
            ControlRefusal::LeaseHeld => CuaError::LeaseHeld(text),
            ControlRefusal::ModeUnsupported => CuaError::ModeUnsupported(text),
            ControlRefusal::BadTask => CuaError::BadTask(text),
            ControlRefusal::ModelUnavailable => CuaError::ModelUnavailable(text),
            // A refusal this build does not know stays a refusal, and the strictest one.
            _ => CuaError::NotPermitted(text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::DBusError;

    #[test]
    fn error_names_cover_control_refusal() {
        for refusal in ControlRefusal::ALL {
            let error = CuaError::from(refusal);
            assert_eq!(error.refusal(), Some(refusal));
            assert_eq!(error.name().as_str(), refusal.error_name(), "{refusal:?}");
        }
    }
}

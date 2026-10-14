//! Identifiers and the vocabulary envelope of the bus.

use serde::{Deserialize, Serialize};

/// A question a run asked the person (`Run.Answer` names it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct QuestionId(pub u64);

/// A per-run handle for the window an action targets; cuad maps it to a `ToplevelIdent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WindowRef(pub u32);

/// A learned procedure (a later wave; the type is frozen so a task can name one).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProcedureId(pub String);

/// The vocabulary version of the `Cua1` bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CuaVocab(pub u32);

impl CuaVocab {
    /// The version this build writes.
    pub const CURRENT: CuaVocab = CuaVocab(1);
}

/// A body with the vocabulary it is written in: `{"vocab": 1, "body": ...}`. One protocol with
/// one carrier: D-Bus strings holding this JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CuaEnvelope<T> {
    /// The vocabulary.
    pub vocab: CuaVocab,
    /// The body.
    pub body: T,
}

impl<T> CuaEnvelope<T> {
    /// `body` in the current vocabulary.
    pub fn current(body: T) -> Self {
        Self {
            vocab: CuaVocab::CURRENT,
            body,
        }
    }
}

/// The stable slug of a closed-set value: its serde form. A unit variant is its string; a
/// variant with data is its `kind`. `None` for a value that has neither.
pub fn slug_of<T: Serialize>(value: &T) -> Option<String> {
    match serde_json::to_value(value).ok()? {
        serde_json::Value::String(slug) => Some(slug),
        serde_json::Value::Object(map) => match map.get("kind") {
            Some(serde_json::Value::String(kind)) => Some(kind.clone()),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cua_desktop::BackendKind;

    #[test]
    fn slugs_are_the_serde_form_of_unit_and_tagged_variants() {
        assert_eq!(
            slug_of(&BackendKind::KwinStock).as_deref(),
            Some("kwin_stock")
        );
        assert_eq!(slug_of(&CuaVocab::CURRENT), None);
        assert_eq!(
            slug_of(&crate::CuaOutcome::BudgetOut(crate::RunBudgetKind::Steps)).as_deref(),
            Some("budget_out")
        );
    }

    #[test]
    fn the_envelope_pins_its_json() {
        let json = serde_json::to_string(&CuaEnvelope::current(QuestionId(4))).unwrap();
        assert_eq!(json, r#"{"vocab":1,"body":4}"#);
        let back: CuaEnvelope<QuestionId> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.body, QuestionId(4));
    }
}

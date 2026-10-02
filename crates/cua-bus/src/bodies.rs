//! The JSON bodies of the `Cua1` properties and signals.

use crate::budget::{BudgetUse, RunBudget};
use cua_desktop::WindowTitle;
use porter_core::AppName;
use prov::Labelled;
use serde::{Deserialize, Serialize};

/// The `Window` property: whose window the run is in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowBody {
    /// The app.
    pub app: AppName,
    /// The title: screen text, shown as data.
    pub title: WindowTitle,
}

/// The `Budget` property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetBody {
    /// The limits.
    pub limits: RunBudget,
    /// What has been used.
    pub used: BudgetUse,
}

/// The `AskUser` signal's body: a question the model asked. The answer comes back through
/// `Run.Answer`, from the shell only; a question is screen text and is shown as data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskBody {
    /// The question.
    pub question: Labelled<String>,
    /// The answers the model offers, if it offers any.
    pub choices: Vec<Labelled<String>>,
}

//! An accessibility source over fixed snapshots, loaded from the a11y-tree fixtures.

use a11y_tree::{A11yError, A11ySnapshot, A11ySource, A11yTarget, NodeBudget};

const GTK4_DIALOG: &str = include_str!("../../a11y-tree/fixtures/a11y/gtk4_dialog.json");
const FIREFOX_FORM: &str = include_str!("../../a11y-tree/fixtures/a11y/firefox_form.json");
const QUIRE_MAIL: &str = include_str!("../../a11y-tree/fixtures/a11y/quire_mail.json");

/// Which fixture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fixture {
    /// A GTK4 "Save changes?" dialog.
    Gtk4Dialog,
    /// A Firefox sign-in form with a password field.
    FirefoxForm,
    /// A quire mail window with typed hints.
    QuireMail,
}

/// An [`A11ySource`] that answers with one snapshot, or one error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeA11y {
    answer: Result<A11ySnapshot, A11yError>,
}

impl FakeA11y {
    /// Answers with the fixture's snapshot.
    pub fn fixture(which: Fixture) -> Self {
        let text = match which {
            Fixture::Gtk4Dialog => GTK4_DIALOG,
            Fixture::FirefoxForm => FIREFOX_FORM,
            Fixture::QuireMail => QUIRE_MAIL,
        };
        let snapshot =
            serde_json::from_str(text).expect("the fixtures are checked by a11y-tree's tests");
        Self {
            answer: Ok(snapshot),
        }
    }

    /// Answers with this snapshot.
    pub fn with(snapshot: A11ySnapshot) -> Self {
        Self {
            answer: Ok(snapshot),
        }
    }

    /// Answers with this error: the app exposes no tree, say.
    pub fn failing(error: A11yError) -> Self {
        Self { answer: Err(error) }
    }
}

impl A11ySource for FakeA11y {
    async fn snapshot(
        &self,
        target: &A11yTarget,
        budget: NodeBudget,
    ) -> Result<A11ySnapshot, A11yError> {
        let _ = (target, budget);
        self.answer.clone()
    }
}

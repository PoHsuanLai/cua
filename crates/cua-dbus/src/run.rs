//! `org.quire.Cua1.Run` at `/org/quire/Cua1/run/<id>`: one run's controls, state and signals.

use zbus::fdo;
use zbus::object_server::SignalEmitter;

/// The caller's side. The service is cuad's name, set per proxy with the run's path.
#[zbus::proxy(interface = "org.quire.Cua1.Run", default_service = "org.quire.Cua1")]
pub trait Run {
    /// Stops the run.
    fn cancel(&self) -> zbus::Result<()>;

    /// Pauses it.
    fn pause(&self) -> zbus::Result<()>;

    /// Resumes a paused run.
    fn resume(&self) -> zbus::Result<()>;

    /// Answers a question the model asked (the shell only).
    fn answer(&self, question: u64, answer: &str) -> zbus::Result<()>;

    /// The person takes the window.
    fn take_over(&self) -> zbus::Result<()>;

    /// The person hands it back; the run resumes.
    fn hand_back(&self) -> zbus::Result<()>;

    /// The phase: a `RunStateSlug`.
    #[zbus(property)]
    fn state(&self) -> zbus::Result<String>;

    /// The step number.
    #[zbus(property)]
    fn step(&self) -> zbus::Result<u32>;

    /// The goal.
    #[zbus(property)]
    fn goal(&self) -> zbus::Result<String>;

    /// The window (`WindowBody` JSON).
    #[zbus(property)]
    fn window(&self) -> zbus::Result<String>;

    /// The mode (`RunMode` slug).
    #[zbus(property)]
    fn mode(&self) -> zbus::Result<String>;

    /// The budget (`BudgetBody` JSON).
    #[zbus(property)]
    fn budget(&self) -> zbus::Result<String>;

    /// The phase changed (`RunStateSlug` slug). The Rust name differs from the bus name because
    /// the `State` property already owns `receive_state_changed`.
    #[zbus(signal, name = "StateChanged")]
    fn phase_changed(&self, state: &str) -> zbus::Result<()>;

    /// A step began.
    #[zbus(signal)]
    fn step_started(&self, step: u32) -> zbus::Result<()>;

    /// Some of the model's thought, as data.
    #[zbus(signal)]
    fn thought_delta(&self, step: u32, text: &str) -> zbus::Result<()>;

    /// An action was proposed (`TargetedAction` JSON).
    #[zbus(signal)]
    fn action_proposed(&self, step: u32, action: &str) -> zbus::Result<()>;

    /// An action ended (`ActionOutcome` JSON).
    #[zbus(signal)]
    fn action_done(&self, step: u32, outcome: &str) -> zbus::Result<()>;

    /// The model asked the person something (`AskBody` JSON).
    #[zbus(signal)]
    fn ask_user(&self, question: u64, ask: &str) -> zbus::Result<()>;

    /// The run ended (`RunEnd` JSON: the outcome and which `prov::Message` carries the report).
    #[zbus(signal)]
    fn finished(&self, end: &str) -> zbus::Result<()>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct RunSkeleton;

#[zbus::interface(name = "org.quire.Cua1.Run")]
impl RunSkeleton {
    fn cancel(&self) -> fdo::Result<()> {
        Err(crate::introspect::frozen())
    }

    fn pause(&self) -> fdo::Result<()> {
        Err(crate::introspect::frozen())
    }

    fn resume(&self) -> fdo::Result<()> {
        Err(crate::introspect::frozen())
    }

    fn answer(&self, question: u64, answer: String) -> fdo::Result<()> {
        let _ = (question, answer);
        Err(crate::introspect::frozen())
    }

    fn take_over(&self) -> fdo::Result<()> {
        Err(crate::introspect::frozen())
    }

    fn hand_back(&self) -> fdo::Result<()> {
        Err(crate::introspect::frozen())
    }

    #[zbus(property)]
    fn state(&self) -> fdo::Result<String> {
        Err(crate::introspect::frozen())
    }

    #[zbus(property)]
    fn step(&self) -> fdo::Result<u32> {
        Err(crate::introspect::frozen())
    }

    #[zbus(property)]
    fn goal(&self) -> fdo::Result<String> {
        Err(crate::introspect::frozen())
    }

    #[zbus(property)]
    fn window(&self) -> fdo::Result<String> {
        Err(crate::introspect::frozen())
    }

    #[zbus(property)]
    fn mode(&self) -> fdo::Result<String> {
        Err(crate::introspect::frozen())
    }

    #[zbus(property)]
    fn budget(&self) -> fdo::Result<String> {
        Err(crate::introspect::frozen())
    }

    #[zbus(signal, name = "StateChanged")]
    async fn phase_changed(emitter: &SignalEmitter<'_>, state: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn step_started(emitter: &SignalEmitter<'_>, step: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn thought_delta(emitter: &SignalEmitter<'_>, step: u32, text: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_proposed(
        emitter: &SignalEmitter<'_>,
        step: u32,
        action: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_done(emitter: &SignalEmitter<'_>, step: u32, outcome: &str)
    -> zbus::Result<()>;

    #[zbus(signal)]
    async fn ask_user(emitter: &SignalEmitter<'_>, question: u64, ask: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn finished(emitter: &SignalEmitter<'_>, end: &str) -> zbus::Result<()>;
}

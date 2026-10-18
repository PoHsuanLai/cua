//! `org.quire.Cua`: cuad as an intent provider, with one action, `cua.run.start`.

use cua_bus::ControlRefusal;
use cua_desktop::ToplevelIdent;
use cua_run::Enabled;
use docket_client::IntentProvider;
use docket_core::{
    AgentReach, AppRefusal, EntityRef, FailText, Follow, Hit, Invocation, Manifest, ManifestError,
    Outcome, ParamName, Preview, SuggestAsk, UndoFault, UndoToken, Undoable, ValidManifest, Value,
    validate,
};
use porter_core::AppName;
use prov::{
    Actor, AgentRef, EntityId, EntityKey, EntityKind, Label, Labelled, RunId, SessionId, SpaceId,
};
use tokio::sync::{mpsc, oneshot};

/// The shipped manifest (`manifests/org.quire.Cua.toml`).
const MANIFEST: &str = include_str!("../../../manifests/org.quire.Cua.toml");

/// Why the provider could not be built.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum ProviderError {
    /// The shipped file does not parse (a bug: a test reads it).
    #[error("manifest does not parse: {0}")]
    Parse(String),
    /// It does not validate.
    #[error("manifest is invalid: {0}")]
    Invalid(ManifestError),
}

/// The provider's manifest for a setting: `cua.run.start` is `Hidden` from agents while
/// computer use is off, `Offered` once it is on. The per-app consent is asked by the router at
/// the first use either way.
pub fn manifest_for(enabled: Enabled) -> Result<ValidManifest, ProviderError> {
    let mut manifest: Manifest =
        toml::from_str(MANIFEST).map_err(|e| ProviderError::Parse(e.to_string()))?;
    let reach = match enabled {
        Enabled::Off => AgentReach::Hidden,
        Enabled::On => AgentReach::Offered,
    };
    for action in &mut manifest.actions {
        action.reach = reach;
    }
    validate(manifest).map_err(ProviderError::Invalid)
}

/// Where a run is to run, as the planner chose: the setting's `auto` is resolved by cuad, which
/// knows its backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeChoice {
    /// The setting decides (`choose_mode`).
    Auto,
    /// In place.
    InPlace,
    /// The agent workspace.
    AgentWorkspace,
}

/// Why a start was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartRefusal {
    /// A refusal, and the sentence that says it to the person: what is wrong and where to put
    /// it right.
    Control {
        /// The refusal.
        why: ControlRefusal,
        /// What to tell the person.
        words: &'static str,
    },
    /// The app is not open, or none was named and none has the focus: these are open, by id.
    NeedsApp(Vec<AppName>),
}

/// What `cua.run.start` asked for, as the daemon needs it to make a [`cua_bus::CuaTask`].
#[derive(Debug)]
pub struct StartRequest {
    /// What to do: the planner's words, with their label.
    pub goal: Labelled<String>,
    /// The window to work in, when the planner named one.
    pub window: Option<ToplevelIdent>,
    /// The app, when it named that instead. With neither, the window the person is in.
    pub app: Option<AppName>,
    /// Where to run.
    pub mode: ModeChoice,
    /// The session that asked.
    pub session: SessionId,
    /// The Space it acts in.
    pub space: SpaceId,
    /// Who asked, so the report goes back to them.
    pub from: AgentRef,
    /// Where the answer goes: the run's id, or why there is none.
    pub reply: oneshot::Sender<Result<RunId, StartRefusal>>,
}

/// cuad's provider.
#[derive(Debug)]
pub struct CuaProvider {
    manifest: ValidManifest,
    starts: Option<mpsc::UnboundedSender<StartRequest>>,
}

impl CuaProvider {
    /// A provider whose action is offered or hidden by `enabled`. It starts nothing (`perform`
    /// is `Unsupported`); the daemon builds one with [`CuaProvider::starting`].
    pub fn new(enabled: Enabled) -> Result<Self, ProviderError> {
        Ok(Self {
            manifest: manifest_for(enabled)?,
            starts: None,
        })
    }

    /// A provider whose `perform` hands each start to whoever reads the returned queue (the
    /// daemon turns them into runs through its supervisor).
    pub fn starting(
        enabled: Enabled,
    ) -> Result<(Self, mpsc::UnboundedReceiver<StartRequest>), ProviderError> {
        let (starts, queue) = mpsc::unbounded_channel();
        Ok((
            Self {
                manifest: manifest_for(enabled)?,
                starts: Some(starts),
            },
            queue,
        ))
    }
}

fn param<'a>(inv: &'a Invocation, name: &str) -> Option<&'a Labelled<Value>> {
    inv.args
        .iter()
        .find(|(key, _)| key.as_str() == name)
        .map(|(_, value)| value)
}

fn needs(name: &str) -> AppRefusal {
    AppRefusal::NeedsParam {
        param: ParamName::parse(name).expect("a parameter name"),
        options: Vec::new(),
    }
}

/// The session and the requester an invocation came from: the companion, in a session. Anyone
/// else has no session to report to.
fn requester(actor: &Actor) -> Option<(SessionId, AgentRef)> {
    match actor {
        Actor::Companion { session, .. } => Some((session.clone(), AgentRef::of(actor)?)),
        _ => None,
    }
}

/// The start the invocation asks for.
fn start_of(
    inv: &Invocation,
) -> Result<(StartRequest, oneshot::Receiver<Result<RunId, StartRefusal>>), AppRefusal> {
    let goal = match param(inv, "goal") {
        Some(Labelled {
            value: Value::Text(text),
            label,
        }) => Labelled::new(text.clone(), label.clone()),
        _ => return Err(needs("goal")),
    };
    let window = match param(inv, "window") {
        Some(Labelled {
            value: Value::Entity(entity),
            ..
        }) => Some(ToplevelIdent::parse(entity.key.as_str()).map_err(|_| needs("window"))?),
        _ => None,
    };
    let app = match param(inv, "app") {
        Some(Labelled {
            value: Value::Text(text),
            ..
        }) => Some(AppName::parse(text).map_err(|_| needs("app"))?),
        _ => None,
    };
    let mode = match param(inv, "mode").map(|p| &p.value) {
        Some(Value::Choice(choice)) => match choice.as_str() {
            "in_place" => ModeChoice::InPlace,
            "agent_workspace" => ModeChoice::AgentWorkspace,
            _ => ModeChoice::Auto,
        },
        _ => ModeChoice::Auto,
    };
    let (session, from) = requester(&inv.actor).ok_or(AppRefusal::Unsupported)?;
    let (reply, answer) = oneshot::channel();
    Ok((
        StartRequest {
            goal,
            window,
            app,
            mode,
            session,
            space: inv.space.clone(),
            from,
            reply,
        },
        answer,
    ))
}

/// An open app as a choice for the `app` parameter: its id, which is also what to pass back.
fn app_option(app: AppName) -> Option<EntityRef> {
    let id = EntityId {
        app: AppName::parse("org.quire.Cua").ok()?,
        kind: EntityKind::parse("desktop.app").ok()?,
        key: EntityKey::parse(app.as_str()).ok()?,
    };
    let say = |text: &str| Labelled::new(text.to_owned(), Label::trusted_user());
    Some(EntityRef {
        id,
        title: say(app.as_str()),
        subtitle: say(""),
    })
}

/// What a refused start is in the words of the app's refusals: a busy window is `Busy`, no open
/// app of that name is `NeedsParam` with the open apps as options, and every other refusal
/// carries its sentence, so the planner is told why and does not ask again blind.
fn refusal_of(refused: StartRefusal) -> AppRefusal {
    match refused {
        StartRefusal::Control {
            why: ControlRefusal::LeaseHeld,
            ..
        } => AppRefusal::Busy,
        StartRefusal::Control { words, .. } => AppRefusal::Failed(FailText(words.to_owned())),
        StartRefusal::NeedsApp(apps) => AppRefusal::NeedsParam {
            param: ParamName::parse("app").expect("a parameter name"),
            options: apps.into_iter().filter_map(app_option).collect(),
        },
    }
}

impl IntentProvider for CuaProvider {
    fn manifest(&self) -> &ValidManifest {
        &self.manifest
    }

    async fn perform(&self, inv: Invocation) -> Result<Outcome, AppRefusal> {
        let starts = self.starts.as_ref().ok_or(AppRefusal::Unsupported)?;
        let (start, answer) = start_of(&inv)?;
        let label = start.goal.label.clone();
        starts.send(start).map_err(|_| AppRefusal::Busy)?;
        let id = answer
            .await
            .map_err(|_| AppRefusal::Busy)?
            .map_err(refusal_of)?;
        // The run's id is cuad's own word; its report arrives as a message.
        Ok(Outcome {
            value: Some(Labelled::new(
                Value::Text(id.as_str().to_owned()),
                Label {
                    integrity: prov::Integrity::Trusted,
                    ..label
                },
            )),
            said: None,
            show: Preview::None,
            undo: Undoable::No,
            follow: Follow::Nothing,
        })
    }

    async fn dry_run(&self, inv: Invocation) -> Result<Preview, AppRefusal> {
        let _ = inv;
        Err(AppRefusal::Unsupported)
    }

    async fn undo(&self, token: UndoToken, actor: Actor) -> Result<(), UndoFault> {
        let _ = (token, actor);
        Err(UndoFault::Gone)
    }

    async fn search(&self, text: &str) -> Vec<Hit> {
        let _ = text;
        Vec::new()
    }

    async fn preview(&self, id: &EntityId) -> Preview {
        let _ = id;
        Preview::None
    }

    async fn suggest(&self, ask: SuggestAsk) -> Vec<EntityRef> {
        let _ = ask;
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reasons::words_of;
    use docket_core::{ArgSink, Lasting};
    use prov::Effect;

    #[test]
    fn the_shipped_manifest_validates_and_declares_one_read_action() {
        for enabled in [Enabled::Off, Enabled::On] {
            let valid = manifest_for(enabled).expect("a valid manifest");
            let manifest = valid.manifest();
            assert_eq!(manifest.app.as_str(), "org.quire.Cua");
            assert_eq!(manifest.actions.len(), 1);
            assert_eq!(
                manifest.entities.len(),
                1,
                "{enabled:?}: no entity but the one"
            );
            let start = &manifest.actions[0];
            assert_eq!(start.name.as_str(), "cua.run.start");
            assert_eq!(start.effect, Effect::Read);
            assert_eq!(start.lasting, Lasting::No);
            let goal = start
                .params
                .iter()
                .find(|p| p.name.as_str() == "goal")
                .expect("goal");
            assert_eq!(goal.sink, ArgSink::Body);
        }
    }

    #[test]
    fn a_refused_start_tells_the_planner_why_except_a_busy_window() {
        for why in ControlRefusal::ALL {
            let words = words_of(why, Enabled::On);
            let refused = refusal_of(StartRefusal::Control { why, words });
            match why {
                ControlRefusal::LeaseHeld => assert_eq!(refused, AppRefusal::Busy),
                _ => assert_eq!(
                    refused,
                    AppRefusal::Failed(FailText(words.to_owned())),
                    "{why:?}"
                ),
            }
        }
    }

    #[test]
    fn a_start_the_model_cannot_serve_says_so_in_one_plain_sentence() {
        let words = words_of(ControlRefusal::ModelUnavailable, Enabled::On);
        let refused = refusal_of(StartRefusal::Control {
            why: ControlRefusal::ModelUnavailable,
            words,
        });
        assert_eq!(
            refused,
            AppRefusal::Failed(FailText(
                "The on-screen model isn't available. You can choose one in Settings.".to_owned()
            ))
        );
    }

    #[test]
    fn the_action_is_hidden_from_agents_until_computer_use_is_on() {
        let reach = |enabled| manifest_for(enabled).unwrap().manifest().actions[0].reach;
        assert_eq!(reach(Enabled::Off), AgentReach::Hidden);
        assert_eq!(reach(Enabled::On), AgentReach::Offered);
    }
}

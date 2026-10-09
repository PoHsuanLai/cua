//! `org.quire.Cua`: cuad as an intent provider, with one action, `cua.run.start`.

use cua_bus::ControlRefusal;
use cua_desktop::ToplevelIdent;
use cua_run::Enabled;
use docket_client::IntentProvider;
use docket_core::{
    AgentReach, AppRefusal, EntityRef, Follow, Hit, Invocation, Manifest, ManifestError, Outcome,
    ParamName, Preview, SuggestAsk, UndoFault, UndoToken, Undoable, ValidManifest, Value, validate,
};
use porter_core::AppName;
use prov::{Actor, AgentRef, EntityId, Label, Labelled, RunId, SessionId, SpaceId};
use tokio::sync::{mpsc, oneshot};

/// The shipped manifest (`manifests/org.quire.Cua.toml`).
const MANIFEST: &str = include_str!("../../../manifests/org.quire.Cua.toml");

/// Why the provider could not be built.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
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

/// What `cua.run.start` asked for, as the daemon needs it to make a [`cua_bus::CuaTask`].
#[derive(Debug)]
pub struct StartRequest {
    /// What to do: the planner's words, with their label.
    pub goal: Labelled<String>,
    /// The window to work in, when the planner named one.
    pub window: Option<ToplevelIdent>,
    /// The app, when it named that instead.
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
    pub reply: oneshot::Sender<Result<RunId, ControlRefusal>>,
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
) -> Result<
    (
        StartRequest,
        oneshot::Receiver<Result<RunId, ControlRefusal>>,
    ),
    AppRefusal,
> {
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
    if window.is_none() && app.is_none() {
        return Err(needs("window"));
    }
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

/// What a refused start is in the words of the app's refusals.
fn refusal_of(why: ControlRefusal) -> AppRefusal {
    match why {
        ControlRefusal::LeaseHeld => AppRefusal::Busy,
        ControlRefusal::WindowUnknown | ControlRefusal::BadTask => needs("window"),
        ControlRefusal::NotPermitted
        | ControlRefusal::WrongState
        | ControlRefusal::UnknownQuestion
        | ControlRefusal::Disabled
        | ControlRefusal::ModeUnsupported => AppRefusal::Unsupported,
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
    fn the_action_is_hidden_from_agents_until_computer_use_is_on() {
        let reach = |enabled| manifest_for(enabled).unwrap().manifest().actions[0].reach;
        assert_eq!(reach(Enabled::Off), AgentReach::Hidden);
        assert_eq!(reach(Enabled::On), AgentReach::Offered);
    }
}

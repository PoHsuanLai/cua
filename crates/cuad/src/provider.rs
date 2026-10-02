//! `org.quire.Cua`: cuad as an intent provider, with one action, `cua.run.start`.

use cua_run::Enabled;
use docket_client::IntentProvider;
use docket_core::{
    AgentReach, AppRefusal, EntityRef, Hit, Invocation, Manifest, ManifestError, Outcome, Preview,
    SuggestAsk, UndoFault, UndoToken, ValidManifest, validate,
};
use prov::{Actor, EntityId};

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

/// cuad's provider.
#[derive(Debug)]
pub struct CuaProvider {
    manifest: ValidManifest,
}

impl CuaProvider {
    /// A provider whose action is offered or hidden by `enabled`.
    pub fn new(enabled: Enabled) -> Result<Self, ProviderError> {
        Ok(Self {
            manifest: manifest_for(enabled)?,
        })
    }
}

impl IntentProvider for CuaProvider {
    fn manifest(&self) -> &ValidManifest {
        &self.manifest
    }

    async fn perform(&self, inv: Invocation) -> Result<Outcome, AppRefusal> {
        let _ = inv;
        todo!(
            "cua.run.start: goal, window or app, mode (auto resolved by choose_mode) into a CuaTask; Supervisor::start; Outcome::value is the run id, its report arrives as a Message"
        )
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

    #[test]
    fn the_provider_offers_nothing_but_the_one_action() {
        let provider = CuaProvider::new(Enabled::On).unwrap();
        assert_eq!(provider.manifest().manifest().entities.len(), 1);
    }
}

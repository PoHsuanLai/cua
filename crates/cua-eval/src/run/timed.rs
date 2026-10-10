//! A model with a stopwatch, and what it said.

use cua_action::{CuaAction, WindowSpace};
use cua_run::{CuaModel, CuaModelError, FrameAttachment, StepEvent, StepSink};
use porter_infer::{CuaBegin, CuaStepReply, CuaStepRequest, ServedBy, TokenUsage, TreeText};
use std::time::Instant;

/// A model with a stopwatch: how long each step took it.
#[derive(Debug)]
pub struct Timed<M> {
    inner: M,
    pub(super) took: Vec<u64>,
    pub(super) said: String,
    pub(super) moves: Vec<String>,
    pub(super) thoughts: Vec<String>,
}

impl<M> Timed<M> {
    /// Wraps `inner`.
    pub fn new(inner: M) -> Timed<M> {
        Timed {
            inner,
            took: Vec::new(),
            said: String::new(),
            moves: Vec::new(),
            thoughts: Vec::new(),
        }
    }
}

impl<M: CuaModel> CuaModel for Timed<M> {
    async fn begin(&mut self, begin: CuaBegin) -> Result<(), CuaModelError> {
        self.inner.begin(begin).await
    }

    async fn step(
        &mut self,
        mut request: CuaStepRequest,
        frame: FrameAttachment,
        sink: &mut impl StepSink,
    ) -> Result<CuaStepReply, CuaModelError> {
        // The eval's models read pixels only: the tree is for the run's own rules.
        request.tree = TreeText::Absent;
        let began = Instant::now();
        let mut heard = String::new();
        let reply = self
            .inner
            .step(request, frame, &mut Tap(sink, &mut heard))
            .await;
        self.took
            .push(u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX));
        if let Err(error) = &reply {
            self.thoughts
                .push(format!("[model failed: {error}] {}", brief(Some(&heard))));
        }
        if let Ok(reply) = &reply {
            self.said.push_str(&finish_words(&reply.actions));
            self.moves
                .extend(reply.actions.iter().map(crate::script::line_of));
            let dropped: Vec<String> = reply
                .dropped
                .iter()
                .map(|d| format!("{} ({:?})", d.verb, d.reason))
                .collect();
            let said = brief(Some(&heard));
            self.thoughts.push(match dropped.is_empty() {
                true => said,
                false => format!("[dropped: {}] {said}", dropped.join(", ")),
            });
        }
        reply
    }

    fn usage(&self) -> TokenUsage {
        self.inner.usage()
    }

    fn served(&self) -> Option<ServedBy> {
        self.inner.served()
    }
}

/// A sink that keeps what the model says while it passes it on.
struct Tap<'a, S>(&'a mut S, &'a mut String);

impl<S: StepSink> StepSink for Tap<'_, S> {
    fn event(&mut self, event: StepEvent) {
        if let StepEvent::ThoughtDelta(text) = &event {
            self.1.push_str(&text.0);
        }
        self.0.event(event);
    }
}

/// The first words of a thought, one line: enough to see what a model made of the screen. The
/// screens here are made up, so the words are kept (a real run never keeps them).
fn brief(thought: Option<&str>) -> String {
    let line: String = thought
        .unwrap_or("")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    line.chars().take(400).collect()
}

/// What a reply says in its `Finish` actions: the summary and the values it extracted.
fn finish_words(actions: &[CuaAction<WindowSpace>]) -> String {
    let mut words = String::new();
    for action in actions {
        if let CuaAction::Finish {
            summary, extracted, ..
        } = action
        {
            words.push_str(summary.as_str());
            for e in extracted {
                words.push(' ');
                words.push_str(e.value.as_str());
            }
        }
    }
    words
}

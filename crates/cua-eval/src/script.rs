//! The moves of a script, written one to a line, and the model that plays them. The same lines
//! are how a live run's actions are kept (the cassette), so a recorded run replays as a script.
//!
//! ```text
//! click 120 80        click @compose       double @row1      right 5 5
//! type hello world    key Enter            key Control+a
//! scroll 480 300 down 3                    scroll @list up 1
//! wait 500            ask Which account?   finish done Sent it.
//! ```
//!
//! `@id` is the middle of the item of that id on the screen that shows when the move is made.

use crate::stage::Stage;
use cua_action::{
    Button, Chord, ClickCount, Coord, CuaAction, FinishOutcome, Modifier, Notches, Point, Repeat,
    ScrollBy, ScrollDir, Summary, Target, TypedText, WaitMs, WindowSpace,
};
use cua_run::{CuaModel, CuaModelError, FrameAttachment, StepSink};
use keyboard_types::Key;
use porter_core::Tokens;
use porter_infer::{CuaBegin, CuaStepReply, CuaStepRequest, ServedBy, TokenUsage};
use std::collections::{BTreeSet, VecDeque};
use std::str::FromStr;
use std::sync::Arc;

/// Why a line is not a move.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MoveError {
    /// The verb is not one of the moves.
    #[error("no move called {0}")]
    Verb(String),
    /// A move is missing something or has something it cannot use.
    #[error("{0}: {1}")]
    Argument(&'static str, String),
    /// An `@id` that is not on the screen.
    #[error("nothing called {0} on the screen")]
    NoItem(String),
}

/// Where a pointer move lands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Where {
    /// A pixel of the window.
    Pixel(u32, u32),
    /// The middle of an item.
    Item(String),
}

/// One move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Move {
    /// A click.
    Click(Where, Button, ClickCount),
    /// Text typed into the field with the focus.
    Type(String),
    /// A key or a chord.
    Key(String),
    /// The wheel.
    Scroll(Where, ScrollDir, u16),
    /// Waiting.
    Wait(u32),
    /// A question for the person.
    Ask(String),
    /// The end.
    Finish(FinishOutcome, String),
}

fn number(text: &str, what: &'static str) -> Result<u32, MoveError> {
    text.parse()
        .map_err(|_| MoveError::Argument(what, text.to_owned()))
}

fn place(words: &mut std::slice::Iter<'_, &str>, what: &'static str) -> Result<Where, MoveError> {
    let first = words
        .next()
        .ok_or_else(|| MoveError::Argument(what, "where?".into()))?;
    if let Some(id) = first.strip_prefix('@') {
        return Ok(Where::Item(id.to_owned()));
    }
    let second = words
        .next()
        .ok_or_else(|| MoveError::Argument(what, "y?".into()))?;
    Ok(Where::Pixel(number(first, what)?, number(second, what)?))
}

impl FromStr for Move {
    type Err = MoveError;

    fn from_str(line: &str) -> Result<Move, MoveError> {
        let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
        let words: Vec<&str> = rest.split_whitespace().collect();
        let mut it = words.iter();
        match verb {
            "click" => Ok(Move::Click(
                place(&mut it, "click")?,
                Button::Left,
                ClickCount::One,
            )),
            "double" => Ok(Move::Click(
                place(&mut it, "double")?,
                Button::Left,
                ClickCount::Two,
            )),
            "right" => Ok(Move::Click(
                place(&mut it, "right")?,
                Button::Right,
                ClickCount::One,
            )),
            "type" => Ok(Move::Type(rest.to_owned())),
            "key" => Ok(Move::Key(rest.trim().to_owned())),
            "scroll" => {
                let at = place(&mut it, "scroll")?;
                let dir = match it.next().copied() {
                    Some("up") => ScrollDir::Up,
                    Some("down") => ScrollDir::Down,
                    other => return Err(MoveError::Argument("scroll", format!("{other:?}"))),
                };
                let notches = it.next().map_or(Ok(1), |n| number(n, "scroll"))?;
                Ok(Move::Scroll(
                    at,
                    dir,
                    u16::try_from(notches).unwrap_or(u16::MAX),
                ))
            }
            "wait" => Ok(Move::Wait(number(rest.trim(), "wait")?)),
            "ask" => Ok(Move::Ask(rest.to_owned())),
            "finish" => {
                let (how, summary) = rest.split_once(' ').unwrap_or((rest, ""));
                let outcome = match how {
                    "done" => FinishOutcome::Done,
                    "failed" => FinishOutcome::Failed,
                    "infeasible" => FinishOutcome::Infeasible,
                    other => return Err(MoveError::Argument("finish", other.to_owned())),
                };
                Ok(Move::Finish(outcome, summary.to_owned()))
            }
            other => Err(MoveError::Verb(other.to_owned())),
        }
    }
}

impl Move {
    /// The action this move is on the screen that shows now.
    pub fn resolve(&self, stage: &Stage) -> Result<CuaAction<WindowSpace>, MoveError> {
        let target = |at: &Where| -> Result<Target<WindowSpace>, MoveError> {
            let [x, y] = match at {
                Where::Pixel(x, y) => [*x, *y],
                Where::Item(id) => stage
                    .centre_of(id)
                    .ok_or_else(|| MoveError::NoItem(id.clone()))?,
            };
            Ok(Target::Point(Point::new(Coord(x), Coord(y))))
        };
        let bad = |what: &'static str, e: String| MoveError::Argument(what, e);
        Ok(match self {
            Move::Click(at, button, count) => CuaAction::Click {
                at: target(at)?,
                button: *button,
                count: *count,
                mods: BTreeSet::new(),
            },
            Move::Type(text) => CuaAction::Type {
                text: TypedText::new(text.replace("\\n", "\n"))
                    .map_err(|e| bad("type", e.to_string()))?,
            },
            Move::Key(text) => CuaAction::Key {
                chord: chord_of(text)?,
                repeat: Repeat::new(1).map_err(|e| bad("key", e.to_string()))?,
            },
            Move::Scroll(at, dir, n) => CuaAction::Scroll {
                at: target(at)?,
                dir: *dir,
                by: ScrollBy::Notches(Notches(*n)),
            },
            Move::Wait(ms) => CuaAction::Wait {
                for_ms: WaitMs::new(*ms).map_err(|e| bad("wait", e.to_string()))?,
            },
            Move::Ask(question) => CuaAction::Ask {
                question: Summary::new(question).map_err(|e| bad("ask", e.to_string()))?,
                choices: vec![],
            },
            Move::Finish(outcome, summary) => CuaAction::Finish {
                outcome: *outcome,
                summary: Summary::new(summary).map_err(|e| bad("finish", e.to_string()))?,
                extracted: vec![],
            },
        })
    }
}

fn chord_of(text: &str) -> Result<Chord, MoveError> {
    let mut parts: Vec<&str> = text.split('+').collect();
    let key = parts.pop().unwrap_or("");
    let mods = parts
        .into_iter()
        .map(|m| match m.to_lowercase().as_str() {
            "control" | "ctrl" => Ok(Modifier::Ctrl),
            "alt" => Ok(Modifier::Alt),
            "shift" => Ok(Modifier::Shift),
            // The Command key reaches a window as Super, whatever a script calls it.
            "super" | "meta" | "cmd" | "command" => Ok(Modifier::Super),
            _ => Err(MoveError::Argument("key", m.to_owned())),
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let key = Key::from_str(key).map_err(|_| MoveError::Argument("key", key.to_owned()))?;
    Chord::new(mods, key).map_err(|e| MoveError::Argument("key", e.to_string()))
}

/// The line that says `action`, with every point as a pixel: how a live run is kept.
pub fn line_of(action: &CuaAction<WindowSpace>) -> String {
    let at = |t: &Target<WindowSpace>| match t {
        Target::Point(p) => format!("{} {}", p.x.0, p.y.0),
        Target::Node(_) | Target::Centre => "0 0".to_owned(),
    };
    match action {
        CuaAction::Click {
            at: t,
            button,
            count,
            ..
        } => {
            let verb = match (button, count) {
                (Button::Right, _) => "right",
                (_, ClickCount::One) => "click",
                _ => "double",
            };
            format!("{verb} {}", at(t))
        }
        CuaAction::Type { text } => format!("type {}", text.as_str().replace('\n', "\\n")),
        CuaAction::Key { chord, .. } => {
            let mods: Vec<String> = chord.mods.iter().map(|m| format!("{m:?}+")).collect();
            format!("key {}{}", mods.concat(), chord.key)
        }
        CuaAction::Scroll { at: t, dir, by } => {
            let n = match by {
                ScrollBy::Notches(n) => n.0,
                ScrollBy::Distance(d) => u16::try_from(d.coord().0.div_ceil(40)).unwrap_or(1),
            };
            let way = if *dir == ScrollDir::Up { "up" } else { "down" };
            format!("scroll {} {way} {n}", at(t))
        }
        CuaAction::Wait { for_ms } => format!("wait {}", for_ms.ms()),
        CuaAction::Ask { question, .. } => format!("ask {}", question.as_str()),
        CuaAction::Finish {
            outcome, summary, ..
        } => {
            let how = match outcome {
                FinishOutcome::Done => "done",
                FinishOutcome::Failed => "failed",
                FinishOutcome::Infeasible => "infeasible",
            };
            format!("finish {how} {}", summary.as_str())
        }
        CuaAction::MoveTo { .. }
        | CuaAction::Drag { .. }
        | CuaAction::Zoom { .. }
        | CuaAction::Observe => "wait 0".to_owned(),
    }
}

/// A model that plays a script, one move a step.
#[derive(Debug)]
pub struct ScriptModel {
    stage: Arc<Stage>,
    moves: VecDeque<Move>,
    served: Option<ServedBy>,
}

impl ScriptModel {
    /// A model that plays `lines` on `stage`.
    pub fn new(stage: Arc<Stage>, lines: &[String]) -> Result<ScriptModel, MoveError> {
        let moves = lines
            .iter()
            .map(|l| l.parse())
            .collect::<Result<VecDeque<Move>, _>>()?;
        Ok(ScriptModel {
            stage,
            moves,
            served: None,
        })
    }

    /// Says it is served by `served` (a cloud model, for the spend cap).
    pub fn served_by(self, served: ServedBy) -> ScriptModel {
        ScriptModel {
            served: Some(served),
            ..self
        }
    }
}

impl CuaModel for ScriptModel {
    async fn begin(&mut self, _begin: CuaBegin) -> Result<(), CuaModelError> {
        Ok(())
    }

    async fn step(
        &mut self,
        _request: CuaStepRequest,
        _frame: FrameAttachment,
        _sink: &mut impl StepSink,
    ) -> Result<CuaStepReply, CuaModelError> {
        let next = self.moves.pop_front().ok_or(CuaModelError::Fatal)?;
        let action = next
            .resolve(&self.stage)
            .map_err(|_| CuaModelError::Fatal)?;
        Ok(CuaStepReply {
            thought: None,
            actions: vec![action],
            dropped: vec![],
            safety: vec![],
        })
    }

    fn usage(&self) -> TokenUsage {
        TokenUsage {
            input: Tokens(0),
            output: Tokens(0),
            cached: Tokens(0),
        }
    }

    fn served(&self) -> Option<ServedBy> {
        self.served.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn super_meta_and_cmd_are_one_modifier_and_ctrl_stays_ctrl() {
        let command = BTreeSet::from([Modifier::Super]);
        let ctrl = BTreeSet::from([Modifier::Ctrl]);
        let cases = [
            ("Super+c", &command),
            ("cmd+c", &command),
            ("Command+c", &command),
            ("META+c", &command),
            ("ctrl+c", &ctrl),
            ("Control+c", &ctrl),
        ];
        for (text, mods) in cases {
            let chord = chord_of(text).unwrap_or_else(|e| panic!("{text}: {e}"));
            assert_eq!(&chord.mods, mods, "{text}");
        }
    }
}

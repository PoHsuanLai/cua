//! A stoker turn as an inferd chat request: the inverse of what `porter-bridge` does inside inferd.
//! The screenshots go inline (base64 in the frame), the sampling is the entry's or the provider's
//! as the caller chooses, and nothing here names a model: inferd routes by the tier.

use model_provider as sp;
use porter_capability::Tier;
use porter_core::DataClass;
use porter_core::consent::Usage;
use porter_ids::{Count, Permille, Tokens};
use porter_infer as pi;
use vision_prep::MediaType;

/// Whose sampling a turn carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplingFrom {
    /// The catalogue entry's (greedy for `--thinking off`): sent to the provider.
    Entry,
    /// None: the provider's own defaults (a model that takes no temperature).
    Provider,
}

/// Why a turn could not be written as a chat request.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RequestFault {
    /// A tool name or a piece of JSON the wire refuses.
    #[error("not representable on the inference wire: {0}")]
    Wire(String),
    /// A shape of reply only a local engine can enforce.
    #[error("a reply shape only a local engine enforces")]
    Shape,
}

fn wire<E: std::fmt::Display>(error: E) -> RequestFault {
    RequestFault::Wire(error.to_string())
}

fn json(text: &sp::JsonText) -> Result<pi::JsonText, RequestFault> {
    pi::JsonText::parse(text.as_str()).map_err(wire)
}

fn name(name: &sp::ToolName) -> Result<pi::ToolName, RequestFault> {
    pi::ToolName::parse(name.as_str()).map_err(wire)
}

fn media(media: MediaType) -> &'static str {
    match media {
        MediaType::Png => "image/png",
        MediaType::Jpeg => "image/jpeg",
    }
}

fn seal(seal: &sp::ThoughtSeal) -> pi::ThoughtSeal {
    match seal {
        sp::ThoughtSeal::None => pi::ThoughtSeal::None,
        sp::ThoughtSeal::Signed(text) => pi::ThoughtSeal::Signed(pi::SignatureText(text.0.clone())),
        sp::ThoughtSeal::Redacted(text) => {
            pi::ThoughtSeal::Redacted(pi::OpaqueText(text.0.clone()))
        }
    }
}

fn part(one: &sp::Part) -> Result<pi::MessagePart, RequestFault> {
    Ok(match one {
        sp::Part::Text(text) => pi::MessagePart::Text(text.clone()),
        sp::Part::Image(image) => pi::MessagePart::Image(pi::ImagePart {
            media_type: media(image.media).to_owned(),
            source: pi::ImageSource::Inline(pi::Base64Bytes(image.bytes.0.clone())),
        }),
        sp::Part::Thought { text, seal: sealed } => pi::MessagePart::Thought(pi::ThoughtPart {
            text: text.clone(),
            seal: seal(sealed),
        }),
        sp::Part::ToolCall(call) => pi::MessagePart::ToolCall(pi::ToolCallPart {
            id: pi::ToolCallId(call.id.0.clone()),
            name: name(&call.name)?,
            args: json(&call.input)?,
        }),
        sp::Part::ToolResult(result) => pi::MessagePart::ToolResult(pi::ToolResultPart {
            id: pi::ToolCallId(result.id.0.clone()),
            status: match result.status {
                sp::ToolStatus::Ok => pi::ToolStatus::Ok,
                sp::ToolStatus::Error => pi::ToolStatus::Error,
            },
            parts: result
                .parts
                .iter()
                .map(part)
                .collect::<Result<_, RequestFault>>()?,
        }),
        _ => {
            return Err(RequestFault::Wire(
                "a part the engine can't carry".to_owned(),
            ));
        }
    })
}

/// A tool result rides in a user message on the inference wire (it has no tool role).
fn role(role: sp::Role) -> pi::Role {
    match role {
        sp::Role::System => pi::Role::System,
        sp::Role::User | sp::Role::Tool => pi::Role::User,
        sp::Role::Assistant => pi::Role::Assistant,
    }
}

fn tool(spec: &sp::ToolSpec) -> Result<pi::ToolDecl, RequestFault> {
    match spec {
        sp::ToolSpec::Function {
            name: tool_name,
            description,
            parameters,
        } => Ok(pi::ToolDecl {
            name: name(tool_name)?,
            description: description.clone(),
            params: pi::JsonSchemaText(json(&parameters.0)?),
        }),
        sp::ToolSpec::Native(_) => Err(RequestFault::Wire("a vendor's own tool".to_owned())),
        _ => Err(RequestFault::Wire(
            "a tool the engine can't carry".to_owned(),
        )),
    }
}

fn choice(choice: &sp::ToolChoice) -> Result<pi::ToolChoice, RequestFault> {
    Ok(match choice {
        sp::ToolChoice::Auto => pi::ToolChoice::Auto,
        sp::ToolChoice::Never => pi::ToolChoice::Never,
        sp::ToolChoice::Required => pi::ToolChoice::Required,
        sp::ToolChoice::Named(tool_name) => pi::ToolChoice::Named(name(tool_name)?),
        _ => {
            return Err(RequestFault::Wire(
                "a tool choice the engine can't carry".to_owned(),
            ));
        }
    })
}

fn knob<T, U>(knob: sp::Knob<T>, map: impl Fn(T) -> U) -> pi::Knob<U> {
    match knob {
        sp::Knob::Off => pi::Knob::Off,
        sp::Knob::Set(value) => pi::Knob::Set(map(value)),
    }
}

fn permille(value: sp::Milli) -> Permille {
    Permille(u32::from(value.0))
}

fn sampling(sampling: &sp::Sampling) -> pi::Sampling {
    pi::Sampling {
        temperature: permille(sampling.temperature),
        top_p: knob(sampling.top_p, permille),
        top_k: knob(sampling.top_k, |count| Count(count.0)),
        min_p: knob(sampling.min_p, permille),
        seed: knob(sampling.seed, |seed| pi::Seed(seed.0)),
    }
}

fn reasoning(reasoning: sp::Reasoning) -> pi::Reasoning {
    let effort = |effort| match effort {
        sp::Effort::Low => pi::Effort::Low,
        sp::Effort::Medium => pi::Effort::Medium,
        sp::Effort::High => pi::Effort::High,
    };
    match reasoning {
        sp::Reasoning::EngineDefault => pi::Reasoning::EngineDefault,
        sp::Reasoning::Off => pi::Reasoning::Off,
        sp::Reasoning::On(level) => pi::Reasoning::On(effort(level)),
        _ => pi::Reasoning::EngineDefault,
    }
}

/// The chat request for `turn`, asked at `tier` for screen data.
pub fn chat(
    turn: &sp::TurnRequest,
    tier: Tier,
    from: SamplingFrom,
) -> Result<pi::ChatRequest, RequestFault> {
    let shape = match &turn.output {
        sp::OutputShape::Free => pi::ReplyShape::Text,
        _ => return Err(RequestFault::Shape),
    };
    let messages: Vec<pi::ChatMessage> = turn
        .messages
        .iter()
        .map(|message| {
            Ok(pi::ChatMessage {
                role: role(message.role),
                parts: message
                    .parts
                    .iter()
                    .map(part)
                    .collect::<Result<_, RequestFault>>()?,
            })
        })
        .collect::<Result<_, RequestFault>>()?;
    let tools: Vec<pi::ToolDecl> = turn
        .tools
        .iter()
        .map(tool)
        .collect::<Result<_, RequestFault>>()?;
    let control = pi::ChatControl::new()
        .with_tool_choice(choice(&turn.tool_choice)?)
        .with_tool_calls(match turn.tool_calls {
            sp::ToolParallelism::One => pi::ToolParallelism::One,
            sp::ToolParallelism::Many => pi::ToolParallelism::Many,
        })
        .with_max_output(pi::Knob::Set(Tokens(turn.limits.max_output.0)))
        .with_reasoning(reasoning(turn.reasoning))
        .with_sampling(match from {
            SamplingFrom::Entry => pi::Knob::Set(sampling(&turn.sampling)),
            SamplingFrom::Provider => pi::Knob::Off,
        })
        .with_stop(turn.limits.stop.clone());
    Ok(
        pi::ChatRequest::new(messages, tier, DataClass::Screen, Usage::Interactive)
            .with_shape(shape)
            .with_tools(tools)
            .with_control(control),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(output: sp::OutputShape) -> sp::TurnRequest {
        sp::TurnRequest {
            model: sp::ModelName("m".to_owned()),
            messages: vec![sp::Message {
                role: sp::Role::User,
                parts: vec![
                    sp::Part::Text("click the button".to_owned()),
                    sp::Part::Image(sp::ImageInput {
                        media: MediaType::Png,
                        bytes: sp::ImageBytes(vec![1, 2, 3]),
                        detail: sp::ImageDetail::Auto,
                    }),
                ],
            }],
            tools: vec![],
            tool_choice: sp::ToolChoice::Auto,
            tool_calls: sp::ToolParallelism::One,
            choice_scores: sp::ChoiceScores::Off,
            output,
            limits: sp::Limits {
                max_output: sp::Tokens(512),
                stop: vec![],
                deadline: None,
            },
            sampling: sp::Sampling {
                temperature: sp::Milli(0),
                top_p: sp::Knob::Off,
                top_k: sp::Knob::Off,
                min_p: sp::Knob::Off,
                repeat_penalty: sp::Knob::Off,
                seed: sp::Knob::Off,
            },
            reasoning: sp::Reasoning::Off,
            engine: sp::EngineExtras::None,
        }
    }

    #[test]
    fn a_screenshot_goes_inline_and_the_class_is_screen() {
        let request = chat(
            &turn(sp::OutputShape::Free),
            Tier::Balanced,
            SamplingFrom::Entry,
        )
        .expect("request");
        assert_eq!(request.class, DataClass::Screen);
        assert!(matches!(
            &request.messages[0].parts[1],
            pi::MessagePart::Image(pi::ImagePart {
                source: pi::ImageSource::Inline(_),
                ..
            })
        ));
        assert!(matches!(request.control.sampling, pi::Knob::Set(_)));
        assert_eq!(request.control.max_output, pi::Knob::Set(Tokens(512)));
    }

    #[test]
    fn the_provider_can_choose_the_sampling() {
        let request = chat(
            &turn(sp::OutputShape::Free),
            Tier::Fast,
            SamplingFrom::Provider,
        )
        .expect("request");
        assert_eq!(request.control.sampling, pi::Knob::Off);
    }

    #[test]
    fn a_local_only_shape_is_refused() {
        let shaped = turn(sp::OutputShape::Regex("a".to_owned()));
        assert_eq!(
            chat(&shaped, Tier::Fast, SamplingFrom::Entry),
            Err(RequestFault::Shape)
        );
    }
}

//! Chat request 参数 -> Anthropic 参数。

use serde_json::{Map, Value, json};

use crate::registry::Plan;
use crate::report::{Error, Report, Side, WarningCode};
use crate::wire::openai_chat::{ChatRequest, ChatToolChoice};

pub(super) fn convert_tool_choice(
    choice: Option<&ChatToolChoice>,
    report: &mut Report,
    plan: &Plan,
) -> Result<Option<Value>, Error> {
    let Some(choice) = choice else {
        return Ok(None);
    };
    match choice {
        ChatToolChoice::Mode(mode) => match mode.as_str() {
            "auto" => Ok(Some(json!({"type": "auto"}))),
            "none" => Ok(Some(json!({"type": "none"}))),
            "required" => Ok(Some(json!({"type": "any"}))),
            other => {
                report.unsupported(
                    plan.policy,
                    Side::Source,
                    "/tool_choice",
                    format!("unknown Chat tool_choice mode: {other}"),
                )?;
                Ok(None)
            }
        },
        ChatToolChoice::Object(object) => {
            let type_ = object.get("type").and_then(Value::as_str).unwrap_or("");
            if type_ == "function"
                && let Some(name) = object
                    .get("function")
                    .and_then(|value| value.get("name"))
                    .and_then(Value::as_str)
            {
                return Ok(Some(json!({"type": "tool", "name": name})));
            }
            report.unsupported(
                plan.policy,
                Side::Source,
                "/tool_choice",
                "Chat tool_choice object has no Anthropic equivalent",
            )?;
            Ok(None)
        }
    }
}

pub(super) fn reject_chat_only_fields(
    req: &ChatRequest,
    report: &mut Report,
    plan: &Plan,
) -> Result<(), Error> {
    if req.n.is_some() {
        report.unsupported(plan.policy, Side::Source, "/n", "Anthropic has no n")?;
    }
    if req.seed.is_some() {
        report.unsupported(plan.policy, Side::Source, "/seed", "Anthropic has no seed")?;
    }
    if req.presence_penalty.is_some() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/presence_penalty",
            "Anthropic has no presence_penalty",
        )?;
    }
    if req.frequency_penalty.is_some() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/frequency_penalty",
            "Anthropic has no frequency_penalty",
        )?;
    }
    if req.logprobs.is_some() || req.top_logprobs.is_some() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/logprobs",
            "Anthropic has no logprobs",
        )?;
    }
    if req.parallel_tool_calls.is_some() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/parallel_tool_calls",
            "Anthropic has no parallel_tool_calls",
        )?;
    }
    if req.stream_options.is_some() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/stream_options",
            "Anthropic has no stream_options",
        )?;
    }
    if req.user.is_some() {
        report.unsupported(
            plan.policy,
            Side::Source,
            "/user",
            "Anthropic has no user field",
        )?;
    }
    Ok(())
}

pub(super) fn warn_unknown(extra: &Map<String, Value>, report: &mut Report, pointer_prefix: &str) {
    for key in extra.keys() {
        report.warn(
            WarningCode::UnknownField,
            Side::Source,
            format!("{pointer_prefix}{key}"),
            "unknown Chat field is not mapped to Anthropic",
        );
    }
}

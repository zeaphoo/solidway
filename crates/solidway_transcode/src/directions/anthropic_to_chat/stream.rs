//! Chat SSE -> Anthropic SSE。

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::common::frame::{InFrame, OutFrame};
use crate::registry::{Plan, StreamBridge};
use crate::report::{Error, Report, Side, WarningCode};
use crate::wire::anthropic::AnthropicUsage;
use crate::wire::openai_chat::{ChatChunk, ChatError, ChatUsage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Text,
    Thinking,
}

#[derive(Debug, Default)]
struct PendingTool {
    id: String,
    name: String,
    arguments: Vec<String>,
}

pub(super) struct Bridge {
    report: Report,
    id: String,
    model: String,
    started: bool,
    done: bool,
    next_index: u32,
    open: Option<(u32, BlockKind)>,
    tools: BTreeMap<u32, PendingTool>,
    finish_reason: Option<String>,
    usage: Option<AnthropicUsage>,
    choice_warned: bool,
    service_tier_warned: bool,
    system_fingerprint_warned: bool,
    refusal_warned: bool,
    tool_type_warned: bool,
    extras_warned: bool,
}

impl Bridge {
    pub(super) fn new(_plan: &Plan) -> Self {
        Self {
            report: Report::default(),
            id: "msg_transcoded".to_string(),
            model: String::new(),
            started: false,
            done: false,
            next_index: 0,
            open: None,
            tools: BTreeMap::new(),
            finish_reason: None,
            usage: None,
            choice_warned: false,
            service_tier_warned: false,
            system_fingerprint_warned: false,
            refusal_warned: false,
            tool_type_warned: false,
            extras_warned: false,
        }
    }

    fn event(&self, event_type: &str, value: Value) -> Result<OutFrame, Error> {
        Ok(OutFrame::Sse {
            event: Some(event_type.to_string()),
            id: None,
            data: serde_json::to_vec(&value)?,
        })
    }

    fn ensure_started(&mut self, out: &mut Vec<OutFrame>) -> Result<(), Error> {
        if self.started {
            return Ok(());
        }
        self.started = true;
        let input_tokens = self
            .usage
            .as_ref()
            .and_then(|usage| usage.input_tokens)
            .unwrap_or(0);
        out.push(self.event(
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": self.id,
                    "type": "message",
                    "role": "assistant",
                    "content": [],
                    "model": self.model,
                    "stop_reason": null,
                    "stop_sequence": null,
                    "usage": {
                        "input_tokens": input_tokens,
                        "output_tokens": 0
                    }
                }
            }),
        )?);
        Ok(())
    }

    fn close_open(&mut self, out: &mut Vec<OutFrame>) -> Result<(), Error> {
        if let Some((index, _)) = self.open.take() {
            out.push(self.event(
                "content_block_stop",
                json!({"type": "content_block_stop", "index": index}),
            )?);
        }
        Ok(())
    }

    fn open_block(&mut self, kind: BlockKind, out: &mut Vec<OutFrame>) -> Result<u32, Error> {
        self.close_open(out)?;
        let index = self.next_index;
        self.next_index += 1;
        self.open = Some((index, kind));
        let content_block = match kind {
            BlockKind::Text => json!({"type": "text", "text": ""}),
            BlockKind::Thinking => json!({"type": "thinking", "thinking": "", "signature": ""}),
        };
        out.push(self.event(
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": index,
                "content_block": content_block
            }),
        )?);
        Ok(index)
    }

    fn ensure_text_block(&mut self, out: &mut Vec<OutFrame>) -> Result<u32, Error> {
        if let Some((index, BlockKind::Text)) = self.open {
            return Ok(index);
        }
        self.open_block(BlockKind::Text, out)
    }

    fn ensure_thinking_block(&mut self, out: &mut Vec<OutFrame>) -> Result<u32, Error> {
        if let Some((index, BlockKind::Thinking)) = self.open {
            return Ok(index);
        }
        self.open_block(BlockKind::Thinking, out)
    }

    fn emit_tool_block(
        &mut self,
        chat_index: u32,
        tool: PendingTool,
        out: &mut Vec<OutFrame>,
    ) -> Result<(), Error> {
        let index = self.next_index;
        self.next_index += 1;
        let id = if tool.id.is_empty() {
            format!("toolu_{chat_index}")
        } else {
            tool.id
        };
        out.push(self.event(
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": index,
                "content_block": {
                    "type": "tool_use",
                    "id": id,
                    "name": tool.name,
                    "input": {}
                }
            }),
        )?);
        if tool.arguments.is_empty() {
            out.push(self.event(
                "content_block_delta",
                json!({
                    "type": "content_block_delta",
                    "index": index,
                    "delta": {
                        "type": "input_json_delta",
                        "partial_json": "{}"
                    }
                }),
            )?);
        } else {
            for arguments in tool.arguments {
                out.push(self.event(
                    "content_block_delta",
                    json!({
                        "type": "content_block_delta",
                        "index": index,
                        "delta": {
                            "type": "input_json_delta",
                            "partial_json": arguments
                        }
                    }),
                )?);
            }
        }
        out.push(self.event(
            "content_block_stop",
            json!({"type": "content_block_stop", "index": index}),
        )?);
        if tool.name.is_empty() {
            self.report.warn(
                WarningCode::Degraded,
                Side::Source,
                format!("/choices/0/delta/tool_calls/{chat_index}/function/name"),
                "Chat tool call never provided a name; Anthropic tool_use has an empty name",
            );
        }
        Ok(())
    }

    fn finalize(&mut self) -> Result<Vec<OutFrame>, Error> {
        if self.done {
            return Ok(Vec::new());
        }
        self.done = true;
        let mut out = Vec::new();
        self.ensure_started(&mut out)?;

        self.close_open(&mut out)?;
        for (chat_index, tool) in std::mem::take(&mut self.tools) {
            self.emit_tool_block(chat_index, tool, &mut out)?;
        }

        self.close_open(&mut out)?;
        let stop_reason = match self.finish_reason.take() {
            Some(reason) => reason,
            None => {
                self.report.warn(
                    WarningCode::Synthesized,
                    Side::Target,
                    "/delta/stop_reason",
                    "Chat stream ended without finish_reason; using end_turn",
                );
                "end_turn".to_string()
            }
        };
        let output_tokens = self
            .usage
            .as_ref()
            .and_then(|usage| usage.output_tokens)
            .unwrap_or(0);
        out.push(self.event(
            "message_delta",
            json!({
                "type": "message_delta",
                "delta": {"stop_reason": stop_reason, "stop_sequence": null},
                "usage": {"output_tokens": output_tokens}
            }),
        )?);
        out.push(self.event("message_stop", json!({"type": "message_stop"}))?);
        Ok(out)
    }

    fn handle_chunk(&mut self, chunk: ChatChunk) -> Result<Vec<OutFrame>, Error> {
        let mut out = Vec::new();
        self.id = chunk.id;
        self.model = chunk.model;
        if let Some(usage) = chunk.usage.as_ref() {
            self.usage = Some(chat_usage_to_anthropic(usage));
        }
        if chunk.service_tier.is_some() && !self.service_tier_warned {
            self.service_tier_warned = true;
            self.report.degraded(
                Side::Source,
                "/service_tier",
                "Anthropic has no service_tier field",
            );
        }
        if chunk.system_fingerprint.is_some() && !self.system_fingerprint_warned {
            self.system_fingerprint_warned = true;
            self.report.degraded(
                Side::Source,
                "/system_fingerprint",
                "Anthropic has no system_fingerprint field",
            );
        }
        if chunk.choices.len() > 1 && !self.choice_warned {
            self.choice_warned = true;
            self.report.degraded(
                Side::Source,
                "/choices",
                "Anthropic streams one message; extra Chat choices are dropped",
            );
        }
        self.ensure_started(&mut out)?;

        if let Some(choice) = chunk.choices.first() {
            if (!chunk.extra.is_empty()
                || !choice.extra.is_empty()
                || !choice.delta.extra.is_empty())
                && !self.extras_warned
            {
                self.extras_warned = true;
                self.report.degraded(
                    Side::Source,
                    "/extra",
                    "unknown Chat stream fields are not mapped to Anthropic",
                );
            }
            if let Some(reasoning) = choice
                .delta
                .reasoning_content
                .as_deref()
                .or(choice.delta.reasoning.as_deref())
            {
                let index = self.ensure_thinking_block(&mut out)?;
                out.push(self.event(
                    "content_block_delta",
                    json!({
                        "type": "content_block_delta",
                        "index": index,
                        "delta": {"type": "thinking_delta", "thinking": reasoning}
                    }),
                )?);
            }
            if let Some(content) = choice
                .delta
                .content
                .as_deref()
                .filter(|content| !content.is_empty())
            {
                let index = self.ensure_text_block(&mut out)?;
                out.push(self.event(
                    "content_block_delta",
                    json!({
                        "type": "content_block_delta",
                        "index": index,
                        "delta": {"type": "text_delta", "text": content}
                    }),
                )?);
            }
            if let Some(refusal) = choice.delta.refusal.as_deref() {
                if !self.refusal_warned {
                    self.refusal_warned = true;
                    self.report.degraded(
                        Side::Target,
                        "/delta/refusal",
                        "Chat refusal is emitted as an Anthropic text_delta",
                    );
                }
                let index = self.ensure_text_block(&mut out)?;
                out.push(self.event(
                    "content_block_delta",
                    json!({
                        "type": "content_block_delta",
                        "index": index,
                        "delta": {"type": "text_delta", "text": refusal}
                    }),
                )?);
            }
            for call in &choice.delta.tool_calls {
                if call.type_.as_deref().is_some_and(|kind| kind != "function")
                    && !self.tool_type_warned
                {
                    self.tool_type_warned = true;
                    self.report.degraded(
                        Side::Source,
                        "/delta/tool_calls/type",
                        "Chat tool call type other than function is emitted as Anthropic tool_use",
                    );
                }
                let chat_index = call.index.unwrap_or(0);
                let tool = self.tools.entry(chat_index).or_default();
                if let Some(id) = call.id.as_deref().filter(|id| !id.is_empty()) {
                    tool.id = id.to_string();
                }
                if let Some(name) = call
                    .function
                    .name
                    .as_deref()
                    .filter(|name| !name.is_empty())
                {
                    tool.name = name.to_string();
                }
                if !call.function.arguments.is_empty() {
                    tool.arguments.push(call.function.arguments.clone());
                }
            }
            if let Some(finish_reason) = choice.finish_reason.as_deref() {
                self.finish_reason = Some(map_finish_reason(finish_reason, &mut self.report));
            }
        }

        Ok(out)
    }
}

impl StreamBridge for Bridge {
    fn push(&mut self, frame: InFrame) -> Result<Vec<OutFrame>, Error> {
        match frame {
            InFrame::Sse { data, .. } => {
                if data == b"[DONE]" {
                    return self.finalize();
                }
                let value: Value = serde_json::from_slice(&data)?;
                if value.get("error").is_some() {
                    let err: ChatError = serde_json::from_value(value)?;
                    let mut error = json!({
                        "type": "api_error",
                        "message": err.error.message
                    });
                    if let Some(object) = error.as_object_mut() {
                        if let Some(code) = err.error.code {
                            object.insert("code".to_string(), Value::String(code));
                        }
                        if let Some(param) = err.error.param {
                            object.insert("param".to_string(), Value::String(param));
                        }
                        for (key, value) in err.error.extra {
                            object.insert(key, value);
                        }
                    }
                    let out = vec![self.event(
                        "error",
                        json!({
                            "type": "error",
                            "error": error
                        }),
                    )?];
                    self.done = true;
                    return Ok(out);
                }
                let chunk: ChatChunk = serde_json::from_value(value)?;
                self.handle_chunk(chunk)
            }
            InFrame::Eof => self.finalize(),
            InFrame::AwsEventStream { .. } => {
                Err(Error::Unsupported("AWS eventstream input".to_string()))
            }
        }
    }

    fn finish(&mut self) -> Result<Vec<OutFrame>, Error> {
        if !self.done {
            self.report.warn(
                WarningCode::Truncated,
                Side::Source,
                "/",
                "Chat stream ended before finish_reason",
            );
        }
        self.finalize()
    }

    fn report(&self) -> &Report {
        &self.report
    }
}

fn chat_usage_to_anthropic(usage: &ChatUsage) -> AnthropicUsage {
    let cached = usage
        .prompt_tokens_details
        .as_ref()
        .and_then(|details| details.cached_tokens)
        .unwrap_or(0);
    let cache_creation = usage
        .prompt_tokens_details
        .as_ref()
        .and_then(|details| details.cache_write_tokens.or(details.cache_creation_tokens))
        .unwrap_or(0);
    let input = usage
        .prompt_tokens
        .unwrap_or(0)
        .saturating_sub(cached)
        .saturating_sub(cache_creation);
    AnthropicUsage {
        input_tokens: Some(input),
        output_tokens: usage.completion_tokens,
        cache_creation_input_tokens: (cache_creation > 0).then_some(cache_creation),
        cache_read_input_tokens: (cached > 0).then_some(cached),
        service_tier: None,
        cache_creation: None,
        server_tool_use: None,
        extra: Map::new(),
    }
}

fn map_finish_reason(finish_reason: &str, report: &mut Report) -> String {
    match finish_reason {
        "stop" => "end_turn",
        "length" => "max_tokens",
        "tool_calls" => "tool_use",
        "content_filter" => "refusal",
        other => {
            report.warn(
                WarningCode::Degraded,
                Side::Target,
                "/delta/stop_reason",
                format!("unknown Chat finish_reason {other}; using end_turn"),
            );
            "end_turn"
        }
    }
    .to_string()
}

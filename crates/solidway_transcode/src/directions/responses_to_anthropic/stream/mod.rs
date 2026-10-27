//! Anthropic SSE -> Responses SSE。

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::common::frame::{InFrame, OutFrame};
use crate::registry::{Plan, StreamBridge};
use crate::report::{Error, Report, Side, WarningCode};
use crate::wire::anthropic::{AnthropicError, AnthropicEvent, AnthropicUsage};

mod emit;

#[derive(Debug, Default, Clone)]
struct ServerToolState {
    output_index: u32,
    item_id: String,
    call_id: String,
    call_type: String,
    name: String,
    input: String,
    results: Option<Value>,
    done: bool,
}

#[derive(Debug, Default, Clone)]
struct ToolState {
    output_index: u32,
    item_id: String,
    call_id: Option<String>,
    name: Option<String>,
    arguments: String,
}

pub(super) struct Bridge {
    plan: Plan,
    report: Report,
    id: String,
    model: String,
    started: bool,
    done: bool,
    seq: u64,
    message_index: Option<u32>,
    reasoning_index: Option<u32>,
    text: String,
    reasoning: String,
    tools: BTreeMap<u32, ToolState>,
    server_tools: BTreeMap<u32, ServerToolState>,
    result_blocks: BTreeMap<u32, u32>,
    finish_reason: Option<String>,
    usage: Option<AnthropicUsage>,
    signature_warned: bool,
    ping_warned: bool,
    stop_sequence_warned: bool,
    container_warned: bool,
    block_extras_warned: bool,
}

impl Bridge {
    pub(super) fn new(plan: &Plan) -> Self {
        Self {
            plan: *plan,
            report: Report::default(),
            id: "resp_transcoded".to_string(),
            model: String::new(),
            started: false,
            done: false,
            seq: 0,
            message_index: None,
            reasoning_index: None,
            text: String::new(),
            reasoning: String::new(),
            tools: BTreeMap::new(),
            server_tools: BTreeMap::new(),
            result_blocks: BTreeMap::new(),
            finish_reason: None,
            usage: None,
            signature_warned: false,
            ping_warned: false,
            stop_sequence_warned: false,
            container_warned: false,
            block_extras_warned: false,
        }
    }

    fn handle_event(&mut self, event: AnthropicEvent) -> Result<Vec<OutFrame>, Error> {
        let mut out = Vec::new();
        match event.type_.as_str() {
            "message_start" => {
                if let Some(message) = event.message.as_ref() {
                    self.id = message.id.clone();
                    self.model = message.model.clone();
                    if let Some(usage) = message.usage.as_ref() {
                        self.usage = Some(usage.clone());
                    }
                    if message.stop_sequence.is_some() && !self.stop_sequence_warned {
                        self.stop_sequence_warned = true;
                        self.report.degraded(
                            Side::Source,
                            "/message/stop_sequence",
                            "Responses has no stop_sequence field",
                        );
                    }
                    if message.container.is_some() && !self.container_warned {
                        self.container_warned = true;
                        self.report.degraded(
                            Side::Source,
                            "/message/container",
                            "Responses has no container field",
                        );
                    }
                }
                self.ensure_started(&mut out)?;
            }
            "content_block_start" => {
                self.ensure_started(&mut out)?;
                let index = event.index.unwrap_or(0);
                if let Some(block) = event.content_block.as_ref() {
                    if (block.cache_control.is_some()
                        || !block.citations.is_empty()
                        || !block.extra.is_empty())
                        && !self.block_extras_warned
                    {
                        self.block_extras_warned = true;
                        self.report.degraded(
                            Side::Source,
                            "/content_block",
                            "Responses has no cache_control / citations / unknown block fields",
                        );
                    }
                    match block.type_.as_str() {
                        "text" => self.ensure_message_item(index, &mut out)?,
                        "thinking" => self.ensure_reasoning_item(index, &mut out)?,
                        "tool_use" => self.ensure_tool_item(
                            index,
                            block.id.as_deref(),
                            block.name.as_deref(),
                            &mut out,
                        )?,
                        "server_tool_use" => {
                            self.ensure_server_tool_item(index, block, &mut out)?
                        }
                        "web_search_tool_result" | "x_search_tool_result" => {
                            self.handle_server_tool_result(index, block, &mut out)?;
                        }
                        other => self.report.unsupported(
                            self.plan.policy,
                            Side::Target,
                            "/content_block/type",
                            format!("Anthropic block {other} has no Responses equivalent"),
                        )?,
                    }
                }
            }
            "content_block_delta" => {
                self.ensure_started(&mut out)?;
                let index = event.index.unwrap_or(0);
                if let Some(delta) = event.delta.as_ref() {
                    match delta.type_.as_deref() {
                        Some("text_delta") => {
                            if let Some(text) = delta.text.as_deref() {
                                self.text.push_str(text);
                                out.push(self.event(
                                    "response.output_text.delta",
                                    json!({
                                        "type": "response.output_text.delta",
                                        "item_id": format!("{}-message", self.id),
                                        "output_index": index,
                                        "content_index": 0,
                                        "delta": text
                                    }),
                                )?);
                            }
                        }
                        Some("thinking_delta") => {
                            if let Some(thinking) = delta.thinking.as_deref() {
                                self.reasoning.push_str(thinking);
                                out.push(self.event(
                                    "response.reasoning_summary_text.delta",
                                    json!({
                                        "type": "response.reasoning_summary_text.delta",
                                        "item_id": format!("{}-reasoning", self.id),
                                        "output_index": index,
                                        "summary_index": 0,
                                        "delta": thinking
                                    }),
                                )?);
                            }
                        }
                        Some("input_json_delta") => {
                            if let Some(partial) = delta.partial_json.as_deref() {
                                if let Some(state) = self.tools.get_mut(&index) {
                                    state.arguments.push_str(partial);
                                }
                                if let Some(state) = self.server_tools.get_mut(&index) {
                                    state.input.push_str(partial);
                                } else {
                                    out.push(self.event(
                                        "response.function_call_arguments.delta",
                                        json!({
                                            "type": "response.function_call_arguments.delta",
                                            "item_id": format!("{}-call-{}", self.id, index),
                                            "output_index": index,
                                            "delta": partial
                                        }),
                                    )?);
                                }
                            }
                        }
                        Some("signature_delta") => {
                            if !self.signature_warned {
                                self.signature_warned = true;
                                self.report.degraded(
                                    Side::Source,
                                    "/delta/signature",
                                    "Responses reasoning stream has no Anthropic signature event",
                                );
                            }
                        }
                        Some(other) => self.report.unsupported(
                            self.plan.policy,
                            Side::Target,
                            "/delta/type",
                            format!("Anthropic delta {other} has no Responses equivalent"),
                        )?,
                        None => {}
                    }
                }
            }
            "content_block_stop" => {
                if let Some(index) = event.index {
                    self.close_block(index, &mut out)?;
                }
            }
            "message_delta" => {
                if let Some(delta) = event.delta.as_ref() {
                    self.finish_reason = delta.stop_reason.clone();
                    if delta.stop_sequence.is_some() && !self.stop_sequence_warned {
                        self.stop_sequence_warned = true;
                        self.report.degraded(
                            Side::Source,
                            "/delta/stop_sequence",
                            "Responses has no stop_sequence field",
                        );
                    }
                }
                if let Some(usage) = event.usage.as_ref() {
                    self.usage = Some(usage.clone());
                }
            }
            "message_stop" => {
                out.extend(self.finalize()?);
            }
            "error" => {
                let message = event
                    .error
                    .as_ref()
                    .map(|error| error.message.clone())
                    .unwrap_or_else(|| "Anthropic stream error".to_string());
                let mut error = json!({"type": "server_error", "message": message});
                if let Some(source) = event.error.as_ref()
                    && let Some(object) = error.as_object_mut()
                {
                    object.insert("code".to_string(), Value::String(source.type_.clone()));
                    for (key, value) in &source.extra {
                        object.insert(key.clone(), value.clone());
                    }
                }
                out.push(self.event("error", json!({"type": "error", "error": error}))?);
                self.done = true;
            }
            "ping" => {
                if !self.ping_warned {
                    self.ping_warned = true;
                    self.report.degraded(
                        Side::Source,
                        "/ping",
                        "Responses stream has no ping event",
                    );
                }
            }
            other => {
                self.report.unsupported(
                    self.plan.policy,
                    Side::Target,
                    "/type",
                    format!("unknown Anthropic stream event {other}"),
                )?;
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
                if value.get("type").and_then(Value::as_str) == Some("error") {
                    let err: AnthropicError = serde_json::from_value(value)?;
                    let mut error = json!({
                        "type": "server_error",
                        "message": err.error.message
                    });
                    if let Some(object) = error.as_object_mut() {
                        object.insert("code".to_string(), Value::String(err.error.type_));
                        for (key, value) in err.error.extra {
                            object.insert(key, value);
                        }
                    }
                    let out = vec![self.event("error", json!({"type": "error", "error": error}))?];
                    self.done = true;
                    return Ok(out);
                }
                let event: AnthropicEvent = serde_json::from_value(value)?;
                self.handle_event(event)
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
                "Anthropic stream ended before message_stop",
            );
        }
        self.finalize()
    }

    fn report(&self) -> &Report {
        &self.report
    }
}

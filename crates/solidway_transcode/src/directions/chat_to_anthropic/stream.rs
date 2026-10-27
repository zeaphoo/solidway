//! Anthropic SSE -> Chat SSE。

use serde_json::{Map, Value};

use crate::common::frame::{InFrame, OutFrame};
use crate::registry::{Plan, StreamBridge};
use crate::report::{Error, Report, Side, WarningCode};
use crate::wire::anthropic::{AnthropicError, AnthropicEvent, AnthropicUsage};
use crate::wire::openai_chat::{
    ChatChunk, ChatChunkChoice, ChatDelta, ChatError, ChatErrorBody, ChatFunctionCall,
    ChatTokenDetails, ChatToolCall, ChatUsage,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Text,
    Thinking,
    Tool { index: u32 },
}

pub(super) struct Bridge {
    plan: Plan,
    report: Report,
    id: String,
    model: String,
    started: bool,
    done: bool,
    finish_reason: Option<String>,
    usage: Option<ChatUsage>,
    block: Option<BlockKind>,
    start_signature_warned: bool,
    delta_signature_warned: bool,
    stop_sequence_warned: bool,
    container_warned: bool,
    ping_warned: bool,
    block_extras_warned: bool,
}

impl Bridge {
    pub(super) fn new(plan: &Plan) -> Self {
        Self {
            plan: *plan,
            report: Report::default(),
            id: "chatcmpl-transcoded".to_string(),
            model: String::new(),
            started: false,
            done: false,
            finish_reason: None,
            usage: None,
            block: None,
            start_signature_warned: false,
            delta_signature_warned: false,
            stop_sequence_warned: false,
            container_warned: false,
            ping_warned: false,
            block_extras_warned: false,
        }
    }

    fn ensure_started(&mut self, out: &mut Vec<OutFrame>) -> Result<(), Error> {
        if self.started {
            return Ok(());
        }
        self.started = true;
        out.push(self.chunk(
            ChatDelta {
                role: Some("assistant".to_string()),
                content: Some(String::new()),
                ..Default::default()
            },
            None,
            false,
        )?);
        Ok(())
    }

    fn chunk(
        &self,
        delta: ChatDelta,
        finish_reason: Option<String>,
        with_usage: bool,
    ) -> Result<OutFrame, Error> {
        let chunk = ChatChunk {
            id: self.id.clone(),
            object: "chat.completion.chunk".to_string(),
            created: 0,
            model: self.model.clone(),
            choices: vec![ChatChunkChoice {
                index: 0,
                delta,
                finish_reason,
                logprobs: None,
                extra: Map::new(),
            }],
            usage: if with_usage {
                self.usage.clone().map(with_usage_totals)
            } else {
                None
            },
            system_fingerprint: None,
            service_tier: None,
            extra: Map::new(),
        };
        Ok(OutFrame::Sse {
            event: None,
            id: None,
            data: serde_json::to_vec(&chunk)?,
        })
    }

    fn finalize(&mut self) -> Result<Vec<OutFrame>, Error> {
        if self.done {
            return Ok(Vec::new());
        }
        self.done = true;
        let mut out = Vec::new();
        self.ensure_started(&mut out)?;
        let finish_reason = match self.finish_reason.take() {
            Some(reason) => reason,
            None => {
                self.report.warn(
                    WarningCode::Synthesized,
                    Side::Target,
                    "/choices/0/finish_reason",
                    "Anthropic stream ended without stop_reason; using stop",
                );
                "stop".to_string()
            }
        };
        out.push(self.chunk(ChatDelta::default(), Some(finish_reason), true)?);
        out.push(OutFrame::Done);
        Ok(out)
    }

    fn handle_event(&mut self, event: AnthropicEvent) -> Result<Vec<OutFrame>, Error> {
        let mut out = Vec::new();
        match event.type_.as_str() {
            "message_start" => {
                if let Some(message) = event.message.as_ref() {
                    self.id = message.id.clone();
                    self.model = message.model.clone();
                    if let Some(usage) = message.usage.as_ref() {
                        self.usage = Some(usage_to_chat(usage));
                    }
                    if message.stop_sequence.is_some() && !self.stop_sequence_warned {
                        self.stop_sequence_warned = true;
                        self.report.degraded(
                            Side::Source,
                            "/message/stop_sequence",
                            "Chat has no stop_sequence field",
                        );
                    }
                    if message.container.is_some() && !self.container_warned {
                        self.container_warned = true;
                        self.report.degraded(
                            Side::Source,
                            "/message/container",
                            "Chat has no container field",
                        );
                    }
                }
                self.ensure_started(&mut out)?;
            }
            "content_block_start" => {
                self.ensure_started(&mut out)?;
                let Some(block) = event.content_block.as_ref() else {
                    return Ok(out);
                };
                if (block.cache_control.is_some()
                    || !block.citations.is_empty()
                    || !block.extra.is_empty())
                    && !self.block_extras_warned
                {
                    self.block_extras_warned = true;
                    self.report.degraded(
                        Side::Source,
                        "/content_block",
                        "Chat has no cache_control / citations / unknown block fields",
                    );
                }
                match block.type_.as_str() {
                    "text" => {
                        self.block = Some(BlockKind::Text);
                        if let Some(text) = block.text.as_deref().filter(|text| !text.is_empty()) {
                            out.push(self.chunk(
                                ChatDelta {
                                    content: Some(text.to_string()),
                                    ..Default::default()
                                },
                                None,
                                false,
                            )?);
                        }
                    }
                    "thinking" => {
                        self.block = Some(BlockKind::Thinking);
                        if block.signature.is_some() && !self.start_signature_warned {
                            self.start_signature_warned = true;
                            self.report.degraded(
                                Side::Source,
                                "/content_block/signature",
                                "Chat has no thinking signature field",
                            );
                        }
                        if let Some(thinking) =
                            block.thinking.as_deref().filter(|text| !text.is_empty())
                        {
                            out.push(self.chunk(
                                ChatDelta {
                                    reasoning_content: Some(thinking.to_string()),
                                    ..Default::default()
                                },
                                None,
                                false,
                            )?);
                        }
                    }
                    "tool_use" => {
                        let index = event.index.unwrap_or(0);
                        self.block = Some(BlockKind::Tool { index });
                        out.push(self.chunk(
                            ChatDelta {
                                tool_calls: vec![ChatToolCall {
                                    index: Some(index),
                                    id: block.id.clone(),
                                    type_: Some("function".to_string()),
                                    function: ChatFunctionCall {
                                        name: block.name.clone(),
                                        arguments: String::new(),
                                        extra: Map::new(),
                                    },
                                    extra: Map::new(),
                                }],
                                ..Default::default()
                            },
                            None,
                            false,
                        )?);
                    }
                    other => {
                        self.report.unsupported(
                            self.plan.policy,
                            Side::Target,
                            "/content_block/type",
                            format!("Anthropic stream block {other} has no Chat equivalent"),
                        )?;
                    }
                }
            }
            "content_block_delta" => {
                self.ensure_started(&mut out)?;
                let Some(delta) = event.delta.as_ref() else {
                    return Ok(out);
                };
                match delta.type_.as_deref() {
                    Some("text_delta") => {
                        if let Some(text) = delta.text.as_deref() {
                            out.push(self.chunk(
                                ChatDelta {
                                    content: Some(text.to_string()),
                                    ..Default::default()
                                },
                                None,
                                false,
                            )?);
                        }
                    }
                    Some("thinking_delta") => {
                        if let Some(thinking) = delta.thinking.as_deref() {
                            out.push(self.chunk(
                                ChatDelta {
                                    reasoning_content: Some(thinking.to_string()),
                                    ..Default::default()
                                },
                                None,
                                false,
                            )?);
                        }
                    }
                    Some("input_json_delta") => {
                        let index = match self.block {
                            Some(BlockKind::Tool { index }) => index,
                            _ => event.index.unwrap_or(0),
                        };
                        if let Some(partial_json) = delta.partial_json.as_deref() {
                            out.push(self.chunk(
                                ChatDelta {
                                    tool_calls: vec![ChatToolCall {
                                        index: Some(index),
                                        id: None,
                                        type_: None,
                                        function: ChatFunctionCall {
                                            name: None,
                                            arguments: partial_json.to_string(),
                                            extra: Map::new(),
                                        },
                                        extra: Map::new(),
                                    }],
                                    ..Default::default()
                                },
                                None,
                                false,
                            )?);
                        }
                    }
                    Some("signature_delta") => {
                        if !self.delta_signature_warned {
                            self.delta_signature_warned = true;
                            self.report.degraded(
                                Side::Target,
                                "/delta/signature",
                                "Anthropic thinking signature has no Chat field and is dropped",
                            );
                        }
                    }
                    Some(other) => {
                        self.report.unsupported(
                            self.plan.policy,
                            Side::Target,
                            "/delta/type",
                            format!("Anthropic delta {other} has no Chat equivalent"),
                        )?;
                    }
                    None => {}
                }
            }
            "content_block_stop" => {
                self.block = None;
            }
            "message_delta" => {
                if let Some(delta) = event.delta.as_ref()
                    && let Some(stop_reason) = delta.stop_reason.as_deref()
                {
                    self.finish_reason = Some(map_finish_reason(stop_reason, &mut self.report));
                }
                if let Some(usage) = event.usage.as_ref() {
                    merge_usage(&mut self.usage, usage);
                }
            }
            "message_stop" => {
                out.extend(self.finalize()?);
            }
            "error" => {
                let (message, code, extra) = event
                    .error
                    .map(|error| (error.message, Some(error.type_), error.extra))
                    .unwrap_or_else(|| ("Anthropic stream error".to_string(), None, Map::new()));
                out.push(OutFrame::Sse {
                    event: None,
                    id: None,
                    data: serde_json::to_vec(&ChatError {
                        error: ChatErrorBody {
                            message,
                            type_: Some("server_error".to_string()),
                            param: None,
                            code,
                            extra,
                        },
                    })?,
                });
                self.done = true;
            }
            "ping" => {
                if !self.ping_warned {
                    self.ping_warned = true;
                    self.report
                        .degraded(Side::Source, "/ping", "Chat stream has no ping event");
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
                    let event: AnthropicError = serde_json::from_value(value)?;
                    let out = vec![OutFrame::Sse {
                        event: None,
                        id: None,
                        data: serde_json::to_vec(&ChatError {
                            error: ChatErrorBody {
                                message: event.error.message,
                                type_: Some("server_error".to_string()),
                                param: None,
                                code: Some(event.error.type_),
                                extra: event.error.extra,
                            },
                        })?,
                    }];
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

fn usage_to_chat(usage: &AnthropicUsage) -> ChatUsage {
    let cache_read = usage.cache_read_input_tokens.unwrap_or(0);
    let cache_creation = usage.cache_creation_input_tokens.unwrap_or(0);
    let prompt = usage.input_tokens.unwrap_or(0) + cache_read + cache_creation;
    let completion = usage.output_tokens.unwrap_or(0);
    ChatUsage {
        prompt_tokens: Some(prompt),
        completion_tokens: Some(completion),
        total_tokens: Some(prompt + completion),
        prompt_tokens_details: if cache_read > 0 {
            Some(ChatTokenDetails {
                cached_tokens: Some(cache_read),
                audio_tokens: None,
                cache_creation_tokens: None,
                cache_write_tokens: None,
                reasoning_tokens: None,
                accepted_prediction_tokens: None,
                rejected_prediction_tokens: None,
                extra: Map::new(),
            })
        } else {
            None
        },
        completion_tokens_details: None,
        extra: Map::new(),
    }
}

fn merge_usage(current: &mut Option<ChatUsage>, update: &AnthropicUsage) {
    let mut next = usage_to_chat(update);
    if let Some(existing) = current.as_ref() {
        if next.prompt_tokens.unwrap_or(0) == 0 {
            next.prompt_tokens = existing.prompt_tokens;
        }
        if next.prompt_tokens_details.is_none() {
            next.prompt_tokens_details = existing.prompt_tokens_details.clone();
        }
    }
    *current = Some(next);
}

fn with_usage_totals(mut usage: ChatUsage) -> ChatUsage {
    if usage.total_tokens.unwrap_or(0) == 0 {
        usage.total_tokens =
            Some(usage.prompt_tokens.unwrap_or(0) + usage.completion_tokens.unwrap_or(0));
    }
    usage
}

fn map_finish_reason(stop_reason: &str, report: &mut Report) -> String {
    match stop_reason {
        "end_turn" | "stop_sequence" | "pause_turn" => "stop".to_string(),
        "max_tokens" => "length".to_string(),
        "tool_use" => "tool_calls".to_string(),
        "refusal" => "content_filter".to_string(),
        other => {
            report.degraded(
                Side::Target,
                "/choices/0/finish_reason",
                format!("unknown Anthropic stop_reason {other}; using stop"),
            );
            "stop".to_string()
        }
    }
}

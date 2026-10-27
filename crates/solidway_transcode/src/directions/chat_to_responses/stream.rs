//! Responses SSE -> Chat SSE。

use std::collections::{HashMap, HashSet};

use serde_json::Map;

use crate::common::frame::{InFrame, OutFrame};
use crate::registry::{Plan, StreamBridge};
use crate::report::{Error, Report, Side, WarningCode};
use crate::wire::openai_chat::{
    ChatChunk, ChatChunkChoice, ChatDelta, ChatError, ChatErrorBody, ChatFunctionCall,
    ChatTokenDetails, ChatToolCall, ChatUsage,
};
use crate::wire::openai_responses::{ResponsesError, ResponsesEvent, ResponsesUsage};

pub(super) struct Bridge {
    plan: Plan,
    report: Report,
    id: String,
    model: String,
    created: i64,
    started: bool,
    done: bool,
    finish_reason: Option<String>,
    usage: Option<ChatUsage>,
    has_tool_calls: bool,
    service_tier_warned: bool,
    unknown_item_warned: bool,
    custom_inputs: HashMap<u32, String>,
    final_tools: HashSet<u32>,
    tool_indices: HashMap<u32, u32>,
    next_tool_index: u32,
    text_delivered: bool,
}

impl Bridge {
    pub(super) fn new(plan: &Plan) -> Self {
        Self {
            plan: *plan,
            report: Report::default(),
            id: "chatcmpl-transcoded".to_string(),
            model: String::new(),
            created: 0,
            started: false,
            done: false,
            finish_reason: None,
            usage: None,
            has_tool_calls: false,
            service_tier_warned: false,
            unknown_item_warned: false,
            custom_inputs: HashMap::new(),
            final_tools: HashSet::new(),
            tool_indices: HashMap::new(),
            next_tool_index: 0,
            text_delivered: false,
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
            created: self.created,
            model: self.model.clone(),
            choices: vec![ChatChunkChoice {
                index: 0,
                delta,
                finish_reason,
                logprobs: None,
                extra: Map::new(),
            }],
            usage: if with_usage { self.usage.clone() } else { None },
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
        let finish_reason = self.finish_reason.take().unwrap_or_else(|| {
            if self.has_tool_calls {
                "tool_calls".to_string()
            } else {
                "stop".to_string()
            }
        });
        out.push(self.chunk(ChatDelta::default(), Some(finish_reason), true)?);
        out.push(OutFrame::Done);
        Ok(out)
    }

    fn chat_tool_index(&mut self, output_index: u32) -> u32 {
        *self.tool_indices.entry(output_index).or_insert_with(|| {
            let index = self.next_tool_index;
            self.next_tool_index += 1;
            index
        })
    }

    fn apply_response_usage(&mut self, usage: Option<&ResponsesUsage>) {
        if let Some(usage) = usage {
            self.usage = Some(responses_usage_to_chat(usage));
        }
    }

    fn handle_event(&mut self, event: ResponsesEvent) -> Result<Vec<OutFrame>, Error> {
        let mut out = Vec::new();
        match event.type_.as_str() {
            "response.created" | "response.in_progress" => {
                if let Some(response) = event.response.as_ref() {
                    self.id = response.id.clone();
                    self.model = response.model.clone();
                    self.created = response.created_at;
                    self.apply_response_usage(response.usage.as_ref());
                    if response.service_tier.is_some() && !self.service_tier_warned {
                        self.service_tier_warned = true;
                        self.report.degraded(
                            Side::Source,
                            "/response/service_tier",
                            "Chat has no service_tier field in streamed chunks",
                        );
                    }
                }
                self.ensure_started(&mut out)?;
            }
            "response.output_item.added" => {
                self.ensure_started(&mut out)?;
                if let Some(item) = event.item.as_ref() {
                    if !matches!(
                        item.type_.as_str(),
                        "message"
                            | "reasoning"
                            | "function_call"
                            | "custom_tool_call"
                            | "tool_search_call"
                            | "x_search_call"
                            | "web_search_call"
                    ) && !self.unknown_item_warned
                    {
                        self.unknown_item_warned = true;
                        self.report.degraded(
                            Side::Source,
                            "/item/type",
                            format!(
                                "Responses item {} has no Chat stream equivalent",
                                item.type_
                            ),
                        );
                    }
                    let tool_name = match item.type_.as_str() {
                        "function_call" => None,
                        "custom_tool_call" => item.name.clone(),
                        "tool_search_call" => Some("tool_search".to_string()),
                        "x_search_call" => Some("x_search".to_string()),
                        _ => None,
                    };
                    if item.type_ == "web_search_call" {
                        // Server-side search is consumed silently, like sub2api.
                    } else if item.type_ == "function_call" || tool_name.is_some() {
                        self.has_tool_calls = true;
                        let chat_index = self.chat_tool_index(event.output_index.unwrap_or(0));
                        out.push(self.chunk(
                            ChatDelta {
                                tool_calls: vec![ChatToolCall {
                                    index: Some(chat_index),
                                    id: item.call_id.clone(),
                                    type_: Some("function".to_string()),
                                    function: ChatFunctionCall {
                                        name: tool_name.or_else(|| item.name.clone()),
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
                }
            }
            "response.output_text.delta" => {
                self.ensure_started(&mut out)?;
                if let Some(delta) = event.delta.as_deref() {
                    if !delta.is_empty() {
                        self.text_delivered = true;
                    }
                    out.push(self.chunk(
                        ChatDelta {
                            content: Some(delta.to_string()),
                            ..Default::default()
                        },
                        None,
                        false,
                    )?);
                }
            }
            "response.reasoning_summary_text.delta" => {
                self.ensure_started(&mut out)?;
                if let Some(delta) = event.delta.as_deref() {
                    out.push(self.chunk(
                        ChatDelta {
                            reasoning_content: Some(delta.to_string()),
                            ..Default::default()
                        },
                        None,
                        false,
                    )?);
                }
            }
            "response.custom_tool_call_input.delta" => {
                self.ensure_started(&mut out)?;
                self.has_tool_calls = true;
                if let Some(index) = event.output_index
                    && let Some(delta) = event.input.as_deref().or(event.delta.as_deref())
                {
                    self.custom_inputs.entry(index).or_default().push_str(delta);
                }
            }
            "response.function_call_arguments.delta" => {
                self.ensure_started(&mut out)?;
                self.has_tool_calls = true;
                if let Some(delta) = event.delta.as_deref() {
                    let chat_index = self.chat_tool_index(event.output_index.unwrap_or(0));
                    out.push(self.chunk(
                        ChatDelta {
                            tool_calls: vec![ChatToolCall {
                                index: Some(chat_index),
                                id: event.call_id.clone(),
                                type_: None,
                                function: ChatFunctionCall {
                                    name: None,
                                    arguments: delta.to_string(),
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
            "response.output_text.done" => {
                self.ensure_started(&mut out)?;
                if !self.text_delivered
                    && let Some(text) = event.text.as_deref().filter(|text| !text.is_empty())
                {
                    self.text_delivered = true;
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
            "response.completed" | "response.incomplete" => {
                self.ensure_started(&mut out)?;
                self.recover_terminal_text(event.response.as_ref(), &mut out)?;
                if let Some(response) = event.response.as_ref() {
                    self.apply_response_usage(response.usage.as_ref());
                    if response.service_tier.is_some() && !self.service_tier_warned {
                        self.service_tier_warned = true;
                        self.report.degraded(
                            Side::Source,
                            "/response/service_tier",
                            "Chat has no service_tier field in streamed chunks",
                        );
                    }
                    self.finish_reason = Some(map_finish_reason(
                        &response.status,
                        response
                            .incomplete_details
                            .as_ref()
                            .map(|details| details.reason.as_str()),
                        self.has_tool_calls,
                        &mut self.report,
                    ));
                } else {
                    self.apply_response_usage(event.usage.as_ref());
                }
                out.extend(self.finalize()?);
            }
            "response.failed" => {
                let error = event
                    .response
                    .as_ref()
                    .and_then(|response| response.error.as_ref())
                    .or(event.error.as_ref());
                out.push(self.error_frame(error)?);
                self.done = true;
            }
            "error" => {
                out.push(self.error_frame(event.error.as_ref())?);
                self.done = true;
            }
            "response.function_call_arguments.done" => {
                if let Some(index) = event.output_index
                    && !self.final_tools.contains(&index)
                    && let Some(arguments) = event.arguments.as_ref()
                {
                    let arguments = match arguments {
                        serde_json::Value::String(text) => text.clone(),
                        other => serde_json::to_string(other).unwrap_or_default(),
                    };
                    self.final_tools.insert(index);
                    self.has_tool_calls = true;
                    let chat_index = self.chat_tool_index(index);
                    out.push(self.chunk(
                        ChatDelta {
                            tool_calls: vec![ChatToolCall {
                                index: Some(chat_index),
                                id: event.call_id.clone(),
                                type_: Some("function".to_string()),
                                function: ChatFunctionCall {
                                    name: event.name.clone(),
                                    arguments,
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
            "response.output_item.done" => {
                if let Some(item) = event.item.as_ref()
                    && let Some(index) = event.output_index
                    && !self.final_tools.contains(&index)
                    && item.type_ != "web_search_call"
                {
                    let arguments = match item.type_.as_str() {
                        "custom_tool_call" => {
                            let input = item.input.clone().unwrap_or_else(|| {
                                serde_json::Value::String(
                                    self.custom_inputs.get(&index).cloned().unwrap_or_default(),
                                )
                            });
                            Some(
                                serde_json::to_string(&serde_json::json!({"input": input}))
                                    .unwrap_or_else(|_| "{}".to_string()),
                            )
                        }
                        "tool_search_call" => Some(match item.arguments.as_ref() {
                            Some(serde_json::Value::String(text)) => text.clone(),
                            Some(value) => {
                                serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
                            }
                            None => "{}".to_string(),
                        }),
                        "x_search_call" => Some(
                            serde_json::to_string(
                                &item.action.clone().unwrap_or_else(|| serde_json::json!({})),
                            )
                            .unwrap_or_else(|_| "{}".to_string()),
                        ),
                        _ => None,
                    };
                    if let Some(arguments) = arguments {
                        self.final_tools.insert(index);
                        self.has_tool_calls = true;
                        let chat_index = self.chat_tool_index(index);
                        out.push(self.chunk(
                            ChatDelta {
                                tool_calls: vec![ChatToolCall {
                                    index: Some(chat_index),
                                    id: item.call_id.clone(),
                                    type_: Some("function".to_string()),
                                    function: ChatFunctionCall {
                                        name: match item.type_.as_str() {
                                            "custom_tool_call" => item.name.clone(),
                                            "tool_search_call" => Some("tool_search".to_string()),
                                            "x_search_call" => Some("x_search".to_string()),
                                            _ => None,
                                        },
                                        arguments,
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
            }
            "response.content_part.added"
            | "response.content_part.done"
            | "response.reasoning_summary_text.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done" => {}
            other => {
                self.report.unsupported(
                    self.plan.policy,
                    Side::Target,
                    "/type",
                    format!("unknown Responses stream event {other}"),
                )?;
            }
        }
        Ok(out)
    }

    fn recover_terminal_text(
        &mut self,
        response: Option<&crate::wire::openai_responses::ResponsesResponse>,
        out: &mut Vec<OutFrame>,
    ) -> Result<(), Error> {
        if self.text_delivered {
            return Ok(());
        }
        let Some(response) = response else {
            return Ok(());
        };
        let mut text = String::new();
        for item in &response.output {
            if item.type_ != "message" {
                continue;
            }
            for part in &item.content {
                if part.type_ == "output_text"
                    && let Some(part_text) = part.text.as_deref()
                {
                    text.push_str(part_text);
                }
            }
        }
        if text.is_empty() {
            return Ok(());
        }
        self.text_delivered = true;
        out.push(self.chunk(
            ChatDelta {
                content: Some(text),
                ..Default::default()
            },
            None,
            false,
        )?);
        Ok(())
    }

    fn error_frame(&self, error: Option<&ResponsesError>) -> Result<OutFrame, Error> {
        let (message, code, param, extra) = error
            .map(|error| {
                (
                    error.message.clone(),
                    error.code.clone(),
                    error.param.clone(),
                    error.extra.clone(),
                )
            })
            .unwrap_or_else(|| ("Responses stream error".to_string(), None, None, Map::new()));
        Ok(OutFrame::Sse {
            event: None,
            id: None,
            data: serde_json::to_vec(&ChatError {
                error: ChatErrorBody {
                    message,
                    type_: Some("server_error".to_string()),
                    param,
                    code,
                    extra,
                },
            })?,
        })
    }
}

impl StreamBridge for Bridge {
    fn push(&mut self, frame: InFrame) -> Result<Vec<OutFrame>, Error> {
        match frame {
            InFrame::Sse { data, .. } => {
                if data == b"[DONE]" {
                    return self.finalize();
                }
                let event: ResponsesEvent = serde_json::from_slice(&data)?;
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
                "Responses stream ended before response.completed",
            );
        }
        self.finalize()
    }

    fn report(&self) -> &Report {
        &self.report
    }
}

fn responses_usage_to_chat(usage: &ResponsesUsage) -> ChatUsage {
    let cached = usage
        .input_tokens_details
        .as_ref()
        .and_then(|details| details.cached_tokens);
    let reasoning = usage
        .output_tokens_details
        .as_ref()
        .and_then(|details| details.reasoning_tokens);
    ChatUsage {
        prompt_tokens: usage.input_tokens,
        completion_tokens: usage.output_tokens,
        total_tokens: usage
            .total_tokens
            .or_else(|| Some(usage.input_tokens.unwrap_or(0) + usage.output_tokens.unwrap_or(0))),
        prompt_tokens_details: cached.map(|cached| ChatTokenDetails {
            cached_tokens: Some(cached),
            ..empty_details()
        }),
        completion_tokens_details: reasoning.map(|reasoning| ChatTokenDetails {
            reasoning_tokens: Some(reasoning),
            ..empty_details()
        }),
        extra: Map::new(),
    }
}

fn empty_details() -> ChatTokenDetails {
    ChatTokenDetails {
        cached_tokens: None,
        audio_tokens: None,
        cache_creation_tokens: None,
        cache_write_tokens: None,
        reasoning_tokens: None,
        accepted_prediction_tokens: None,
        rejected_prediction_tokens: None,
        extra: Map::new(),
    }
}

fn map_finish_reason(
    status: &str,
    incomplete_reason: Option<&str>,
    has_tool_calls: bool,
    report: &mut Report,
) -> String {
    match status {
        "completed" => {
            if has_tool_calls {
                "tool_calls".to_string()
            } else {
                "stop".to_string()
            }
        }
        "incomplete" => match incomplete_reason {
            Some("content_filter") => "content_filter".to_string(),
            Some("max_output_tokens") => "length".to_string(),
            other => {
                report.warn(
                    WarningCode::Degraded,
                    Side::Target,
                    "/choices/0/finish_reason",
                    format!("unknown Responses incomplete reason {other:?}; using length"),
                );
                "length".to_string()
            }
        },
        "failed" => "stop".to_string(),
        other => {
            report.warn(
                WarningCode::Degraded,
                Side::Target,
                "/choices/0/finish_reason",
                format!("unknown Responses status {other}; using stop"),
            );
            "stop".to_string()
        }
    }
}

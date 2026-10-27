//! Responses SSE -> Anthropic SSE。

use std::collections::{BTreeSet, HashMap};

use serde_json::{Map, Value, json};

use crate::common::frame::{InFrame, OutFrame};
use crate::registry::{Plan, StreamBridge};
use crate::report::{Error, Report, Side, WarningCode};
use crate::wire::anthropic::AnthropicUsage;
use crate::wire::openai_responses::{ResponsesError, ResponsesEvent, ResponsesUsage};

pub(super) struct Bridge {
    plan: Plan,
    report: Report,
    id: String,
    model: String,
    started: bool,
    done: bool,
    open: BTreeSet<u32>,
    finish_reason: Option<String>,
    usage: Option<AnthropicUsage>,
    signature_warned: bool,
    service_tier_warned: bool,
    custom_inputs: HashMap<u32, String>,
    thinking_signatures: HashMap<u32, String>,
    delivered_text: HashMap<u32, String>,
    text_delivered: bool,
}

impl Bridge {
    pub(super) fn new(plan: &Plan) -> Self {
        Self {
            plan: *plan,
            report: Report::default(),
            id: "msg_transcoded".to_string(),
            model: String::new(),
            started: false,
            done: false,
            open: BTreeSet::new(),
            finish_reason: None,
            usage: None,
            signature_warned: false,
            service_tier_warned: false,
            custom_inputs: HashMap::new(),
            thinking_signatures: HashMap::new(),
            delivered_text: HashMap::new(),
            text_delivered: false,
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
                    "usage": {"input_tokens": input_tokens, "output_tokens": 0}
                }
            }),
        )?);
        Ok(())
    }

    fn close(&mut self, index: u32, out: &mut Vec<OutFrame>) -> Result<(), Error> {
        if self.open.remove(&index) {
            out.push(self.event(
                "content_block_stop",
                json!({"type": "content_block_stop", "index": index}),
            )?);
        }
        Ok(())
    }

    fn open_block(
        &mut self,
        index: u32,
        block: Value,
        out: &mut Vec<OutFrame>,
    ) -> Result<(), Error> {
        self.open.insert(index);
        out.push(self.event(
            "content_block_start",
            json!({
                "type": "content_block_start",
                "index": index,
                "content_block": block
            }),
        )?);
        Ok(())
    }

    fn finalize(&mut self) -> Result<Vec<OutFrame>, Error> {
        if self.done {
            return Ok(Vec::new());
        }
        self.done = true;
        let mut out = Vec::new();
        self.ensure_started(&mut out)?;
        let indices: Vec<u32> = self.open.iter().copied().collect();
        for index in indices {
            self.close(index, &mut out)?;
        }
        let stop_reason = self.finish_reason.take().unwrap_or_else(|| {
            self.report.warn(
                WarningCode::Synthesized,
                Side::Target,
                "/delta/stop_reason",
                "Responses stream ended without completion; using end_turn",
            );
            "end_turn".to_string()
        });
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

    fn handle_event(&mut self, event: ResponsesEvent) -> Result<Vec<OutFrame>, Error> {
        let mut out = Vec::new();
        match event.type_.as_str() {
            "response.created" | "response.in_progress" => {
                if let Some(response) = event.response.as_ref() {
                    self.id = response.id.clone();
                    self.model = response.model.clone();
                    if let Some(usage) = response.usage.as_ref() {
                        self.usage = Some(responses_usage_to_anthropic(usage));
                    }
                    if response.service_tier.is_some() && !self.service_tier_warned {
                        self.service_tier_warned = true;
                        self.report.degraded(
                            Side::Source,
                            "/response/service_tier",
                            "Anthropic message_start has no service_tier field",
                        );
                    }
                }
                self.ensure_started(&mut out)?;
            }
            "response.output_item.added" => {
                self.ensure_started(&mut out)?;
                let index = event.output_index.unwrap_or(0);
                if let Some(item) = event.item.as_ref() {
                    match item.type_.as_str() {
                        "message" => {
                            self.open_block(index, json!({"type": "text", "text": ""}), &mut out)?
                        }
                        "reasoning" => {
                            let decoded =
                                item.encrypted_content.as_deref().and_then(decode_thinking);
                            let (thinking, signature) = match decoded {
                                Some(payload) => (
                                    payload
                                        .get("thinking")
                                        .and_then(Value::as_str)
                                        .unwrap_or("")
                                        .to_string(),
                                    payload
                                        .get("signature")
                                        .and_then(Value::as_str)
                                        .unwrap_or("")
                                        .to_string(),
                                ),
                                None => {
                                    if item.encrypted_content.is_some() && !self.signature_warned {
                                        self.signature_warned = true;
                                        self.report.degraded(
                                            Side::Source,
                                            "/item/encrypted_content",
                                            "Responses encrypted reasoning is not an Anthropic thinking envelope",
                                        );
                                    }
                                    (String::new(), String::new())
                                }
                            };
                            if !signature.is_empty() {
                                self.thinking_signatures.insert(index, signature.clone());
                            }
                            self.open_block(
                                index,
                                json!({
                                    "type": "thinking",
                                    "thinking": thinking,
                                    "signature": signature
                                }),
                                &mut out,
                            )?
                        }
                        "function_call" => self.open_block(
                            index,
                            json!({
                                "type": "tool_use",
                                "id": item.call_id,
                                "name": item.name,
                                "input": {}
                            }),
                            &mut out,
                        )?,
                        "custom_tool_call" => self.open_block(
                            index,
                            json!({
                                "type": "tool_use",
                                "id": item.call_id,
                                "name": item.name,
                                "input": {}
                            }),
                            &mut out,
                        )?,
                        "tool_search_call" => self.open_block(
                            index,
                            json!({
                                "type": "tool_use",
                                "id": item.call_id,
                                "name": "tool_search",
                                "input": {}
                            }),
                            &mut out,
                        )?,
                        "x_search_call" | "web_search_call" => self.open_block(
                            index,
                            json!({
                                "type": "server_tool_use",
                                "id": responses_server_tool_id(item, index),
                                "name": item.type_.trim_end_matches("_call"),
                                "input": {}
                            }),
                            &mut out,
                        )?,
                        other => self.report.unsupported(
                            self.plan.policy,
                            Side::Target,
                            "/item/type",
                            format!("Responses item {other} has no Anthropic block equivalent"),
                        )?,
                    }
                }
            }
            "response.output_text.delta" => {
                self.ensure_started(&mut out)?;
                if let Some(delta) = event.delta.as_deref() {
                    if !delta.is_empty() {
                        self.text_delivered = true;
                        self.delivered_text
                            .entry(event.output_index.unwrap_or(0))
                            .or_default()
                            .push_str(delta);
                    }
                    out.push(self.event(
                        "content_block_delta",
                        json!({
                            "type": "content_block_delta",
                            "index": event.output_index.unwrap_or(0),
                            "delta": {"type": "text_delta", "text": delta}
                        }),
                    )?);
                }
            }
            "response.reasoning_summary_text.delta" => {
                self.ensure_started(&mut out)?;
                if let Some(delta) = event.delta.as_deref() {
                    out.push(self.event(
                        "content_block_delta",
                        json!({
                            "type": "content_block_delta",
                            "index": event.output_index.unwrap_or(0),
                            "delta": {"type": "thinking_delta", "thinking": delta}
                        }),
                    )?);
                }
            }
            "response.custom_tool_call_input.delta" => {
                self.ensure_started(&mut out)?;
                if let Some(index) = event.output_index
                    && let Some(delta) = event.input.as_deref().or(event.delta.as_deref())
                {
                    self.custom_inputs.entry(index).or_default().push_str(delta);
                }
            }
            "response.function_call_arguments.delta" => {
                self.ensure_started(&mut out)?;
                if let Some(delta) = event.delta.as_deref() {
                    out.push(self.event(
                        "content_block_delta",
                        json!({
                            "type": "content_block_delta",
                            "index": event.output_index.unwrap_or(0),
                            "delta": {"type": "input_json_delta", "partial_json": delta}
                        }),
                    )?);
                }
            }
            "response.output_text.done" => {
                self.ensure_started(&mut out)?;
                let index = event.output_index.unwrap_or(0);
                if let Some(text) = event.text.as_deref().filter(|text| !text.is_empty()) {
                    let delivered = self
                        .delivered_text
                        .get(&index)
                        .map(String::as_str)
                        .unwrap_or("");
                    let suffix = if delivered.is_empty() {
                        text
                    } else if text.starts_with(delivered) && text.len() > delivered.len() {
                        &text[delivered.len()..]
                    } else {
                        ""
                    };
                    if !suffix.is_empty() {
                        if !self.open.contains(&index) {
                            self.open_block(index, json!({"type": "text", "text": ""}), &mut out)?;
                        }
                        self.text_delivered = true;
                        self.delivered_text
                            .entry(index)
                            .or_default()
                            .push_str(suffix);
                        out.push(self.event(
                            "content_block_delta",
                            json!({
                                "type": "content_block_delta",
                                "index": index,
                                "delta": {"type": "text_delta", "text": suffix}
                            }),
                        )?);
                    }
                }
            }
            "response.output_item.done" => {
                if let (Some(index), Some(item)) = (event.output_index, event.item.as_ref()) {
                    if item.type_ == "reasoning"
                        && let Some(signature) = self.thinking_signatures.remove(&index)
                    {
                        out.push(self.event(
                            "content_block_delta",
                            json!({
                                "type": "content_block_delta",
                                "index": index,
                                "delta": {"type": "signature_delta", "signature": signature}
                            }),
                        )?);
                    }
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
                        "x_search_call" | "web_search_call" => Some(
                            serde_json::to_string(
                                &item.action.clone().unwrap_or_else(|| serde_json::json!({})),
                            )
                            .unwrap_or_else(|_| "{}".to_string()),
                        ),
                        _ => None,
                    };
                    if let Some(arguments) = arguments {
                        out.push(self.event(
                            "content_block_delta",
                            json!({
                                "type": "content_block_delta",
                                "index": index,
                                "delta": {"type": "input_json_delta", "partial_json": arguments}
                            }),
                        )?);
                    }
                    self.close(index, &mut out)?;
                    if matches!(item.type_.as_str(), "x_search_call" | "web_search_call") {
                        let result_index = index.saturating_add(10_000);
                        let name = item.type_.trim_end_matches("_call");
                        let content = responses_server_tool_results(item);
                        self.open_block(
                            result_index,
                            json!({
                                "type": format!("{name}_tool_result"),
                                "tool_use_id": responses_server_tool_id(item, index),
                                "content": content
                            }),
                            &mut out,
                        )?;
                        self.close(result_index, &mut out)?;
                    }
                }
            }
            "response.completed" | "response.incomplete" => {
                self.ensure_started(&mut out)?;
                self.recover_terminal_text(event.response.as_ref(), &mut out)?;
                if let Some(response) = event.response.as_ref() {
                    if let Some(usage) = response.usage.as_ref() {
                        self.usage = Some(responses_usage_to_anthropic(usage));
                    }
                    if response.service_tier.is_some() && !self.service_tier_warned {
                        self.service_tier_warned = true;
                        self.report.degraded(
                            Side::Source,
                            "/response/service_tier",
                            "Anthropic message_delta has no service_tier field",
                        );
                    }
                    self.finish_reason = Some(map_stop_reason(
                        &response.status,
                        response
                            .incomplete_details
                            .as_ref()
                            .map(|details| details.reason.as_str()),
                    ));
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
            "response.content_part.added"
            | "response.content_part.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done"
            | "response.reasoning_summary_text.done"
            | "response.function_call_arguments.done" => {}
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
        let index = self.open.iter().next().copied().unwrap_or(0);
        if self.open.is_empty() {
            self.open_block(index, json!({"type": "text", "text": ""}), out)?;
        }
        out.push(self.event(
            "content_block_delta",
            json!({
                "type": "content_block_delta",
                "index": index,
                "delta": {"type": "text_delta", "text": text}
            }),
        )?);
        Ok(())
    }

    fn error_frame(&self, error: Option<&ResponsesError>) -> Result<OutFrame, Error> {
        let message = error
            .map(|error| error.message.clone())
            .unwrap_or_else(|| "Responses stream error".to_string());
        let mut body = json!({"type": "api_error", "message": message});
        if let Some(error) = error
            && let Some(object) = body.as_object_mut()
        {
            if let Some(code) = error.code.clone() {
                object.insert("code".to_string(), Value::String(code));
            }
            if let Some(param) = error.param.clone() {
                object.insert("param".to_string(), Value::String(param));
            }
            for (key, value) in &error.extra {
                object.insert(key.clone(), value.clone());
            }
        }
        self.event("error", json!({"type": "error", "error": body}))
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

fn decode_thinking(encrypted: &str) -> Option<Value> {
    if let Some(json) = encrypted.strip_prefix("anthropic-thinking-v1:") {
        return serde_json::from_str(json).ok();
    }
    if encrypted.is_empty() {
        None
    } else {
        Some(serde_json::json!({"signature": encrypted}))
    }
}

fn responses_usage_to_anthropic(usage: &ResponsesUsage) -> AnthropicUsage {
    let cached = usage
        .input_tokens_details
        .as_ref()
        .and_then(|details| details.cached_tokens)
        .unwrap_or(0);
    let cache_creation = usage
        .cache_creation_input_tokens
        .or_else(|| {
            usage
                .input_tokens_details
                .as_ref()
                .and_then(|details| details.cache_creation_tokens.or(details.cache_write_tokens))
        })
        .unwrap_or(0);
    let input = usage
        .input_tokens
        .unwrap_or(0)
        .saturating_sub(cached)
        .saturating_sub(cache_creation);
    AnthropicUsage {
        input_tokens: Some(input),
        output_tokens: usage.output_tokens,
        cache_creation_input_tokens: (cache_creation > 0).then_some(cache_creation),
        cache_read_input_tokens: (cached > 0).then_some(cached),
        service_tier: None,
        cache_creation: None,
        server_tool_use: None,
        extra: Map::new(),
    }
}

fn responses_server_tool_id(
    item: &crate::wire::openai_responses::ResponsesOutput,
    index: u32,
) -> String {
    let raw = item
        .id
        .as_deref()
        .or(item.call_id.as_deref())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("web_search_{index}"));
    if raw.starts_with("srvtoolu_") {
        raw
    } else {
        format!("srvtoolu_{raw}")
    }
}

fn responses_server_tool_results(item: &crate::wire::openai_responses::ResponsesOutput) -> Value {
    let value = item
        .extra
        .get("results")
        .or_else(|| item.extra.get("sources"))
        .or_else(|| item.extra.get("content"))
        .or_else(|| {
            item.action
                .as_ref()
                .and_then(|action| action.get("results"))
        })
        .cloned()
        .unwrap_or_else(|| json!([]));
    normalize_results(value)
}

fn normalize_results(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items),
        Value::Null => json!([]),
        Value::Object(mut object) => {
            if let Some(Value::Array(items)) = object
                .remove("results")
                .or_else(|| object.remove("sources"))
            {
                Value::Array(items)
            } else if object
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|type_| type_.ends_with("_error"))
            {
                Value::Object(object)
            } else {
                json!([Value::Object(object)])
            }
        }
        other => json!([other]),
    }
}

fn map_stop_reason(status: &str, incomplete_reason: Option<&str>) -> String {
    match status {
        "incomplete" => match incomplete_reason {
            Some("content_filter") => "refusal",
            _ => "max_tokens",
        },
        _ => "end_turn",
    }
    .to_string()
}

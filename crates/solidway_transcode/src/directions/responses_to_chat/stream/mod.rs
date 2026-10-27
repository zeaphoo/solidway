//! Chat SSE -> Responses SSE。

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{Value, json};

use crate::common::frame::{InFrame, OutFrame};
use crate::registry::{Plan, ResponseContext, StreamBridge};
use crate::report::{Error, Report, Side, WarningCode};
use crate::wire::openai_chat::{ChatChunk, ChatError, ChatUsage};

mod emit;

#[derive(Debug, Default, Clone)]
struct ToolState {
    output_index: u32,
    item_id: String,
    call_id: Option<String>,
    name: Option<String>,
    arguments: String,
}

pub(super) struct Bridge {
    report: Report,
    id: String,
    model: String,
    created: i64,
    started: bool,
    done: bool,
    seq: u64,
    next_output_index: u32,
    message_index: Option<u32>,
    reasoning_index: Option<u32>,
    text: String,
    reasoning: String,
    tools: BTreeMap<u32, ToolState>,
    pending_tool_ids: HashMap<u32, String>,
    pending_tool_names: HashMap<u32, String>,
    pending_tool_args: HashMap<u32, String>,
    tool_args_seen: HashSet<u32>,
    finish_reason: Option<String>,
    usage: Option<ChatUsage>,
    choice_warned: bool,
    service_tier_warned: bool,
    system_fingerprint_warned: bool,
    refusal_warned: bool,
    tool_type_warned: bool,
    extras_warned: bool,
    context: ResponseContext,
}

impl Bridge {
    pub(super) fn new(_plan: &Plan, context: &ResponseContext) -> Self {
        Self {
            report: Report::default(),
            id: "resp_transcoded".to_string(),
            model: String::new(),
            created: 0,
            started: false,
            done: false,
            seq: 0,
            next_output_index: 0,
            message_index: None,
            reasoning_index: None,
            text: String::new(),
            reasoning: String::new(),
            tools: BTreeMap::new(),
            pending_tool_ids: HashMap::new(),
            pending_tool_names: HashMap::new(),
            pending_tool_args: HashMap::new(),
            tool_args_seen: HashSet::new(),
            finish_reason: None,
            usage: None,
            choice_warned: false,
            service_tier_warned: false,
            system_fingerprint_warned: false,
            refusal_warned: false,
            tool_type_warned: false,
            extras_warned: false,
            context: context.clone(),
        }
    }

    fn handle_chunk(&mut self, chunk: ChatChunk) -> Result<Vec<OutFrame>, Error> {
        let mut out = Vec::new();
        self.id = chunk.id;
        self.model = chunk.model;
        if self.created == 0 {
            self.created = chunk.created;
        }
        if let Some(usage) = chunk.usage.as_ref() {
            self.usage = Some(usage.clone());
        }
        if chunk.service_tier.is_some() && !self.service_tier_warned {
            self.service_tier_warned = true;
            self.report.degraded(
                Side::Source,
                "/service_tier",
                "Responses has no service_tier field in streamed events",
            );
        }
        if chunk.system_fingerprint.is_some() && !self.system_fingerprint_warned {
            self.system_fingerprint_warned = true;
            self.report.degraded(
                Side::Source,
                "/system_fingerprint",
                "Responses has no system_fingerprint field",
            );
        }
        if chunk.choices.len() > 1 && !self.choice_warned {
            self.choice_warned = true;
            self.report.degraded(
                Side::Source,
                "/choices",
                "Responses streams one output; extra Chat choices are dropped",
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
                    "unknown Chat stream fields are not mapped to Responses",
                );
            }
            if let Some(reasoning) = choice
                .delta
                .reasoning_content
                .as_deref()
                .or(choice.delta.reasoning.as_deref())
            {
                let index = self.ensure_reasoning_item(&mut out)?;
                self.reasoning.push_str(reasoning);
                out.push(self.event(
                    "response.reasoning_summary_text.delta",
                    json!({
                        "type": "response.reasoning_summary_text.delta",
                        "item_id": format!("{}-reasoning", self.id),
                        "output_index": index,
                        "summary_index": 0,
                        "delta": reasoning
                    }),
                )?);
            }
            if let Some(content) = choice
                .delta
                .content
                .as_deref()
                .filter(|content| !content.is_empty())
            {
                let index = self.ensure_message_item(&mut out)?;
                self.text.push_str(content);
                out.push(self.event(
                    "response.output_text.delta",
                    json!({
                        "type": "response.output_text.delta",
                        "item_id": format!("{}-message", self.id),
                        "output_index": index,
                        "content_index": 0,
                        "delta": content
                    }),
                )?);
            }
            if let Some(refusal) = choice.delta.refusal.as_deref() {
                if !self.refusal_warned {
                    self.refusal_warned = true;
                    self.report.degraded(
                        Side::Target,
                        "/delta/refusal",
                        "Chat refusal is emitted as Responses output_text",
                    );
                }
                let index = self.ensure_message_item(&mut out)?;
                self.text.push_str(refusal);
                out.push(self.event(
                    "response.output_text.delta",
                    json!({
                        "type": "response.output_text.delta",
                        "item_id": format!("{}-message", self.id),
                        "output_index": index,
                        "content_index": 0,
                        "delta": refusal
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
                        "Chat tool call type other than function is emitted as Responses function_call",
                    );
                }
                let chat_index = call.index.unwrap_or(0);
                if let Some(id) = call.id.as_deref().filter(|id| !id.is_empty()) {
                    self.pending_tool_ids.insert(chat_index, id.to_string());
                }
                if let Some(name) = call
                    .function
                    .name
                    .as_deref()
                    .filter(|name| !name.is_empty())
                {
                    self.pending_tool_names.insert(chat_index, name.to_string());
                }
                let name = self
                    .pending_tool_names
                    .get(&chat_index)
                    .cloned()
                    .unwrap_or_default();
                if !self.tools.contains_key(&chat_index) && !name.is_empty() {
                    let id = self
                        .pending_tool_ids
                        .remove(&chat_index)
                        .unwrap_or_default();
                    let pending = self
                        .pending_tool_args
                        .remove(&chat_index)
                        .unwrap_or_default();
                    self.announce_tool_item(chat_index, Some(&id), Some(&name), &mut out)?;
                    self.emit_tool_arguments(chat_index, &pending, &mut out)?;
                }
                if self.tools.contains_key(&chat_index) {
                    self.emit_tool_arguments(chat_index, &call.function.arguments, &mut out)?;
                } else {
                    self.pending_tool_args
                        .entry(chat_index)
                        .or_default()
                        .push_str(&call.function.arguments);
                }
            }
            if let Some(reason) = choice.finish_reason.as_deref() {
                self.finish_reason = Some(reason.to_string());
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
                        "type": "server_error",
                        "message": err.error.message,
                        "code": err.error.code
                    });
                    if let Some(object) = error.as_object_mut() {
                        if let Some(param) = err.error.param {
                            object.insert("param".to_string(), Value::String(param));
                        }
                        for (key, value) in err.error.extra {
                            object.insert(key, value);
                        }
                    }
                    let out = vec![self.event("error", json!({"type": "error", "error": error}))?];
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
                "Chat stream ended before [DONE]",
            );
        }
        self.finalize()
    }

    fn report(&self) -> &Report {
        &self.report
    }
}

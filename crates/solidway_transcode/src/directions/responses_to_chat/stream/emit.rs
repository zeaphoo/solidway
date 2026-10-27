//! Chat -> Responses 的 emit 辅助方法。

use serde_json::{Value, json};

use crate::common::frame::OutFrame;
use crate::report::{Error, Side, WarningCode};
use crate::wire::openai_chat::ChatUsage;

use super::{Bridge, ToolState};

impl Bridge {
    fn tool_shape(&self, name: Option<&str>) -> (&'static str, Option<String>, Option<String>) {
        let name = name.unwrap_or("");
        if name == "tool_search" {
            ("tool_search_call", None, None)
        } else if self.context.custom_tools.contains(name) {
            ("custom_tool_call", Some(name.to_string()), None)
        } else if let Some(namespace) = self.context.namespaces.get(name) {
            (
                "function_call",
                Some(namespace.name.clone()),
                Some(namespace.namespace.clone()),
            )
        } else if let Some((namespace, child)) = name.split_once("__") {
            (
                "function_call",
                Some(child.to_string()),
                Some(namespace.to_string()),
            )
        } else {
            ("function_call", Some(name.to_string()), None)
        }
    }

    pub(super) fn next_seq(&mut self) -> u64 {
        let value = self.seq;
        self.seq += 1;
        value
    }

    pub(super) fn event(&mut self, event_type: &str, mut value: Value) -> Result<OutFrame, Error> {
        if let Some(object) = value.as_object_mut() {
            object.insert("sequence_number".to_string(), json!(self.next_seq()));
        }
        Ok(OutFrame::Sse {
            event: Some(event_type.to_string()),
            id: None,
            data: serde_json::to_vec(&value)?,
        })
    }

    pub(super) fn ensure_started(&mut self, out: &mut Vec<OutFrame>) -> Result<(), Error> {
        if self.started {
            return Ok(());
        }
        self.started = true;
        out.push(self.event(
            "response.created",
            json!({
                "type": "response.created",
                "response": self.response_json("in_progress", Vec::new())
            }),
        )?);
        Ok(())
    }

    pub(super) fn response_json(&self, status: &str, output: Vec<Value>) -> Value {
        let mut value = json!({
            "id": self.id,
            "object": "response",
            "created_at": self.created,
            "model": self.model,
            "status": status,
            "output": output,
            "usage": self.usage.as_ref().map(chat_usage_to_responses)
        });
        if status == "incomplete" {
            let reason = match self.finish_reason.as_deref() {
                Some("content_filter") => "content_filter",
                _ => "max_output_tokens",
            };
            value["incomplete_details"] = json!({"reason": reason});
        }
        value
    }

    pub(super) fn ensure_message_item(&mut self, out: &mut Vec<OutFrame>) -> Result<u32, Error> {
        if let Some(index) = self.message_index {
            return Ok(index);
        }
        let index = self.next_output_index;
        self.next_output_index += 1;
        self.message_index = Some(index);
        let item_id = format!("{}-message", self.id);
        out.push(self.event(
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": index,
                "item": {
                    "type": "message",
                    "id": item_id,
                    "role": "assistant",
                    "status": "in_progress",
                    "content": []
                }
            }),
        )?);
        out.push(self.event(
            "response.content_part.added",
            json!({
                "type": "response.content_part.added",
                "item_id": item_id,
                "output_index": index,
                "content_index": 0,
                "part": {"type": "output_text", "text": ""}
            }),
        )?);
        Ok(index)
    }

    pub(super) fn ensure_reasoning_item(&mut self, out: &mut Vec<OutFrame>) -> Result<u32, Error> {
        if let Some(index) = self.reasoning_index {
            return Ok(index);
        }
        let index = self.next_output_index;
        self.next_output_index += 1;
        self.reasoning_index = Some(index);
        let item_id = format!("{}-reasoning", self.id);
        out.push(self.event(
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": index,
                "item": {
                    "type": "reasoning",
                    "id": item_id,
                    "summary": []
                }
            }),
        )?);
        out.push(self.event(
            "response.reasoning_summary_part.added",
            json!({
                "type": "response.reasoning_summary_part.added",
                "item_id": item_id,
                "output_index": index,
                "summary_index": 0,
                "part": {"type": "summary_text", "text": ""}
            }),
        )?);
        Ok(index)
    }

    pub(super) fn announce_tool_item(
        &mut self,
        chat_index: u32,
        id: Option<&str>,
        name: Option<&str>,
        out: &mut Vec<OutFrame>,
    ) -> Result<u32, Error> {
        let index = self.next_output_index;
        self.next_output_index += 1;
        let item_id = format!("{}-call-{}", self.id, chat_index);
        self.tools.insert(
            chat_index,
            ToolState {
                output_index: index,
                item_id: item_id.clone(),
                call_id: id.map(ToString::to_string),
                name: name.map(ToString::to_string),
                arguments: String::new(),
            },
        );
        let (item_type, item_name, namespace) = self.tool_shape(name);
        out.push(self.event(
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": index,
                "item": {
                    "type": item_type,
                    "id": item_id,
                    "call_id": id.unwrap_or(""),
                    "name": item_name,
                    "namespace": namespace,
                    "arguments": "",
                    "input": if item_type == "custom_tool_call" { Some("") } else { None }
                }
            }),
        )?);
        Ok(index)
    }

    pub(super) fn emit_tool_arguments(
        &mut self,
        chat_index: u32,
        arguments: &str,
        out: &mut Vec<OutFrame>,
    ) -> Result<(), Error> {
        if arguments.is_empty() {
            return Ok(());
        }
        let item_type = self
            .tools
            .get(&chat_index)
            .and_then(|state| state.name.as_deref())
            .map(|name| self.tool_shape(Some(name)).0)
            .unwrap_or("function_call");
        let Some(state) = self.tools.get_mut(&chat_index) else {
            return Ok(());
        };
        state.arguments.push_str(arguments);
        let index = state.output_index;
        let item_id = state.item_id.clone();
        self.tool_args_seen.insert(chat_index);
        if item_type != "custom_tool_call" {
            out.push(self.event(
                "response.function_call_arguments.delta",
                json!({
                    "type": "response.function_call_arguments.delta",
                    "item_id": item_id,
                    "output_index": index,
                    "delta": arguments
                }),
            )?);
        }
        Ok(())
    }

    pub(super) fn finalize(&mut self) -> Result<Vec<OutFrame>, Error> {
        if self.done {
            return Ok(Vec::new());
        }
        self.done = true;
        let mut out = Vec::new();
        self.ensure_started(&mut out)?;

        if self.message_index.is_none()
            && self.text.is_empty()
            && self.tools.is_empty()
            && !self.reasoning.trim().is_empty()
        {
            let index = self.ensure_message_item(&mut out)?;
            self.text = self.reasoning.clone();
            out.push(self.event(
                "response.output_text.delta",
                json!({
                    "type": "response.output_text.delta",
                    "item_id": format!("{}-message", self.id),
                    "output_index": index,
                    "content_index": 0,
                    "delta": self.reasoning
                }),
            )?);
        }

        let mut pending: Vec<u32> = self
            .pending_tool_ids
            .keys()
            .chain(self.pending_tool_names.keys())
            .chain(self.pending_tool_args.keys())
            .copied()
            .filter(|index| !self.tools.contains_key(index))
            .collect();
        pending.sort_unstable();
        pending.dedup();
        for chat_index in pending {
            let id = self
                .pending_tool_ids
                .remove(&chat_index)
                .unwrap_or_default();
            let name = self
                .pending_tool_names
                .remove(&chat_index)
                .unwrap_or_default();
            let arguments = self
                .pending_tool_args
                .remove(&chat_index)
                .unwrap_or_default();
            self.announce_tool_item(chat_index, Some(&id), Some(&name), &mut out)?;
            self.emit_tool_arguments(chat_index, &arguments, &mut out)?;
        }

        let no_arguments: Vec<u32> = self
            .tools
            .keys()
            .copied()
            .filter(|index| !self.tool_args_seen.contains(index))
            .collect();
        for chat_index in no_arguments {
            self.emit_tool_arguments(chat_index, "{}", &mut out)?;
        }

        if let Some(index) = self.message_index {
            let item_id = format!("{}-message", self.id);
            let text = self.text.clone();
            out.push(self.event(
                "response.output_text.done",
                json!({
                    "type": "response.output_text.done",
                    "item_id": item_id,
                    "output_index": index,
                    "content_index": 0,
                    "text": text
                }),
            )?);
            out.push(self.event(
                "response.content_part.done",
                json!({
                    "type": "response.content_part.done",
                    "item_id": item_id,
                    "output_index": index,
                    "content_index": 0,
                    "part": {"type": "output_text", "text": text}
                }),
            )?);
            out.push(self.event(
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": index,
                    "item": {
                        "type": "message",
                        "id": item_id,
                        "role": "assistant",
                        "status": "completed",
                        "content": [{"type": "output_text", "text": text}]
                    }
                }),
            )?);
        }

        if let Some(index) = self.reasoning_index {
            let item_id = format!("{}-reasoning", self.id);
            let reasoning = self.reasoning.clone();
            out.push(self.event(
                "response.reasoning_summary_text.done",
                json!({
                    "type": "response.reasoning_summary_text.done",
                    "item_id": item_id,
                    "output_index": index,
                    "summary_index": 0,
                    "text": reasoning
                }),
            )?);
            out.push(self.event(
                "response.reasoning_summary_part.done",
                json!({
                    "type": "response.reasoning_summary_part.done",
                    "item_id": item_id,
                    "output_index": index,
                    "summary_index": 0,
                    "part": {"type": "summary_text", "text": reasoning}
                }),
            )?);
            out.push(self.event(
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": index,
                    "item": {
                        "type": "reasoning",
                        "id": item_id,
                        "summary": [{"type": "summary_text", "text": reasoning}]
                    }
                }),
            )?);
        }

        let tool_states: Vec<ToolState> = self.tools.values().cloned().collect();
        for state in &tool_states {
            out.push(self.event(
                "response.function_call_arguments.done",
                json!({
                    "type": "response.function_call_arguments.done",
                    "item_id": state.item_id,
                    "output_index": state.output_index,
                    "arguments": state.arguments
                }),
            )?);
            let (item_type, item_name, namespace) = self.tool_shape(state.name.as_deref());
            let arguments = if item_type == "tool_search_call" {
                serde_json::from_str::<Value>(&state.arguments)
                    .unwrap_or_else(|_| Value::String(state.arguments.clone()))
            } else {
                Value::String(state.arguments.clone())
            };
            if item_type == "custom_tool_call" {
                out.push(self.event(
                    "response.custom_tool_call_input.done",
                    json!({
                        "type": "response.custom_tool_call_input.done",
                        "item_id": state.item_id,
                        "output_index": state.output_index,
                        "input": extract_custom_input(&state.arguments)
                    }),
                )?);
            }
            out.push(self.event(
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": state.output_index,
                    "item": {
                        "type": item_type,
                        "id": state.item_id,
                        "call_id": state.call_id,
                        "name": item_name,
                        "namespace": namespace,
                        "arguments": arguments,
                        "input": if item_type == "custom_tool_call" {
                            Some(extract_custom_input(&state.arguments))
                        } else {
                            None
                        },
                        "status": "completed"
                    }
                }),
            )?);
        }

        let status = match self.finish_reason.as_deref() {
            Some("length") | Some("content_filter") => "incomplete",
            Some(_) => "completed",
            None => {
                self.report.warn(
                    WarningCode::Synthesized,
                    Side::Target,
                    "/status",
                    "Chat stream ended without finish_reason; using completed",
                );
                "completed"
            }
        };
        let output = self.output_json();
        out.push(self.event(
            "response.completed",
            json!({
                "type": "response.completed",
                "response": self.response_json(status, output)
            }),
        )?);
        Ok(out)
    }

    pub(super) fn output_json(&self) -> Vec<Value> {
        let mut output = Vec::new();
        if let Some(index) = self.reasoning_index {
            output.push((
                index,
                json!({
                    "type": "reasoning",
                    "id": format!("{}-reasoning", self.id),
                    "summary": [{"type": "summary_text", "text": self.reasoning}]
                }),
            ));
        }
        if let Some(index) = self.message_index {
            output.push((
                index,
                json!({
                    "type": "message",
                    "id": format!("{}-message", self.id),
                    "role": "assistant",
                    "status": "completed",
                    "content": [{"type": "output_text", "text": self.text}]
                }),
            ));
        }
        for state in self.tools.values() {
            let (item_type, item_name, namespace) = self.tool_shape(state.name.as_deref());
            let arguments = if item_type == "tool_search_call" {
                serde_json::from_str::<Value>(&state.arguments)
                    .unwrap_or_else(|_| Value::String(state.arguments.clone()))
            } else {
                Value::String(state.arguments.clone())
            };
            output.push((
                state.output_index,
                json!({
                    "type": item_type,
                    "id": state.item_id,
                    "call_id": state.call_id,
                    "name": item_name,
                    "namespace": namespace,
                    "arguments": arguments,
                    "input": if item_type == "custom_tool_call" {
                        Some(extract_custom_input(&state.arguments))
                    } else {
                        None
                    },
                    "status": "completed"
                }),
            ));
        }
        output.sort_by_key(|(index, _)| *index);
        output.into_iter().map(|(_, value)| value).collect()
    }
}

fn extract_custom_input(arguments: &str) -> Value {
    let trimmed = arguments.trim();
    if trimmed.is_empty() {
        return Value::String(String::new());
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(object)) => object
            .get("input")
            .cloned()
            .unwrap_or_else(|| Value::String(trimmed.to_string())),
        Ok(value) => value,
        Err(_) => Value::String(trimmed.to_string()),
    }
}

fn chat_usage_to_responses(usage: &ChatUsage) -> Value {
    let cached = usage
        .prompt_tokens_details
        .as_ref()
        .and_then(|details| details.cached_tokens);
    let reasoning = usage
        .completion_tokens_details
        .as_ref()
        .and_then(|details| details.reasoning_tokens);
    json!({
        "input_tokens": usage.prompt_tokens,
        "output_tokens": usage.completion_tokens,
        "total_tokens": usage.total_tokens.or_else(|| Some(usage.prompt_tokens.unwrap_or(0) + usage.completion_tokens.unwrap_or(0))),
        "input_tokens_details": cached.map(|cached| json!({"cached_tokens": cached})),
        "output_tokens_details": reasoning.map(|reasoning| json!({"reasoning_tokens": reasoning}))
    })
}

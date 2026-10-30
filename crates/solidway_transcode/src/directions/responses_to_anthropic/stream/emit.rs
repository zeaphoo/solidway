//! Anthropic -> Responses 的 emit 辅助方法。

use serde_json::{Value, json};

use crate::common::frame::OutFrame;
use crate::common::usage::{positive, saturating_sum};
use crate::report::{Error, Side, WarningCode};
use crate::wire::anthropic::{AnthropicContentBlock, AnthropicUsage};

use super::{Bridge, ServerToolState, ToolState};

impl Bridge {
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
        json!({
            "id": self.id,
            "object": "response",
            "created_at": 0,
            "model": self.model,
            "status": status,
            "output": output,
            "usage": self.usage.as_ref().map(anthropic_usage_to_responses)
        })
    }

    pub(super) fn ensure_message_item(
        &mut self,
        index: u32,
        out: &mut Vec<OutFrame>,
    ) -> Result<(), Error> {
        if self.message_index.is_some() {
            return Ok(());
        }
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
        Ok(())
    }

    pub(super) fn ensure_reasoning_item(
        &mut self,
        index: u32,
        out: &mut Vec<OutFrame>,
    ) -> Result<(), Error> {
        if self.reasoning_index.is_some() {
            return Ok(());
        }
        self.reasoning_index = Some(index);
        let item_id = format!("{}-reasoning", self.id);
        out.push(self.event(
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": index,
                "item": {"type": "reasoning", "id": item_id, "summary": []}
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
        Ok(())
    }

    pub(super) fn ensure_tool_item(
        &mut self,
        index: u32,
        call_id: Option<&str>,
        name: Option<&str>,
        out: &mut Vec<OutFrame>,
    ) -> Result<(), Error> {
        if self.tools.contains_key(&index) {
            return Ok(());
        }
        let item_id = format!("{}-call-{}", self.id, index);
        self.tools.insert(
            index,
            ToolState {
                output_index: index,
                item_id: item_id.clone(),
                call_id: call_id.map(ToString::to_string),
                name: name.map(ToString::to_string),
                arguments: String::new(),
            },
        );
        out.push(self.event(
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": index,
                "item": {
                    "type": "function_call",
                    "id": item_id,
                    "call_id": call_id.unwrap_or(""),
                    "name": name.unwrap_or(""),
                    "arguments": ""
                }
            }),
        )?);
        Ok(())
    }

    pub(super) fn ensure_server_tool_item(
        &mut self,
        index: u32,
        block: &AnthropicContentBlock,
        out: &mut Vec<OutFrame>,
    ) -> Result<(), Error> {
        if self.server_tools.contains_key(&index) {
            return Ok(());
        }
        let name = block
            .name
            .clone()
            .unwrap_or_else(|| "web_search".to_string());
        let call_type = match name.as_str() {
            "web_search" => "web_search_call".to_string(),
            "x_search" => "x_search_call".to_string(),
            other => format!("{other}_call"),
        };
        let call_id = block
            .id
            .clone()
            .unwrap_or_else(|| format!("srvtoolu_{index}"));
        let input = block
            .input
            .as_ref()
            .filter(|value| !value.as_object().is_some_and(|object| object.is_empty()))
            .map(|value| serde_json::to_string(value).unwrap_or_default())
            .unwrap_or_default();
        let item_id = call_id.clone();
        self.server_tools.insert(
            index,
            ServerToolState {
                output_index: index,
                item_id: item_id.clone(),
                call_id,
                call_type: call_type.clone(),
                name,
                input,
                results: None,
                done: false,
            },
        );
        out.push(self.event(
            "response.output_item.added",
            json!({
                "type": "response.output_item.added",
                "output_index": index,
                "item": {
                    "type": call_type,
                    "id": item_id,
                    "status": "in_progress",
                    "action": {}
                }
            }),
        )?);
        Ok(())
    }

    pub(super) fn handle_server_tool_result(
        &mut self,
        index: u32,
        block: &AnthropicContentBlock,
        out: &mut Vec<OutFrame>,
    ) -> Result<(), Error> {
        let tool_use_id = block
            .tool_use_id
            .clone()
            .or_else(|| block.id.clone())
            .unwrap_or_else(|| format!("srvtoolu_{index}"));
        let Some(server_index) = self
            .server_tools
            .iter()
            .find(|(_, state)| state.call_id == tool_use_id)
            .map(|(index, _)| *index)
        else {
            return Ok(());
        };
        self.result_blocks.insert(index, server_index);
        let results = server_tool_results(block);
        if let Some(state) = self.server_tools.get_mut(&server_index) {
            state.results = Some(results);
        }
        self.close_server_tool(server_index, out)
    }

    pub(super) fn close_server_tool(
        &mut self,
        index: u32,
        out: &mut Vec<OutFrame>,
    ) -> Result<(), Error> {
        let Some(state) = self.server_tools.get_mut(&index) else {
            return Ok(());
        };
        if state.done {
            return Ok(());
        }
        state.done = true;
        let state = state.clone();
        let mut item = json!({
            "type": state.call_type,
            "id": state.item_id,
            "status": "completed",
            "action": server_tool_action(&state.input)
        });
        if let Some(results) = state.results {
            item.as_object_mut()
                .expect("object")
                .insert("results".to_string(), results);
        }
        out.push(self.event(
            "response.output_item.done",
            json!({
                "type": "response.output_item.done",
                "output_index": state.output_index,
                "item": item
            }),
        )?);
        Ok(())
    }

    pub(super) fn close_block(&mut self, index: u32, out: &mut Vec<OutFrame>) -> Result<(), Error> {
        if let Some(state) = self.server_tools.get(&index) {
            if !matches!(state.name.as_str(), "web_search" | "x_search") {
                self.close_server_tool(index, out)?;
            }
            return Ok(());
        }
        if self.result_blocks.remove(&index).is_some() {
            return Ok(());
        }
        if self.message_index == Some(index) {
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
        } else if self.reasoning_index == Some(index) {
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
        } else if let Some(state) = self.tools.get(&index).cloned() {
            out.push(self.event(
                "response.function_call_arguments.done",
                json!({
                    "type": "response.function_call_arguments.done",
                    "item_id": state.item_id,
                    "output_index": index,
                    "arguments": state.arguments
                }),
            )?);
            out.push(self.event(
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": index,
                    "item": {
                        "type": "function_call",
                        "id": state.item_id,
                        "call_id": state.call_id,
                        "name": state.name,
                        "arguments": state.arguments,
                        "status": "completed"
                    }
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
        let mut indices: Vec<u32> = Vec::new();
        if let Some(index) = self.message_index {
            indices.push(index);
        }
        if let Some(index) = self.reasoning_index {
            indices.push(index);
        }
        for index in self.tools.keys().copied() {
            indices.push(index);
        }
        for index in self.server_tools.keys().copied() {
            indices.push(index);
        }
        indices.sort_unstable();
        indices.dedup();
        for index in indices {
            self.close_block(index, &mut out)?;
        }
        let pending: Vec<u32> = self
            .server_tools
            .iter()
            .filter(|(_, state)| !state.done)
            .map(|(index, _)| *index)
            .collect();
        for index in pending {
            self.close_server_tool(index, &mut out)?;
        }
        let status = match self.finish_reason.as_deref() {
            Some("max_tokens") | Some("refusal") => "incomplete",
            Some(_) => "completed",
            None => {
                self.report.warn(
                    WarningCode::Synthesized,
                    Side::Target,
                    "/status",
                    "Anthropic stream ended without stop_reason; using completed",
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
            output.push((
                state.output_index,
                json!({
                    "type": "function_call",
                    "id": state.item_id,
                    "call_id": state.call_id,
                    "name": state.name,
                    "arguments": state.arguments,
                    "status": "completed"
                }),
            ));
        }
        for state in self.server_tools.values() {
            let mut item = json!({
                "type": state.call_type,
                "id": state.item_id,
                "call_id": state.call_id,
                "status": "completed",
                "action": server_tool_action(&state.input)
            });
            if let Some(results) = state.results.clone() {
                item.as_object_mut()
                    .expect("object")
                    .insert("results".to_string(), results);
            }
            output.push((state.output_index, item));
        }
        output.sort_by_key(|(index, _)| *index);
        output.into_iter().map(|(_, value)| value).collect()
    }
}

fn server_tool_results(block: &AnthropicContentBlock) -> Value {
    let value = match block.content.as_ref() {
        Some(crate::wire::anthropic::AnthropicToolResultContent::Other(value)) => value.clone(),
        Some(crate::wire::anthropic::AnthropicToolResultContent::Text(text)) => {
            json!([{"type": "text", "text": text}])
        }
        Some(crate::wire::anthropic::AnthropicToolResultContent::Blocks(blocks)) => {
            serde_json::to_value(blocks).unwrap_or_else(|_| json!([]))
        }
        None => json!([]),
    };
    match value {
        Value::Array(items) => Value::Array(items),
        Value::Null => json!([]),
        Value::Object(mut object) => {
            if let Some(Value::Array(items)) = object
                .remove("results")
                .or_else(|| object.remove("sources"))
            {
                Value::Array(items)
            } else {
                json!([Value::Object(object)])
            }
        }
        other => json!([other]),
    }
}

fn server_tool_action(input: &str) -> Value {
    let Ok(value) = serde_json::from_str::<Value>(input) else {
        return json!({});
    };
    let Value::Object(mut object) = value else {
        return json!({});
    };
    if object.contains_key("query") {
        object.entry("type").or_insert_with(|| json!("search"));
    }
    Value::Object(object)
}

fn anthropic_usage_to_responses(usage: &AnthropicUsage) -> Value {
    let cache_read = usage.cache_read_input_tokens.unwrap_or(0);
    let cache_creation = usage.cache_creation_input_tokens.unwrap_or(0);
    let input = saturating_sum(&[usage.input_tokens, Some(cache_read), Some(cache_creation)]);
    let output = usage.output_tokens.unwrap_or(0);
    json!({
        "input_tokens": input,
        "output_tokens": output,
        "total_tokens": saturating_sum(&[Some(input), Some(output)]),
        "cache_creation_input_tokens": positive(cache_creation),
        "input_tokens_details": positive(cache_read).map(|cache_read| json!({"cached_tokens": cache_read}))
    })
}

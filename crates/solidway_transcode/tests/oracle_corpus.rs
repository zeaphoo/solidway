//! sub2api oracle 批量 fixture 对照。
//!
//! 参考文件由 sub2api `dba770d56` 的 `cmd/transcode-oracle` 生成。
//! 对全部用例比较语义摘要；对已确认无已知差异的用例追加规范化 JSON 全量对拍。

use std::collections::BTreeMap;

use serde_json::{Map, Value};
use solidway_transcode::protocol::ProtocolId;
use solidway_transcode::registry::{Plan, Transcoded, direction};
use solidway_transcode::report::TranscodePolicy;

const ANTHROPIC: &str = include_str!("fixtures/anthropic_request.json");
const ANTHROPIC_MULTIMODAL: &str = include_str!("fixtures/multimodal_anthropic_request.json");
const ANTHROPIC_PARALLEL: &str = include_str!("fixtures/parallel_tools_anthropic_request.json");
const CHAT: &str = include_str!("fixtures/chat_request.json");
const CHAT_MULTIMODAL: &str = include_str!("fixtures/multimodal_chat_request.json");
const CHAT_PARALLEL: &str = include_str!("fixtures/parallel_tools_chat_request.json");
const RESPONSES: &str = include_str!("fixtures/responses_request.json");
const RESPONSES_MULTIMODAL: &str = include_str!("fixtures/multimodal_responses_request.json");
const RESPONSES_PARALLEL: &str = include_str!("fixtures/parallel_tools_responses_request.json");

struct Case {
    direction: &'static str,
    src: ProtocolId,
    dst: ProtocolId,
    fixture: &'static str,
    reference: &'static str,
    exact: bool,
}

const CASES: &[Case] = &[
    Case {
        direction: "anthropic_to_chat",
        src: ProtocolId::ANTHROPIC_MESSAGES,
        dst: ProtocolId::OPENAI_CHAT,
        fixture: ANTHROPIC,
        reference: include_str!("reference/oracle/anthropic_to_chat/anthropic_request.json"),
        exact: false,
    },
    Case {
        direction: "anthropic_to_chat",
        src: ProtocolId::ANTHROPIC_MESSAGES,
        dst: ProtocolId::OPENAI_CHAT,
        fixture: ANTHROPIC_MULTIMODAL,
        reference: include_str!(
            "reference/oracle/anthropic_to_chat/multimodal_anthropic_request.json"
        ),
        exact: false,
    },
    Case {
        direction: "anthropic_to_chat",
        src: ProtocolId::ANTHROPIC_MESSAGES,
        dst: ProtocolId::OPENAI_CHAT,
        fixture: ANTHROPIC_PARALLEL,
        reference: include_str!(
            "reference/oracle/anthropic_to_chat/parallel_tools_anthropic_request.json"
        ),
        exact: false,
    },
    Case {
        direction: "anthropic_to_responses",
        src: ProtocolId::ANTHROPIC_MESSAGES,
        dst: ProtocolId::OPENAI_RESPONSES,
        fixture: ANTHROPIC,
        reference: include_str!("reference/oracle/anthropic_to_responses/anthropic_request.json"),
        exact: false,
    },
    Case {
        direction: "anthropic_to_responses",
        src: ProtocolId::ANTHROPIC_MESSAGES,
        dst: ProtocolId::OPENAI_RESPONSES,
        fixture: ANTHROPIC_MULTIMODAL,
        reference: include_str!(
            "reference/oracle/anthropic_to_responses/multimodal_anthropic_request.json"
        ),
        exact: false,
    },
    Case {
        direction: "anthropic_to_responses",
        src: ProtocolId::ANTHROPIC_MESSAGES,
        dst: ProtocolId::OPENAI_RESPONSES,
        fixture: ANTHROPIC_PARALLEL,
        reference: include_str!(
            "reference/oracle/anthropic_to_responses/parallel_tools_anthropic_request.json"
        ),
        exact: false,
    },
    Case {
        direction: "chat_to_responses",
        src: ProtocolId::OPENAI_CHAT,
        dst: ProtocolId::OPENAI_RESPONSES,
        fixture: CHAT,
        reference: include_str!("reference/oracle/chat_to_responses/chat_request.json"),
        exact: false,
    },
    Case {
        direction: "chat_to_responses",
        src: ProtocolId::OPENAI_CHAT,
        dst: ProtocolId::OPENAI_RESPONSES,
        fixture: CHAT_MULTIMODAL,
        reference: include_str!("reference/oracle/chat_to_responses/multimodal_chat_request.json"),
        exact: false,
    },
    Case {
        direction: "chat_to_responses",
        src: ProtocolId::OPENAI_CHAT,
        dst: ProtocolId::OPENAI_RESPONSES,
        fixture: CHAT_PARALLEL,
        reference: include_str!(
            "reference/oracle/chat_to_responses/parallel_tools_chat_request.json"
        ),
        exact: true,
    },
    Case {
        direction: "responses_to_chat",
        src: ProtocolId::OPENAI_RESPONSES,
        dst: ProtocolId::OPENAI_CHAT,
        fixture: RESPONSES,
        reference: include_str!("reference/oracle/responses_to_chat/responses_request.json"),
        exact: false,
    },
    Case {
        direction: "responses_to_chat",
        src: ProtocolId::OPENAI_RESPONSES,
        dst: ProtocolId::OPENAI_CHAT,
        fixture: RESPONSES_MULTIMODAL,
        reference: include_str!(
            "reference/oracle/responses_to_chat/multimodal_responses_request.json"
        ),
        exact: false,
    },
    Case {
        direction: "responses_to_chat",
        src: ProtocolId::OPENAI_RESPONSES,
        dst: ProtocolId::OPENAI_CHAT,
        fixture: RESPONSES_PARALLEL,
        reference: include_str!(
            "reference/oracle/responses_to_chat/parallel_tools_responses_request.json"
        ),
        exact: true,
    },
    Case {
        direction: "responses_to_anthropic",
        src: ProtocolId::OPENAI_RESPONSES,
        dst: ProtocolId::ANTHROPIC_MESSAGES,
        fixture: RESPONSES,
        reference: include_str!("reference/oracle/responses_to_anthropic/responses_request.json"),
        exact: false,
    },
    Case {
        direction: "responses_to_anthropic",
        src: ProtocolId::OPENAI_RESPONSES,
        dst: ProtocolId::ANTHROPIC_MESSAGES,
        fixture: RESPONSES_MULTIMODAL,
        reference: include_str!(
            "reference/oracle/responses_to_anthropic/multimodal_responses_request.json"
        ),
        exact: true,
    },
    Case {
        direction: "responses_to_anthropic",
        src: ProtocolId::OPENAI_RESPONSES,
        dst: ProtocolId::ANTHROPIC_MESSAGES,
        fixture: RESPONSES_PARALLEL,
        reference: include_str!(
            "reference/oracle/responses_to_anthropic/parallel_tools_responses_request.json"
        ),
        exact: false,
    },
];

#[derive(Debug, PartialEq, Eq)]
struct Summary {
    text: String,
    tool_calls: usize,
    tool_results: usize,
    finish: String,
}

fn ours(src: ProtocolId, dst: ProtocolId, body: &str) -> Value {
    let plan = Plan {
        src,
        dst,
        policy: TranscodePolicy::BestEffort,
    };
    let Transcoded { body, .. } = direction(src, dst)
        .expect("direction")
        .request(body.as_bytes(), &plan)
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

fn summary(target: ProtocolId, value: &Value) -> Summary {
    match target {
        ProtocolId::OPENAI_CHAT => summary_chat(value),
        ProtocolId::ANTHROPIC_MESSAGES => summary_anthropic(value),
        ProtocolId::OPENAI_RESPONSES => summary_responses(value),
        other => panic!("unsupported target {other}"),
    }
}

fn summary_chat(value: &Value) -> Summary {
    let mut text = String::new();
    let mut tool_calls = 0;
    let mut tool_results = 0;
    for message in value["messages"].as_array().into_iter().flatten() {
        if let Some(content) = message.get("content") {
            collect_text(content, &mut text);
        }
        tool_calls += message["tool_calls"].as_array().map_or(0, Vec::len);
        if message["role"] == "tool" {
            tool_results += 1;
        }
    }
    Summary {
        text: normalize_text(text),
        tool_calls,
        tool_results,
        finish: String::new(),
    }
}

fn summary_anthropic(value: &Value) -> Summary {
    let mut text = String::new();
    if let Some(system) = value.get("system") {
        collect_text(system, &mut text);
    }
    let mut tool_calls = 0;
    let mut tool_results = 0;
    for block in value["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|message| message["content"].as_array().into_iter().flatten())
    {
        let type_ = block["type"].as_str().unwrap_or("");
        if type_ == "text" {
            if let Some(part) = block["text"].as_str() {
                text.push_str(part);
            }
        } else if matches!(type_, "tool_use" | "server_tool_use") {
            tool_calls += 1;
        } else if type_ == "tool_result" || type_.ends_with("_tool_result") {
            tool_results += 1;
        }
    }
    Summary {
        text: normalize_text(text),
        tool_calls,
        tool_results,
        finish: String::new(),
    }
}

fn summary_responses(value: &Value) -> Summary {
    let mut text = String::new();
    if let Some(instructions) = value.get("instructions") {
        collect_text(instructions, &mut text);
    }
    let mut tool_calls = 0;
    let mut tool_results = 0;
    for item in value["input"].as_array().into_iter().flatten() {
        match item["type"].as_str().unwrap_or("") {
            "message" => {
                if let Some(content) = item.get("content") {
                    collect_text(content, &mut text);
                }
            }
            "function_call" | "custom_tool_call" | "tool_search_call" => tool_calls += 1,
            "function_call_output" | "custom_tool_call_output" => tool_results += 1,
            _ => {}
        }
    }
    Summary {
        text: normalize_text(text),
        tool_calls,
        tool_results,
        finish: String::new(),
    }
}

fn normalize_text(text: String) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn collect_text(value: &Value, out: &mut String) {
    match value {
        Value::String(text) => out.push_str(text),
        Value::Array(parts) => {
            for part in parts {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    out.push_str(text);
                }
            }
        }
        _ => {}
    }
}

fn canonical(value: &Value) -> String {
    match value {
        Value::Object(object) => {
            let sorted: BTreeMap<&String, Value> =
                object.iter().map(|(k, v)| (k, v.clone())).collect();
            let mut map = Map::new();
            for (key, value) in sorted {
                map.insert(
                    key.clone(),
                    serde_json::from_str(&canonical(&value)).unwrap(),
                );
            }
            serde_json::to_string(&Value::Object(map)).unwrap()
        }
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        other => serde_json::to_string(other).unwrap(),
    }
}

#[test]
fn oracle_corpus_matches_sub2api_semantics() {
    for case in CASES {
        let ours = ours(case.src, case.dst, case.fixture);
        let reference: Value = serde_json::from_str(case.reference).unwrap();
        assert_eq!(
            summary(case.dst, &ours),
            summary(case.dst, &reference),
            "semantic mismatch for {}",
            case.direction
        );
        if case.exact {
            assert_eq!(
                canonical(&ours),
                canonical(&reference),
                "canonical mismatch for {}",
                case.direction
            );
        }
    }
}

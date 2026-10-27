//! Responses -> Chat Completions。

mod request;
mod response;
mod stream;

use crate::common::schema::flatten_namespace_name;
use crate::protocol::{DirectionCaps, ProtocolId, RequestCaps, ResponseCaps, StreamCaps, Support};
use crate::registry::{Direction, NamespaceTool, Plan, ResponseContext, StreamBridge, Transcoded};
use crate::report::Error;

pub(crate) struct ResponsesToChat;

pub(crate) static DIRECTION: ResponsesToChat = ResponsesToChat;

pub(crate) const CAPS: DirectionCaps = DirectionCaps {
    request: RequestCaps {
        structured_output: Support::Lossy(
            "Responses text.format is mapped to Chat response_format",
        ),
        audio_input: Support::Unsupported("Responses input_audio is not mapped"),
        custom_tools: Support::Unsupported("Chat has no custom/grammar tools"),
        hosted_tools: Support::Unsupported("Chat has no hosted/server tools"),
        cache_control: Support::Lossy("Responses prompt_cache_key is dropped"),
        thinking: Support::Lossy("Responses reasoning is mapped to reasoning_effort"),
        ..RequestCaps::DEFAULT
    },
    response: ResponseCaps {
        refusal: Support::Lossy("refusal is mapped to a text block"),
        thinking_signature: Support::Lossy("Chat has no encrypted reasoning content"),
        citations: Support::Lossy("Chat has no citations field"),
        server_tools: Support::Unsupported("Chat has no server tool items"),
        ..ResponseCaps::DEFAULT
    },
    stream: StreamCaps {
        thinking: Support::Lossy("Responses reasoning summary is mapped to reasoning_content"),
        parallel_tool_calls: Support::Lossy(
            "Responses output items are flattened into Chat tool_calls",
        ),
        keepalive: Support::Unsupported("Chat stream has no ping event"),
        ..StreamCaps::DEFAULT
    },
};

impl Direction for ResponsesToChat {
    fn src(&self) -> ProtocolId {
        ProtocolId::OPENAI_RESPONSES
    }

    fn dst(&self) -> ProtocolId {
        ProtocolId::OPENAI_CHAT
    }

    fn caps(&self) -> &'static DirectionCaps {
        &CAPS
    }

    fn request(&self, body: &[u8], plan: &Plan) -> Result<Transcoded<Vec<u8>>, Error> {
        let req: crate::wire::openai_responses::ResponsesRequest =
            serde_json::from_slice(body).map_err(|err| Error::Invalid(err.to_string()))?;
        let (out, report) = request::convert(req, plan)?;
        Ok(Transcoded {
            body: serde_json::to_vec(&out)?,
            report,
        })
    }

    fn response(&self, body: &[u8], plan: &Plan) -> Result<Transcoded<Vec<u8>>, Error> {
        response::convert(body, plan, &ResponseContext::default())
    }

    fn response_context(&self, request_body: &[u8]) -> ResponseContext {
        let Ok(req) =
            serde_json::from_slice::<crate::wire::openai_responses::ResponsesRequest>(request_body)
        else {
            return ResponseContext::default();
        };
        let mut context = ResponseContext::default();
        for tool in &req.tools {
            match tool.type_.as_str() {
                "custom" => {
                    if let Some(name) = tool.name.as_ref() {
                        context.custom_tools.insert(name.clone());
                    }
                }
                "tool_search" => context.tool_search = true,
                "namespace" => {
                    let Some(namespace) = tool.name.as_ref() else {
                        continue;
                    };
                    let children = if tool.tools.is_empty() {
                        &tool.children
                    } else {
                        &tool.tools
                    };
                    for child in children {
                        if child.type_ != "function" {
                            continue;
                        }
                        let Some(child_name) = child.name.as_ref() else {
                            continue;
                        };
                        let flat = flatten_namespace_name(namespace, child_name);
                        context.namespaces.insert(
                            flat,
                            NamespaceTool {
                                namespace: namespace.clone(),
                                name: child_name.clone(),
                            },
                        );
                    }
                }
                _ => {}
            }
        }
        context
    }

    fn response_with_context(
        &self,
        body: &[u8],
        plan: &Plan,
        context: &ResponseContext,
    ) -> Result<Transcoded<Vec<u8>>, Error> {
        response::convert(body, plan, context)
    }

    fn stream(&self, plan: &Plan) -> Result<Box<dyn StreamBridge>, Error> {
        Ok(Box::new(stream::Bridge::new(
            plan,
            &ResponseContext::default(),
        )))
    }

    fn stream_with_context(
        &self,
        plan: &Plan,
        context: &ResponseContext,
    ) -> Result<Box<dyn StreamBridge>, Error> {
        Ok(Box::new(stream::Bridge::new(plan, context)))
    }
}

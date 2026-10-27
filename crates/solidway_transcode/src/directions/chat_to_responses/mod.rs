//! Chat Completions -> Responses。

mod request;
mod response;
mod stream;

use crate::protocol::{DirectionCaps, ProtocolId, RequestCaps, ResponseCaps, StreamCaps, Support};
use crate::registry::{Direction, Plan, StreamBridge, Transcoded};
use crate::report::Error;

pub(crate) struct ChatToResponses;

pub(crate) static DIRECTION: ChatToResponses = ChatToResponses;

pub(crate) const CAPS: DirectionCaps = DirectionCaps {
    request: RequestCaps {
        documents: Support::Emulated,
        cache_control: Support::Lossy("Chat prompt cache breakpoints are dropped"),
        thinking: Support::Lossy("Chat reasoning_effort is mapped to Responses reasoning"),
        structured_output: Support::Lossy(
            "Chat response_format is mapped to Responses text.format",
        ),
        audio_input: Support::Unsupported("Responses has no input_audio content part"),
        custom_tools: Support::Unsupported("Chat has no custom/grammar tools"),
        hosted_tools: Support::Unsupported("Chat has no hosted/server tools"),
        stop: Support::Unsupported("Responses has no stop"),
        top_k: Support::Unsupported("Responses has no top_k"),
        seed: Support::Unsupported("Responses has no seed"),
        penalties: Support::Unsupported("Responses has no penalties"),
        n_choices: Support::Unsupported("Responses has no n"),
        logprobs: Support::Unsupported("Responses has no logprobs"),
        ..RequestCaps::DEFAULT
    },
    response: ResponseCaps {
        refusal: Support::Lossy("refusal is mapped to a text block"),
        thinking_signature: Support::Lossy("Chat has no thinking signature field"),
        citations: Support::Lossy("Chat has no citations field"),
        server_tools: Support::Lossy("web_search_call is mapped to Chat tool_calls"),
        ..ResponseCaps::DEFAULT
    },
    stream: StreamCaps {
        thinking: Support::Lossy("Responses reasoning summary is mapped to reasoning_content"),
        parallel_tool_calls: Support::Lossy(
            "Chat stream can carry parallel tool calls; Responses events are per item",
        ),
        keepalive: Support::Unsupported("Chat stream has no ping event"),
        ..StreamCaps::DEFAULT
    },
};

impl Direction for ChatToResponses {
    fn src(&self) -> ProtocolId {
        ProtocolId::OPENAI_CHAT
    }

    fn dst(&self) -> ProtocolId {
        ProtocolId::OPENAI_RESPONSES
    }

    fn caps(&self) -> &'static DirectionCaps {
        &CAPS
    }

    fn request(&self, body: &[u8], plan: &Plan) -> Result<Transcoded<Vec<u8>>, Error> {
        let req: crate::wire::openai_chat::ChatRequest =
            serde_json::from_slice(body).map_err(|err| Error::Invalid(err.to_string()))?;
        let (out, report) = request::convert(req, plan)?;
        Ok(Transcoded {
            body: serde_json::to_vec(&out)?,
            report,
        })
    }

    fn response(&self, body: &[u8], plan: &Plan) -> Result<Transcoded<Vec<u8>>, Error> {
        response::convert(body, plan)
    }

    fn stream(&self, plan: &Plan) -> Result<Box<dyn StreamBridge>, Error> {
        Ok(Box::new(stream::Bridge::new(plan)))
    }
}

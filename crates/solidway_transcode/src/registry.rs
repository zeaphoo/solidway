//! 方向注册表与公开契约。

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::common::frame::{FrameDecoder, FrameEncoder};
use crate::dialect::{self, Dialect};
use crate::protocol::{DirectionCaps, ProtocolId};
use crate::report::{Error, Report, TranscodePolicy};

pub use crate::common::frame::{InFrame, OutFrame};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    /// 客户端格式（入站）。
    pub src: ProtocolId,
    /// 上游协议。
    pub dst: ProtocolId,
    pub policy: TranscodePolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceTool {
    pub namespace: String,
    pub name: String,
}

/// 响应转换所需的请求侧工具上下文（custom / namespace / tool_search）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResponseContext {
    pub custom_tools: HashSet<String>,
    pub tool_search: bool,
    pub namespaces: HashMap<String, NamespaceTool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcoded<T> {
    pub body: T,
    pub report: Report,
}

pub trait StreamBridge {
    fn push(&mut self, frame: InFrame) -> Result<Vec<OutFrame>, Error>;
    fn finish(&mut self) -> Result<Vec<OutFrame>, Error>;
    fn report(&self) -> &Report;
}

pub trait Direction: Send + Sync {
    fn src(&self) -> ProtocolId;
    fn dst(&self) -> ProtocolId;
    fn caps(&self) -> &'static DirectionCaps;

    /// `src` 请求 -> `dst` 请求。
    fn request(&self, body: &[u8], plan: &Plan) -> Result<Transcoded<Vec<u8>>, Error>;

    /// `dst` 响应 -> `src` 响应。
    fn response(&self, body: &[u8], plan: &Plan) -> Result<Transcoded<Vec<u8>>, Error>;

    /// 从客户端请求提取响应转换所需的工具上下文。
    fn response_context(&self, request_body: &[u8]) -> ResponseContext {
        let _ = request_body;
        ResponseContext::default()
    }

    /// 带请求上下文的响应转换；默认忽略上下文。
    fn response_with_context(
        &self,
        body: &[u8],
        plan: &Plan,
        context: &ResponseContext,
    ) -> Result<Transcoded<Vec<u8>>, Error> {
        let _ = context;
        self.response(body, plan)
    }

    /// `dst` 流式帧 -> `src` 流式帧。
    fn stream(&self, plan: &Plan) -> Result<Box<dyn StreamBridge>, Error>;

    /// 带请求上下文的流式转换；默认忽略上下文。
    fn stream_with_context(
        &self,
        plan: &Plan,
        context: &ResponseContext,
    ) -> Result<Box<dyn StreamBridge>, Error> {
        let _ = context;
        self.stream(plan)
    }
}

static PUBLIC_DIRECTIONS: &[&dyn Direction] = &[
    &crate::directions::chat_to_anthropic::DIRECTION,
    &crate::directions::anthropic_to_chat::DIRECTION,
    &crate::directions::chat_to_responses::DIRECTION,
    &crate::directions::responses_to_chat::DIRECTION,
    &crate::directions::anthropic_to_responses::DIRECTION,
    &crate::directions::responses_to_anthropic::DIRECTION,
];

/// 查找方向。未实现的方向返回 `None`，由调用方决定报错或回退。
pub fn direction(src: ProtocolId, dst: ProtocolId) -> Option<&'static dyn Direction> {
    if let Some(direction) = PUBLIC_DIRECTIONS
        .iter()
        .copied()
        .find(|direction| direction.src() == src && direction.dst() == dst)
    {
        return Some(direction);
    }
    composed()
        .iter()
        .map(Box::as_ref)
        .find(|direction| direction.src() == src && direction.dst() == dst)
}

/// 全部已注册方向（公开 + 方言组合），用于矩阵测试与运维自检。
pub fn all_directions() -> Vec<&'static dyn Direction> {
    let mut directions: Vec<&'static dyn Direction> = PUBLIC_DIRECTIONS.to_vec();
    directions.extend(composed().iter().map(Box::as_ref));
    directions
}

pub fn response_context(src: ProtocolId, dst: ProtocolId, request_body: &[u8]) -> ResponseContext {
    direction(src, dst)
        .map(|direction| direction.response_context(request_body))
        .unwrap_or_default()
}

pub fn response_with_context(
    src: ProtocolId,
    dst: ProtocolId,
    body: &[u8],
    plan: &Plan,
    context: &ResponseContext,
) -> Result<Transcoded<Vec<u8>>, Error> {
    let direction = direction(src, dst).ok_or(Error::NoDirection { src, dst })?;
    direction.response_with_context(body, plan, context)
}

fn public_direction(src: ProtocolId, dst: ProtocolId) -> Option<&'static dyn Direction> {
    PUBLIC_DIRECTIONS
        .iter()
        .copied()
        .find(|direction| direction.src() == src && direction.dst() == dst)
}

fn composed() -> &'static [Box<dyn Direction>] {
    static COMPOSED: OnceLock<Vec<Box<dyn Direction>>> = OnceLock::new();
    COMPOSED.get_or_init(build_composed)
}

fn build_composed() -> Vec<Box<dyn Direction>> {
    let mut list: Vec<Box<dyn Direction>> = Vec::new();
    let dialects = dialect::all();

    for dialect in dialects {
        let dialect_id = dialect.id();
        let base = dialect.base();

        list.push(Box::new(Composed::new(
            base,
            dialect_id,
            None,
            None,
            Some(*dialect),
            Some(*dialect),
            None,
            Some(*dialect),
            None,
        )));
        list.push(Box::new(Composed::new(
            dialect_id,
            base,
            None,
            Some(*dialect),
            None,
            None,
            Some(*dialect),
            None,
            Some(*dialect),
        )));

        for public in ProtocolId::PUBLIC {
            if public == base {
                continue;
            }
            if let Some(base_direction) = public_direction(public, base) {
                list.push(Box::new(Composed::new(
                    public,
                    dialect_id,
                    Some(base_direction),
                    None,
                    Some(*dialect),
                    Some(*dialect),
                    None,
                    Some(*dialect),
                    None,
                )));
            }
            if let Some(base_direction) = public_direction(base, public) {
                list.push(Box::new(Composed::new(
                    dialect_id,
                    public,
                    Some(base_direction),
                    Some(*dialect),
                    None,
                    None,
                    Some(*dialect),
                    None,
                    Some(*dialect),
                )));
            }
        }
    }

    for (index, first) in dialects.iter().enumerate() {
        for second in dialects.iter().skip(index + 1) {
            let first_base = first.base();
            let second_base = second.base();
            let forward_base = if first_base == second_base {
                None
            } else {
                public_direction(first_base, second_base)
            };
            let backward_base = if first_base == second_base {
                None
            } else {
                public_direction(second_base, first_base)
            };

            list.push(Box::new(Composed::new(
                first.id(),
                second.id(),
                forward_base,
                Some(*first),
                Some(*second),
                Some(*second),
                Some(*first),
                Some(*second),
                Some(*first),
            )));
            list.push(Box::new(Composed::new(
                second.id(),
                first.id(),
                backward_base,
                Some(*second),
                Some(*first),
                Some(*first),
                Some(*second),
                Some(*first),
                Some(*second),
            )));
        }
    }

    list
}

struct Composed {
    src: ProtocolId,
    dst: ProtocolId,
    base: Option<&'static dyn Direction>,
    request_pre: Option<&'static dyn Dialect>,
    request_post: Option<&'static dyn Dialect>,
    response_pre: Option<&'static dyn Dialect>,
    response_post: Option<&'static dyn Dialect>,
    stream_pre: Option<&'static dyn Dialect>,
    stream_post: Option<&'static dyn Dialect>,
    caps: &'static DirectionCaps,
}

impl Composed {
    #[allow(clippy::too_many_arguments)]
    fn new(
        src: ProtocolId,
        dst: ProtocolId,
        base: Option<&'static dyn Direction>,
        request_pre: Option<&'static dyn Dialect>,
        request_post: Option<&'static dyn Dialect>,
        response_pre: Option<&'static dyn Dialect>,
        response_post: Option<&'static dyn Dialect>,
        stream_pre: Option<&'static dyn Dialect>,
        stream_post: Option<&'static dyn Dialect>,
    ) -> Self {
        let mut caps = base.map_or(DirectionCaps::DEFAULT, |direction| *direction.caps());
        for dialect in [
            request_pre,
            request_post,
            response_pre,
            response_post,
            stream_pre,
            stream_post,
        ]
        .into_iter()
        .flatten()
        {
            caps = caps.narrow(dialect.caps());
        }
        let caps = Box::leak(Box::new(caps));
        Self {
            src,
            dst,
            base,
            request_pre,
            request_post,
            response_pre,
            response_post,
            stream_pre,
            stream_post,
            caps,
        }
    }
}

impl Composed {
    fn response_impl(
        &self,
        body: &[u8],
        plan: &Plan,
        context: Option<&ResponseContext>,
    ) -> Result<Transcoded<Vec<u8>>, Error> {
        let mut body = body.to_vec();
        let mut report = Report::default();
        if let Some(dialect) = self.response_pre {
            let (next, next_report) = dialect.decode_response(&body)?;
            body = next;
            report.merge(next_report);
        }
        if let Some(base) = self.base {
            let intermediate = match context {
                Some(context) => base.response_with_context(&body, plan, context)?,
                None => base.response(&body, plan)?,
            };
            body = intermediate.body;
            report.merge(intermediate.report);
        }
        if let Some(dialect) = self.response_post {
            let (next, next_report) = dialect.encode_response(&body)?;
            body = next;
            report.merge(next_report);
        }
        Ok(Transcoded { body, report })
    }

    fn stream_impl(
        &self,
        plan: &Plan,
        context: Option<&ResponseContext>,
    ) -> Result<Box<dyn StreamBridge>, Error> {
        let bridge = match (self.base, context) {
            (Some(base), Some(context)) => Some(base.stream_with_context(plan, context)?),
            (Some(base), None) => Some(base.stream(plan)?),
            (None, _) => None,
        };
        Ok(Box::new(ComposedStream {
            bridge,
            pre: self.stream_pre,
            post: self.stream_post,
            report: Report::default(),
        }))
    }
}

impl Direction for Composed {
    fn src(&self) -> ProtocolId {
        self.src
    }

    fn dst(&self) -> ProtocolId {
        self.dst
    }

    fn caps(&self) -> &'static DirectionCaps {
        self.caps
    }

    fn request(&self, body: &[u8], plan: &Plan) -> Result<Transcoded<Vec<u8>>, Error> {
        let mut body = body.to_vec();
        let mut report = Report::default();
        if let Some(dialect) = self.request_pre {
            let (next, next_report) = dialect.decode_request(&body)?;
            body = next;
            report.merge(next_report);
        }
        if let Some(base) = self.base {
            let intermediate = base.request(&body, plan)?;
            body = intermediate.body;
            report.merge(intermediate.report);
        }
        if let Some(dialect) = self.request_post {
            let (next, next_report) = dialect.encode_request(&body)?;
            body = next;
            report.merge(next_report);
        }
        Ok(Transcoded { body, report })
    }

    fn response(&self, body: &[u8], plan: &Plan) -> Result<Transcoded<Vec<u8>>, Error> {
        self.response_impl(body, plan, None)
    }

    fn response_context(&self, request_body: &[u8]) -> ResponseContext {
        self.base
            .map(|base| base.response_context(request_body))
            .unwrap_or_default()
    }

    fn response_with_context(
        &self,
        body: &[u8],
        plan: &Plan,
        context: &ResponseContext,
    ) -> Result<Transcoded<Vec<u8>>, Error> {
        self.response_impl(body, plan, Some(context))
    }

    fn stream(&self, plan: &Plan) -> Result<Box<dyn StreamBridge>, Error> {
        self.stream_impl(plan, None)
    }

    fn stream_with_context(
        &self,
        plan: &Plan,
        context: &ResponseContext,
    ) -> Result<Box<dyn StreamBridge>, Error> {
        self.stream_impl(plan, Some(context))
    }
}

struct ComposedStream {
    bridge: Option<Box<dyn StreamBridge>>,
    pre: Option<&'static dyn Dialect>,
    post: Option<&'static dyn Dialect>,
    report: Report,
}

impl StreamBridge for ComposedStream {
    fn push(&mut self, frame: InFrame) -> Result<Vec<OutFrame>, Error> {
        let frames = if let Some(pre) = self.pre {
            let (frame, report) = pre.decode_frame(frame)?;
            self.report.merge(report);
            vec![frame]
        } else {
            vec![frame]
        };

        let mut out = Vec::new();
        for frame in frames {
            let frames = if let Some(bridge) = self.bridge.as_mut() {
                bridge.push(frame)?
            } else {
                match frame {
                    InFrame::Sse { event, id, data } => {
                        vec![OutFrame::Sse { event, id, data }]
                    }
                    InFrame::AwsEventStream { headers, payload } => {
                        vec![OutFrame::AwsEventStream { headers, payload }]
                    }
                    InFrame::Eof => Vec::new(),
                }
            };
            for frame in frames {
                if let Some(post) = self.post {
                    let (frame, report) = post.encode_frame(frame)?;
                    self.report.merge(report);
                    out.push(frame);
                } else {
                    out.push(frame);
                }
            }
        }
        Ok(out)
    }

    fn finish(&mut self) -> Result<Vec<OutFrame>, Error> {
        let mut out = Vec::new();
        if let Some(bridge) = self.bridge.as_mut() {
            let frames = bridge.finish()?;
            self.report.merge(bridge.report().clone());
            for frame in frames {
                if let Some(post) = self.post {
                    let (frame, report) = post.encode_frame(frame)?;
                    self.report.merge(report);
                    out.push(frame);
                } else {
                    out.push(frame);
                }
            }
        }
        Ok(out)
    }

    fn report(&self) -> &Report {
        &self.report
    }
}

/// 字节级流式转码门面：解码输入 SSE 帧 -> 方向桥 -> 编码目标 SSE 帧。
pub struct StreamTranslator {
    decoder: FrameDecoder,
    encoder: FrameEncoder,
    bridge: Box<dyn StreamBridge>,
}

impl StreamTranslator {
    pub fn new(direction: &'static dyn Direction, plan: &Plan) -> Result<Self, Error> {
        Ok(Self {
            decoder: FrameDecoder::new(),
            encoder: FrameEncoder::new(),
            bridge: direction.stream(plan)?,
        })
    }

    pub fn new_with_context(
        direction: &'static dyn Direction,
        plan: &Plan,
        context: &ResponseContext,
    ) -> Result<Self, Error> {
        Ok(Self {
            decoder: FrameDecoder::new(),
            encoder: FrameEncoder::new(),
            bridge: direction.stream_with_context(plan, context)?,
        })
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        for frame in self.decoder.feed(chunk)? {
            for frame in self.bridge.push(frame)? {
                out.extend(self.encoder.encode(&frame)?);
            }
        }
        Ok(out)
    }

    pub fn finish(&mut self) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        for frame in self.decoder.finish()? {
            for frame in self.bridge.push(frame)? {
                out.extend(self.encoder.encode(&frame)?);
            }
        }
        for frame in self.bridge.finish()? {
            out.extend(self.encoder.encode(&frame)?);
        }
        Ok(out)
    }

    pub fn report(&self) -> &Report {
        self.bridge.report()
    }
}

pub fn stream_translator(
    src: ProtocolId,
    dst: ProtocolId,
    plan: &Plan,
) -> Result<StreamTranslator, Error> {
    let direction = direction(src, dst).ok_or(Error::NoDirection { src, dst })?;
    StreamTranslator::new(direction, plan)
}

pub fn stream_translator_with_context(
    src: ProtocolId,
    dst: ProtocolId,
    plan: &Plan,
    context: &ResponseContext,
) -> Result<StreamTranslator, Error> {
    let direction = direction(src, dst).ok_or(Error::NoDirection { src, dst })?;
    StreamTranslator::new_with_context(direction, plan, context)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_public_pairs_are_registered() {
        for src in ProtocolId::PUBLIC {
            for dst in ProtocolId::PUBLIC {
                if src == dst {
                    continue;
                }
                let direction = direction(src, dst)
                    .unwrap_or_else(|| panic!("missing direction {src} -> {dst}"));
                assert_eq!(direction.src(), src);
                assert_eq!(direction.dst(), dst);
            }
        }
    }

    #[test]
    fn dialect_directions_are_registered() {
        for dialect in dialect::all() {
            for src in ProtocolId::PUBLIC.iter().copied().chain([dialect.id()]) {
                for dst in ProtocolId::PUBLIC.iter().copied().chain([dialect.id()]) {
                    if src == dst {
                        continue;
                    }
                    if src == dialect.id() && dst == dialect.base() {
                        assert!(direction(src, dst).is_some());
                    }
                    if dst == dialect.id() && src == dialect.base() {
                        assert!(direction(src, dst).is_some());
                    }
                }
            }
        }
    }
}

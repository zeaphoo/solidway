//! SSE / eventstream 分帧。
//!
//! 三个公开方向目前都使用 SSE；AWS eventstream 只保留帧类型，编码时返回不支持。

use crate::report::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InFrame {
    Sse {
        event: Option<String>,
        id: Option<String>,
        data: Vec<u8>,
    },
    AwsEventStream {
        headers: Vec<(String, String)>,
        payload: Vec<u8>,
    },
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutFrame {
    Sse {
        event: Option<String>,
        id: Option<String>,
        data: Vec<u8>,
    },
    AwsEventStream {
        headers: Vec<(String, String)>,
        payload: Vec<u8>,
    },
    /// 仅 Chat 目标使用，渲染为 `data: [DONE]`。
    Done,
}

/// SSE 增量解码器：允许半帧留在内部。
#[derive(Debug, Default)]
pub(crate) struct FrameDecoder {
    buffer: Vec<u8>,
}

impl FrameDecoder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn feed(&mut self, chunk: &[u8]) -> Result<Vec<InFrame>, Error> {
        self.buffer.extend_from_slice(chunk);
        let mut frames = Vec::new();

        while let Some((end, sep_len)) = find_frame_end(&self.buffer) {
            let block = self.buffer[..end].to_vec();
            self.buffer.drain(..end + sep_len);
            if let Some(frame) = parse_sse_block(&block)? {
                frames.push(frame);
            }
        }

        Ok(frames)
    }

    /// 输入结束：把剩余内容按最后一帧处理；半帧 / 截断交给上层判断。
    pub(crate) fn finish(&mut self) -> Result<Vec<InFrame>, Error> {
        if self.buffer.is_empty() {
            return Ok(Vec::new());
        }
        let block = std::mem::take(&mut self.buffer);
        Ok(parse_sse_block(&block)?.into_iter().collect())
    }
}

/// SSE 编码器：只支持 SSE 目标。
#[derive(Debug, Default)]
pub(crate) struct FrameEncoder;

impl FrameEncoder {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn encode(&mut self, frame: &OutFrame) -> Result<Vec<u8>, Error> {
        match frame {
            OutFrame::Sse { event, id, data } => {
                let mut out = Vec::new();
                if let Some(event) = event {
                    out.extend_from_slice(b"event: ");
                    out.extend_from_slice(event.as_bytes());
                    out.push(b'\n');
                }
                if let Some(id) = id {
                    out.extend_from_slice(b"id: ");
                    out.extend_from_slice(id.as_bytes());
                    out.push(b'\n');
                }
                let text = std::str::from_utf8(data)
                    .map_err(|_| Error::Stream("frame data is not UTF-8".to_string()))?;
                for line in text.split('\n') {
                    out.extend_from_slice(b"data: ");
                    out.extend_from_slice(line.as_bytes());
                    out.push(b'\n');
                }
                out.push(b'\n');
                Ok(out)
            }
            OutFrame::Done => Ok(b"data: [DONE]\n\n".to_vec()),
            OutFrame::AwsEventStream { .. } => {
                Err(Error::Unsupported("AWS eventstream encoding".to_string()))
            }
        }
    }
}

fn find_frame_end(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer.windows(2).position(|w| w == b"\n\n").map(|i| (i, 2));
    let crlf = buffer
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| (i, 4));
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn parse_sse_block(block: &[u8]) -> Result<Option<InFrame>, Error> {
    let text = std::str::from_utf8(block)
        .map_err(|_| Error::Stream("SSE block is not UTF-8".to_string()))?;
    let mut event = None;
    let mut id = None;
    let mut data_lines = Vec::new();

    for line in text.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() || line.starts_with(':') {
            continue;
        }
        if let Some(value) = line.strip_prefix("event:") {
            event = Some(value.trim_start().to_string());
        } else if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start().to_string());
        } else if let Some(value) = line.strip_prefix("id:") {
            id = Some(value.trim_start().to_string());
        }
    }

    if event.is_none() && data_lines.is_empty() {
        return Ok(None);
    }

    Ok(Some(InFrame::Sse {
        event,
        id,
        data: data_lines.join("\n").into_bytes(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_two_frames_and_keeps_half_frame() {
        let mut decoder = FrameDecoder::new();
        let frames = decoder
            .feed(b"event: a\ndata: {\"x\":1}\n\nevent: b\ndata: {\"y\"")
            .unwrap();
        assert_eq!(
            frames,
            vec![InFrame::Sse {
                event: Some("a".to_string()),
                id: None,
                data: b"{\"x\":1}".to_vec(),
            }]
        );
        let frames = decoder.feed(b":2}\n\n").unwrap();
        assert_eq!(
            frames,
            vec![InFrame::Sse {
                event: Some("b".to_string()),
                id: None,
                data: b"{\"y\":2}".to_vec(),
            }]
        );
    }

    #[test]
    fn handles_crlf_and_multiline_data() {
        let mut decoder = FrameDecoder::new();
        let frames = decoder
            .feed(b"event: a\r\ndata: line1\r\ndata: line2\r\n\r\n")
            .unwrap();
        assert_eq!(
            frames,
            vec![InFrame::Sse {
                event: Some("a".to_string()),
                id: None,
                data: b"line1\nline2".to_vec(),
            }]
        );
    }

    #[test]
    fn encodes_done() {
        let mut encoder = FrameEncoder::new();
        assert_eq!(
            encoder.encode(&OutFrame::Done).unwrap(),
            b"data: [DONE]\n\n"
        );
    }
}

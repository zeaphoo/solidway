//! 上游响应体按 `content-encoding` 旁路解码。
//!
//! **只服务用量观察**：转发给客户端的字节完全不动（压缩体照原样透传），
//! 这里解出来的明文仅喂给 usage 解析器。解码失败时静默降级为「解析不到 usage」，
//! 绝不影响转发结果。
//!
//! 原则：KISS —— 流式与非流式共用同一增量解码器，非流式只是「一次性喂完 + finish」。

use std::io::Write;

use flate2::{Decompress, FlushDecompress, Status};
use zstd::stream::raw::Operation;

/// 非流式整段解码允许的明文字节上限，防解压炸弹。
const MAX_DECODED_BYTES: usize = 32 * 1024 * 1024;
const CHUNK: usize = 8 * 1024;
/// brotli 解码器的内部缓冲大小。
const BROTLI_BUFFER: usize = 4096;
/// gzip 固定头长度（魔数 2 + CM 1 + FLG 1 + MTIME 4 + XFL 1 + OS 1）。
const GZIP_FIXED_HEADER: usize = 10;

/// 支持的 `content-encoding`；未知取值按 `Identity` 处理（解析自然失败，与不解码等价）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContentEncoding {
    #[default]
    Identity,
    Gzip,
    Deflate,
    Brotli,
    Zstd,
}

impl ContentEncoding {
    /// 解析响应头。多值（叠加编码）取最外层（最后一个非 `identity` 取值）。
    pub fn parse(raw: &str) -> Self {
        let token = raw
            .split(',')
            .map(str::trim)
            .rfind(|token| !token.is_empty() && !token.eq_ignore_ascii_case("identity"));
        match token.map(str::to_ascii_lowercase).as_deref() {
            Some("gzip" | "x-gzip") => Self::Gzip,
            Some("deflate") => Self::Deflate,
            Some("br") => Self::Brotli,
            Some("zstd") => Self::Zstd,
            _ => Self::Identity,
        }
    }

    pub fn is_identity(self) -> bool {
        self == Self::Identity
    }
}

/// 增量解码器：`push` 一段压缩字节得到可解析的明文片段。
pub struct BodyDecoder {
    kind: ContentEncoding,
    /// gzip / deflate 的原始 inflate 状态。
    flate: Option<Decompress>,
    /// gzip 头尚未收全时的累积缓冲。
    gzip_header: Vec<u8>,
    gzip_header_done: bool,
    /// `deflate` 首块失败时回退到裸 deflate（部分服务端不按 RFC 发 zlib 头）。
    deflate_fallback: bool,
    brotli: Option<brotli_decompressor::writer::DecompressorWriter<Vec<u8>>>,
    zstd: Option<zstd::stream::raw::Decoder<'static>>,
    /// 解码失败后置位：后续片段一律丢弃，调用方只会看到「解析不到 usage」。
    failed: bool,
}

impl BodyDecoder {
    pub fn new(kind: ContentEncoding) -> Self {
        // gzip 的 deflate 载荷是裸流；头由 `gzip_header` 自行剥离（flate2 的
        // `new_gzip` 需要 C zlib，本仓库只用纯 Rust 后端）。
        let flate = match kind {
            // gzip 头自己剥离，载荷是**裸** deflate；deflate 按 RFC 带 zlib 头，失败再回退。
            ContentEncoding::Gzip => Some(Decompress::new(false)),
            ContentEncoding::Deflate => Some(Decompress::new(true)),
            _ => None,
        };
        let brotli = (kind == ContentEncoding::Brotli).then(|| {
            brotli_decompressor::writer::DecompressorWriter::new(Vec::new(), BROTLI_BUFFER)
        });
        let zstd = (kind == ContentEncoding::Zstd)
            .then(|| zstd::stream::raw::Decoder::new().ok())
            .flatten();
        let mut decoder = Self {
            kind,
            flate,
            gzip_header: Vec::new(),
            gzip_header_done: kind != ContentEncoding::Gzip,
            deflate_fallback: kind == ContentEncoding::Deflate,
            brotli,
            zstd,
            failed: false,
        };
        // 解码器构造失败（只会是 zstd 分配失败）按解码失效处理。
        if !kind.is_identity()
            && decoder.flate.is_none()
            && decoder.brotli.is_none()
            && decoder.zstd.is_none()
        {
            decoder.failed = true;
        }
        decoder
    }

    pub fn failed(&self) -> bool {
        self.failed
    }

    /// 解码一段压缩字节；失败后返回空片段。
    pub fn push(&mut self, chunk: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        if self.failed || chunk.is_empty() {
            return out;
        }
        match self.kind {
            ContentEncoding::Identity => out.extend_from_slice(chunk),
            ContentEncoding::Gzip => {
                let input = match self.take_gzip_payload(chunk) {
                    Some(input) => input,
                    None => return out,
                };
                if input.is_empty() {
                    return out;
                }
                if !self.inflate(&input, &mut out) {
                    self.failed = true;
                    out.clear();
                }
            }
            ContentEncoding::Deflate => {
                if !self.inflate(chunk, &mut out) {
                    self.failed = true;
                    out.clear();
                }
            }
            ContentEncoding::Brotli => {
                let Some(brotli) = self.brotli.as_mut() else {
                    self.failed = true;
                    return out;
                };
                if brotli.write_all(chunk).is_err() {
                    self.failed = true;
                    out.clear();
                    return out;
                }
                out = std::mem::take(brotli.get_mut());
            }
            ContentEncoding::Zstd => {
                let Some(zstd) = self.zstd.as_mut() else {
                    self.failed = true;
                    return out;
                };
                let mut input = chunk;
                let mut buf = [0u8; CHUNK];
                loop {
                    match zstd.run_on_buffers(input, &mut buf) {
                        Ok(status) => {
                            out.extend_from_slice(&buf[..status.bytes_written]);
                            input = &input[status.bytes_read..];
                            if input.is_empty()
                                || (status.bytes_read == 0 && status.bytes_written == 0)
                            {
                                break;
                            }
                        }
                        Err(_) => {
                            self.failed = true;
                            out.clear();
                            break;
                        }
                    }
                }
            }
        }
        out
    }

    /// 响应体结束：吐出解码器内部残留。
    pub fn finish(&mut self) -> Vec<u8> {
        let mut out = Vec::new();
        if self.failed {
            return out;
        }
        if let Some(flate) = self.flate.as_mut() {
            // `Finish` 只对未完成的流有意义；失败无副作用（已解码部分仍可用）。
            let _ = inflate(flate, &[], FlushDecompress::Finish, &mut out);
        }
        out
    }

    /// gzip：剥离头部后返回本次可 inflate 的裸 deflate 载荷。
    fn take_gzip_payload(&mut self, chunk: &[u8]) -> Option<Vec<u8>> {
        if self.gzip_header_done {
            return Some(chunk.to_vec());
        }
        self.gzip_header.extend_from_slice(chunk);
        let header_len = gzip_header_len(&self.gzip_header)?;
        self.gzip_header_done = true;
        let payload = self.gzip_header[header_len..].to_vec();
        self.gzip_header = Vec::new();
        Some(payload)
    }

    fn inflate(&mut self, input: &[u8], out: &mut Vec<u8>) -> bool {
        let Some(flate) = self.flate.as_mut() else {
            return false;
        };
        if inflate(flate, input, FlushDecompress::None, out) {
            self.deflate_fallback = false;
            return true;
        }
        // deflate 首块失败：换裸 deflate 重试一次（zlib 头缺失的实现在野存在）。
        if !self.deflate_fallback {
            return false;
        }
        self.deflate_fallback = false;
        out.clear();
        self.flate = Some(Decompress::new(false));
        let Some(flate) = self.flate.as_mut() else {
            return false;
        };
        inflate(flate, input, FlushDecompress::None, out)
    }
}

/// 计算 gzip 头长度；数据不足时返回 `None`（继续累积），头部非法时标记失败。
fn gzip_header_len(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < GZIP_FIXED_HEADER {
        return None;
    }
    if bytes[0] != 0x1f || bytes[1] != 0x8b || bytes[2] != 0x08 {
        return Some(0);
    }
    let flags = bytes[3];
    let mut pos = GZIP_FIXED_HEADER;
    if flags & 0x04 != 0 {
        // FEXTRA：2 字节 XLEN + 负载
        if bytes.len() < pos + 2 {
            return None;
        }
        let xlen = usize::from(u16::from_le_bytes([bytes[pos], bytes[pos + 1]]));
        pos += 2 + xlen;
    }
    for flag in [0x08u8, 0x10] {
        // FNAME / FCOMMENT：以 NUL 结尾
        if flags & flag != 0 {
            let end = bytes.get(pos..)?.iter().position(|byte| *byte == 0)?;
            pos += end + 1;
        }
    }
    if flags & 0x02 != 0 {
        // FHCRC：2 字节
        pos += 2;
    }
    (bytes.len() >= pos).then_some(pos)
}

/// 喂一段输入并尽量榨干输出；返回 `false` 表示数据损坏。
fn inflate(
    flate: &mut Decompress,
    mut input: &[u8],
    flush: FlushDecompress,
    out: &mut Vec<u8>,
) -> bool {
    let mut buf = [0u8; CHUNK];
    loop {
        let before_in = flate.total_in();
        let before_out = flate.total_out();
        let status = match flate.decompress(input, &mut buf, flush) {
            Ok(status) => status,
            Err(_) => return false,
        };
        let consumed = (flate.total_in() - before_in) as usize;
        let produced = (flate.total_out() - before_out) as usize;
        out.extend_from_slice(&buf[..produced]);
        input = &input[consumed..];
        if status == Status::StreamEnd || (consumed == 0 && produced == 0) {
            return true;
        }
    }
}

/// 非流式整段解码；失败或超出上限时返回 `None`。
pub fn decode_bytes(kind: ContentEncoding, bytes: &[u8]) -> Option<Vec<u8>> {
    if kind.is_identity() {
        return Some(bytes.to_vec());
    }
    let mut decoder = BodyDecoder::new(kind);
    let mut out = decoder.push(bytes);
    out.extend(decoder.finish());
    if decoder.failed() || out.len() > MAX_DECODED_BYTES {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gzip(raw: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(raw).unwrap();
        encoder.finish().unwrap()
    }

    fn deflate(raw: &[u8]) -> Vec<u8> {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(raw).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn parses_common_content_encodings() {
        assert_eq!(ContentEncoding::parse("gzip"), ContentEncoding::Gzip);
        assert_eq!(ContentEncoding::parse(" br "), ContentEncoding::Brotli);
        assert_eq!(ContentEncoding::parse("zstd"), ContentEncoding::Zstd);
        assert_eq!(ContentEncoding::parse("deflate"), ContentEncoding::Deflate);
        // 多值取最外层
        assert_eq!(ContentEncoding::parse("gzip, br"), ContentEncoding::Brotli);
        assert_eq!(
            ContentEncoding::parse("identity"),
            ContentEncoding::Identity
        );
        assert_eq!(ContentEncoding::parse(""), ContentEncoding::Identity);
        // 未知取值不解码
        assert_eq!(
            ContentEncoding::parse("compress"),
            ContentEncoding::Identity
        );
    }

    #[test]
    fn gzip_round_trip_in_chunks() {
        let raw = b"data: {\"usage\":{\"prompt_tokens\":7}}\n\n".repeat(64);
        let encoded = gzip(&raw);
        let mut decoder = BodyDecoder::new(ContentEncoding::Gzip);
        let mut out = Vec::new();
        // 1 字节切片：头部与载荷都跨块，验证 gzip 头累积逻辑。
        for piece in encoded.chunks(1) {
            out.extend(decoder.push(piece));
        }
        out.extend(decoder.finish());
        assert_eq!(out, raw);
        assert!(!decoder.failed());
    }

    #[test]
    fn gzip_second_member_is_ignored_after_stream_end() {
        let raw = b"{\"usage\":{\"prompt_tokens\":3}}";
        let mut encoded = gzip(raw);
        encoded.extend(gzip(raw));
        let decoded = decode_bytes(ContentEncoding::Gzip, &encoded).unwrap();
        // 只解第一个成员，后续成员不参与（HTTP 响应体不会有第二个成员）。
        assert_eq!(decoded, raw);
    }

    #[test]
    fn deflate_round_trip() {
        let raw = br#"{"usage":{"prompt_tokens":3,"completion_tokens":4}}"#;
        let encoded = deflate(raw);
        let decoded = decode_bytes(ContentEncoding::Deflate, &encoded).unwrap();
        assert_eq!(decoded, raw);
    }

    #[test]
    fn corrupt_input_marks_failed() {
        let mut decoder = BodyDecoder::new(ContentEncoding::Gzip);
        let out = decoder.push(b"not gzip at all");
        assert!(out.is_empty());
        assert!(decoder.failed());
        assert!(decoder.push(b"more").is_empty());
    }

    #[test]
    fn identity_passes_through() {
        let raw = br#"{"usage":{"input_tokens":1}}"#;
        assert_eq!(decode_bytes(ContentEncoding::Identity, raw).unwrap(), raw);
    }

    #[test]
    fn brotli_round_trip_in_chunks() {
        // 预压缩常量：`data: {"usage":{"input_tokens":9,"output_tokens":2}}\n\n`。
        const ENCODED: &[u8] = &[
            0x8b, 0x1a, 0x80, 0x64, 0x61, 0x74, 0x61, 0x3a, 0x20, 0x7b, 0x22, 0x75, 0x73, 0x61,
            0x67, 0x65, 0x22, 0x3a, 0x7b, 0x22, 0x69, 0x6e, 0x70, 0x75, 0x74, 0x5f, 0x74, 0x6f,
            0x6b, 0x65, 0x6e, 0x73, 0x22, 0x3a, 0x39, 0x2c, 0x22, 0x6f, 0x75, 0x74, 0x70, 0x75,
            0x74, 0x5f, 0x74, 0x6f, 0x6b, 0x65, 0x6e, 0x73, 0x22, 0x3a, 0x32, 0x7d, 0x7d, 0x0a,
            0x0a, 0x03,
        ];
        let mut decoder = BodyDecoder::new(ContentEncoding::Brotli);
        let mut out = Vec::new();
        for piece in ENCODED.chunks(3) {
            out.extend(decoder.push(piece));
        }
        out.extend(decoder.finish());
        assert!(!decoder.failed());
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "data: {\"usage\":{\"input_tokens\":9,\"output_tokens\":2}}\n\n"
        );
    }

    #[test]
    fn zstd_round_trip() {
        let raw = b"{\"usage\":{\"input_tokens\":9}}";
        let encoded = zstd::encode_all(&raw[..], 3).unwrap();
        let decoded = decode_bytes(ContentEncoding::Zstd, &encoded).unwrap();
        assert_eq!(decoded, raw);
    }
}

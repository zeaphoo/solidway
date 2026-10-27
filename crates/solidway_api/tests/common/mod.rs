//! 集成测试公共设施：mock 上游、裸 HTTP 客户端与网关脚手架。
//!
//! 刻意不引入额外测试依赖（不用 tower / http-body-util）：mock 上游与客户端都用裸
//! TCP 自建，这样对「真实 HTTP 字节」的观察更直接，也不扩大依赖面。
//!
//! provider 归属策略：**一个 provider 一个 mock 实例**（各自随机端口），因此每个实例的
//! 请求计数与请求记录天然只属于该 provider，无需任何推断。

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// 一次到达 mock 上游的请求（原样记录，供断言白名单改写 / 透传）。
#[derive(Debug, Clone)]
pub struct RecordedRequest {
    /// 请求方法。
    pub method: String,
    /// 请求 path（不含 query）。
    pub path: String,
    /// 原始 query（无则为空串）。
    pub query: String,
    /// 小写化后的请求头。
    pub headers: HashMap<String, String>,
    /// 请求体字节。
    pub body: Vec<u8>,
}

impl RecordedRequest {
    /// 取请求头（大小写不敏感）。
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    /// 请求体按 JSON 解析。
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("请求体应为 JSON")
    }

    /// 请求体按 UTF-8 文本返回。
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
}

/// mock 上游要返回的响应。
#[derive(Debug, Clone)]
pub struct MockResponse {
    /// 状态码。
    pub status: u16,
    /// 额外响应头。
    pub headers: Vec<(String, String)>,
    /// 响应体。
    pub body: Vec<u8>,
}

impl MockResponse {
    /// 构造响应。
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    /// 追加响应头。
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// 429：限流（可带 `Retry-After`）。
    pub fn rate_limited(retry_after: Option<&str>) -> Self {
        let response = Self::new(
            429,
            r#"{"error":{"message":"Rate limit reached","type":"requests","code":"rate_limit_exceeded"}}"#,
        )
        .with_header("content-type", "application/json");
        match retry_after {
            Some(value) => response.with_header("retry-after", value),
            None => response,
        }
    }

    /// 429 + `insufficient_quota`（触发永久退役）。
    pub fn insufficient_quota() -> Self {
        Self::new(
            429,
            r#"{"error":{"message":"You exceeded your current quota","type":"insufficient_quota","code":"insufficient_quota"}}"#,
        )
        .with_header("content-type", "application/json")
    }

    /// 401：认证失败。
    pub fn forbidden() -> Self {
        Self::new(
            403,
            r#"{"error":{"message":"Account is not allowed","type":"permission_error","code":"account_forbidden"}}"#,
        )
        .with_header("content-type", "application/json")
    }

    pub fn unauthorized() -> Self {
        Self::new(
            401,
            r#"{"error":{"message":"Incorrect API key","type":"invalid_request_error","code":"invalid_api_key"}}"#,
        )
        .with_header("content-type", "application/json")
    }

    /// 503：上游过载。
    pub fn overloaded() -> Self {
        Self::new(
            503,
            r#"{"error":{"message":"overloaded","code":"server_error"}}"#,
        )
        .with_header("content-type", "application/json")
    }

    /// 400：请求级错误（按分类表不标记 provider）。
    pub fn bad_request() -> Self {
        Self::new(
            400,
            r#"{"error":{"message":"bad request","code":"bad_request"}}"#,
        )
        .with_header("content-type", "application/json")
    }

    /// 成功响应。
    pub fn ok(model: &str) -> Self {
        Self::new(
            200,
            format!(
                r#"{{"id":"chatcmpl-mock","object":"chat.completion","model":"{model}","choices":[{{"index":0,"message":{{"role":"assistant","content":"ok"}}}}]}}"#
            ),
        )
        .with_header("content-type", "application/json")
    }

    /// 流式成功响应：按 `gap` 间隔逐块下发（用于验证网关不缓冲）。
    pub fn sse(events: &[&str], gap: Duration) -> Self {
        let payload = events.join("");
        Self {
            status: 200,
            headers: vec![
                ("content-type".to_string(), "text/event-stream".to_string()),
                ("x-mock-gap-ms".to_string(), gap.as_millis().to_string()),
                (
                    "x-mock-chunk-bytes".to_string(),
                    events
                        .iter()
                        .map(|event| event.len())
                        .max()
                        .unwrap_or(1)
                        .to_string(),
                ),
            ],
            body: payload.into_bytes(),
        }
    }

    /// 立即返回响应头、延迟一段时间后再发 body（用于喂满首字节窗口）。
    pub fn delayed_body(status: u16, body: &str, delay: Duration) -> Self {
        Self {
            status,
            headers: vec![("x-mock-delay-ms".to_string(), delay.as_millis().to_string())],
            body: body.as_bytes().to_vec(),
        }
    }
}

/// mock 上游内部状态。
#[derive(Default)]
struct MockState {
    /// 脚本化响应队列（FIFO）。
    queue: Vec<MockResponse>,
    /// 队列用尽后的兜底响应。
    fallback: Option<MockResponse>,
    /// 收到的请求明细（顺序即到达顺序）。
    requests: Vec<RecordedRequest>,
    /// 每个连接的**原始字节**（用于断言报文级细节：重复头、长度头等）。
    raw: Vec<Vec<u8>>,
}

/// 启动一个「接受连接后立即关闭」的假上游：用于确定性制造传输层失败。
///
/// 它不实现任何 HTTP 语义，只负责让网关的连接立刻断掉。
pub async fn start_dropping_upstream() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind dropping mock");
    let port = listener.local_addr().expect("dropping mock addr").port();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            // 立刻关闭，不读不写：网关侧表现为连接被重置 / 请求发送失败。
            drop(stream);
        }
    });
    port
}

/// mock 上游句柄：一个实例代表一个上游 provider。
#[derive(Clone)]
pub struct MockUpstream {
    state: Arc<Mutex<MockState>>,
    port: u16,
    base_path: String,
}

impl MockUpstream {
    /// 启动 mock（`base_path` 即该 provider base URL 的路径部分，如 `/v1`）。
    pub async fn start(base_path: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind mock");
        let port = listener.local_addr().expect("mock addr").port();
        let state = Arc::new(Mutex::new(MockState::default()));
        let server_state = Arc::clone(&state);
        let base_path_text = base_path.to_string();
        let base_path = Arc::new(base_path_text.clone());
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let state = Arc::clone(&server_state);
                let base_path = Arc::clone(&base_path);
                tokio::spawn(async move {
                    let _ = handle_connection(stream, state, &base_path).await;
                });
            }
        });
        Self {
            state,
            port,
            base_path: base_path_text,
        }
    }

    /// 该 provider 的 base URL（含路径与 `/v1`）。
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}{}", self.port, self.base_path)
    }

    /// 预置响应队列（调用顺序即返回顺序）。
    pub fn script(&self, responses: Vec<MockResponse>) {
        let mut guard = self.state.lock().expect("mock state");
        guard.queue.extend(responses);
    }

    /// 设置单条脚本响应。
    pub fn script_one(&self, response: MockResponse) {
        self.script(vec![response]);
    }

    /// 设置兜底响应（队列用尽后使用）。
    pub fn set_fallback(&self, response: MockResponse) {
        self.state.lock().expect("mock state").fallback = Some(response);
    }

    /// 收到的请求数。
    pub fn count(&self) -> usize {
        self.state.lock().expect("mock state").requests.len()
    }

    /// 收到的最后一条请求。
    pub fn last_request(&self) -> Option<RecordedRequest> {
        self.state
            .lock()
            .expect("mock state")
            .requests
            .last()
            .cloned()
    }

    /// 收到的全部请求。
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.state.lock().expect("mock state").requests.clone()
    }

    /// 最后一个连接的原始请求字节（含请求行与头）。
    pub fn last_raw(&self) -> Option<Vec<u8>> {
        self.state.lock().expect("mock state").raw.last().cloned()
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    state: Arc<Mutex<MockState>>,
    base_path: &str,
) -> std::io::Result<()> {
    // 一个连接处理一个请求（测试客户端始终 connection: close）。
    let mut raw: Vec<u8> = Vec::new();
    let Some((request, teed)) = read_request(&mut stream).await? else {
        return Ok(());
    };
    raw.extend_from_slice(&teed);
    let _ = base_path;
    let response = {
        let mut guard = state.lock().expect("mock state");
        guard.requests.push(request);
        if guard.queue.is_empty() {
            guard
                .fallback
                .clone()
                .unwrap_or_else(|| MockResponse::new(500, "mock upstream: no scripted response"))
        } else {
            guard.queue.remove(0)
        }
    };
    state.lock().expect("mock state").raw.push(raw);
    write_response(&mut stream, &response).await?;
    Ok(())
}

/// 读一条 HTTP/1.1 请求；连接正常关闭时返回 `None`。
/// 第二个返回值是该请求的原始字节（含请求行与头），供报文级断言使用。
async fn read_request(
    stream: &mut TcpStream,
) -> std::io::Result<Option<(RecordedRequest, Vec<u8>)>> {
    let mut buffer: Vec<u8> = Vec::new();
    loop {
        if let Some(header_end) = find_subslice(&buffer, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buffer[..header_end]).to_string();
            let mut lines = head.split("\r\n");
            let request_line = lines.next().unwrap_or_default();
            let mut parts = request_line.split_whitespace();
            let method = parts.next().unwrap_or_default().to_string();
            let target = parts.next().unwrap_or_default().to_string();
            let (path, query) = match target.split_once('?') {
                Some((path, query)) => (path.to_string(), query.to_string()),
                None => (target.clone(), String::new()),
            };

            let mut headers: HashMap<String, String> = HashMap::new();
            let mut chunked = false;
            let mut content_length = 0usize;
            for line in lines {
                let Some((name, value)) = line.split_once(':') else {
                    continue;
                };
                let name = name.trim().to_ascii_lowercase();
                let value = value.trim().to_string();
                if name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked") {
                    chunked = true;
                }
                if name == "content-length" {
                    content_length = value.parse().unwrap_or(0);
                }
                headers.insert(name, value);
            }

            let mut consumed = header_end + 4;
            let body = if chunked {
                // chunked 可能分多次到达：循环读到结束块（`0\r\n\r\n`）为止。
                let mut body: Vec<u8> = Vec::new();
                let mut cursor = consumed;
                let mut pending = 0usize;
                let mut need_size_line = true;
                loop {
                    if need_size_line {
                        let Some(line_end) = find_subslice(&buffer[cursor..], b"\r\n") else {
                            let read = stream.read_buf(&mut buffer).await?;
                            if read == 0 {
                                break;
                            }
                            continue;
                        };
                        let size_line =
                            String::from_utf8_lossy(&buffer[cursor..cursor + line_end]).to_string();
                        let size_text = size_line.split(';').next().unwrap_or_default().trim();
                        let Ok(size) = usize::from_str_radix(size_text, 16) else {
                            break;
                        };
                        cursor += line_end + 2;
                        if size == 0 {
                            break;
                        }
                        pending = size;
                        need_size_line = false;
                    }
                    if buffer.len() < cursor + pending + 2 {
                        let read = stream.read_buf(&mut buffer).await?;
                        if read == 0 {
                            body.extend_from_slice(&buffer[cursor..]);
                            cursor = buffer.len();
                            break;
                        }
                        continue;
                    }
                    body.extend_from_slice(&buffer[cursor..cursor + pending]);
                    cursor += pending + 2;
                    pending = 0;
                    need_size_line = true;
                }
                consumed = cursor;
                body
            } else {
                while buffer.len() < consumed + content_length {
                    let read = stream.read_buf(&mut buffer).await?;
                    if read == 0 {
                        break;
                    }
                }
                let end = (consumed + content_length).min(buffer.len());
                let body = buffer[consumed..end].to_vec();
                consumed = end;
                body
            };
            let raw = buffer[..consumed.min(buffer.len())].to_vec();
            buffer.drain(..consumed.min(buffer.len()));

            return Ok(Some((
                RecordedRequest {
                    method,
                    path,
                    query,
                    headers,
                    body,
                },
                raw,
            )));
        }
        let read = stream.read_buf(&mut buffer).await?;
        if read == 0 {
            return Ok(None);
        }
    }
}

async fn write_response(stream: &mut TcpStream, response: &MockResponse) -> std::io::Result<()> {
    let gap_ms = header_usize(response, "x-mock-gap-ms").unwrap_or(0) as u64;
    let delay_ms = header_usize(response, "x-mock-delay-ms").unwrap_or(0) as u64;
    let chunk_size = header_usize(response, "x-mock-chunk-bytes").unwrap_or(0);

    let mut head = format!(
        "HTTP/1.1 {} {}\r\n",
        response.status,
        reason(response.status)
    );
    head.push_str("transfer-encoding: chunked\r\n");
    for (name, value) in &response.headers {
        if name.starts_with("x-mock-") || name.eq_ignore_ascii_case("content-length") {
            continue;
        }
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    stream.flush().await?;

    if delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }

    let chunks: Vec<Vec<u8>> = if chunk_size == 0 || response.body.len() <= chunk_size {
        vec![response.body.clone()]
    } else {
        response
            .body
            .chunks(chunk_size)
            .map(<[u8]>::to_vec)
            .collect()
    };
    for chunk in chunks {
        if chunk.is_empty() {
            continue;
        }
        stream
            .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
            .await?;
        stream.write_all(&chunk).await?;
        stream.write_all(b"\r\n").await?;
        stream.flush().await?;
        if gap_ms > 0 {
            tokio::time::sleep(Duration::from_millis(gap_ms)).await;
        }
    }
    stream.write_all(b"0\r\n\r\n").await?;
    stream.flush().await?;
    Ok(())
}

fn header_usize(response: &MockResponse, name: &str) -> Option<usize> {
    response
        .headers
        .iter()
        .find(|(key, _)| key == name)
        .and_then(|(_, value)| value.parse::<usize>().ok())
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Status",
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// 客户端收到的响应。
#[derive(Debug, Clone)]
pub struct ClientResponse {
    /// 状态码。
    pub status: u16,
    /// 响应头（小写名）。
    pub headers: HashMap<String, String>,
    /// 完整响应体。
    pub body: Vec<u8>,
}

impl ClientResponse {
    /// 取响应头。
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    /// 响应体按 UTF-8 文本返回。
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }

    /// 响应体按 JSON 解析。
    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("响应体应为 JSON")
    }
}

/// 待发送的客户端请求。
pub struct ClientRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl ClientRequest {
    /// 构造请求。
    pub fn new(method: &str, path: &str) -> Self {
        Self {
            method: method.to_string(),
            path: path.to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// POST + JSON 体。
    pub fn post_json(path: &str, value: &Value) -> Self {
        Self::new("POST", path)
            .header("content-type", "application/json")
            .raw_body(serde_json::to_vec(value).expect("serialize body"))
    }

    /// 携带租户密钥。
    pub fn bearer(mut self, key: &str) -> Self {
        self.headers
            .push(("authorization".to_string(), format!("Bearer {key}")));
        self
    }

    /// 追加请求头。
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// 设置原始请求体。
    pub fn raw_body(mut self, body: Vec<u8>) -> Self {
        self.body = body;
        self
    }
}

/// 裸 HTTP/1.1 客户端（连接到网关）。
#[derive(Clone)]
pub struct RawClient {
    base: String,
}

impl RawClient {
    /// 指向某地址（如 `http://127.0.0.1:1234`）。
    pub fn new(base: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_string(),
        }
    }

    /// 发送请求并读取完整响应（连接失败即 panic；就绪探测请用 `try_send`）。
    pub async fn send(&self, request: ClientRequest) -> ClientResponse {
        let mut stream = self.connect().await.expect("connect gateway");
        let head = self.encode(&request);
        stream.write_all(head.as_bytes()).await.expect("write head");
        if !request.body.is_empty() {
            stream.write_all(&request.body).await.expect("write body");
        }
        stream.flush().await.expect("flush");
        read_response(&mut stream).await
    }

    /// 发送请求并读取完整响应；连接失败返回错误而不是 panic。
    pub async fn try_send(&self, request: ClientRequest) -> std::io::Result<ClientResponse> {
        let mut stream = self.connect().await?;
        let head = self.encode(&request);
        stream.write_all(head.as_bytes()).await?;
        if !request.body.is_empty() {
            stream.write_all(&request.body).await?;
        }
        stream.flush().await?;
        Ok(read_response(&mut stream).await)
    }

    /// 逐块读取响应体，每收到一块调用 `on_chunk`（用于验证「不缓冲」）。
    pub async fn stream_chunks<F>(&self, request: ClientRequest, mut on_chunk: F) -> ClientResponse
    where
        F: FnMut(&[u8]),
    {
        let mut stream = self.connect().await.expect("connect gateway");
        let head = self.encode(&request);
        stream.write_all(head.as_bytes()).await.expect("write head");
        if !request.body.is_empty() {
            stream.write_all(&request.body).await.expect("write body");
        }
        stream.flush().await.expect("flush");

        let (status, headers, mut rest) = read_head(&mut stream).await;
        let mut collected: Vec<u8> = Vec::new();
        loop {
            match decode_one_chunk(&rest) {
                Some((chunk, used)) => {
                    if used == 0 {
                        break;
                    }
                    on_chunk(&chunk);
                    collected.extend_from_slice(&chunk);
                    rest.drain(..used);
                }
                None => {
                    let read = stream.read_buf(&mut rest).await.unwrap_or(0);
                    if read == 0 {
                        break;
                    }
                }
            }
        }
        ClientResponse {
            status,
            headers,
            body: collected,
        }
    }

    async fn connect(&self) -> std::io::Result<TcpStream> {
        let addr = self.base.trim_start_matches("http://").to_string();
        let stream = TcpStream::connect(&addr).await?;
        stream.set_nodelay(true)?;
        Ok(stream)
    }

    fn encode(&self, request: &ClientRequest) -> String {
        let mut head = format!("{} {} HTTP/1.1\r\n", request.method, request.path);
        head.push_str("host: gateway.local\r\n");
        head.push_str("connection: close\r\n");
        if !request.body.is_empty() {
            head.push_str(&format!("content-length: {}\r\n", request.body.len()));
        }
        for (name, value) in &request.headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("\r\n");
        head
    }
}

/// 读响应头（含 chunked 判定），返回 (status, headers, 已读的 body 前缀)。
async fn read_head(stream: &mut TcpStream) -> (u16, HashMap<String, String>, Vec<u8>) {
    let mut buffer: Vec<u8> = Vec::new();
    loop {
        let read = stream.read_buf(&mut buffer).await.expect("read head");
        assert!(read > 0, "连接在响应头之前关闭");
        if let Some(end) = find_subslice(&buffer, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buffer[..end]).to_string();
            let (status, headers) = parse_head(&head);
            return (status, headers, buffer[end + 4..].to_vec());
        }
    }
}

async fn read_response(stream: &mut TcpStream) -> ClientResponse {
    let (status, headers, mut rest) = read_head(stream).await;
    let chunked = headers
        .get("transfer-encoding")
        .map(|value| value.contains("chunked"))
        .unwrap_or(false);

    if !chunked {
        let expected = headers
            .get("content-length")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let mut body = rest;
        while body.len() < expected {
            let read = stream.read_buf(&mut body).await.unwrap_or(0);
            if read == 0 {
                break;
            }
        }
        return ClientResponse {
            status,
            headers,
            body,
        };
    }

    let mut body: Vec<u8> = Vec::new();
    loop {
        match decode_one_chunk(&rest) {
            Some((chunk, used)) => {
                if used == 0 {
                    break;
                }
                body.extend_from_slice(&chunk);
                rest.drain(..used);
            }
            None => {
                let read = stream.read_buf(&mut rest).await.unwrap_or(0);
                if read == 0 {
                    break;
                }
            }
        }
    }
    ClientResponse {
        status,
        headers,
        body,
    }
}

/// 解析响应头。
fn parse_head(head: &str) -> (u16, HashMap<String, String>) {
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or(0);
    let mut headers = HashMap::new();
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }
    (status, headers)
}

/// 从缓冲中解析一个 chunk；数据不足返回 `None`，结束块返回 `(空, 0)`。
fn decode_one_chunk(data: &[u8]) -> Option<(Vec<u8>, usize)> {
    let line_end = find_subslice(data, b"\r\n")?;
    let size_line = String::from_utf8_lossy(&data[..line_end]).to_string();
    let size = usize::from_str_radix(size_line.split(';').next()?.trim(), 16).ok()?;
    if size == 0 {
        return Some((Vec::new(), 0));
    }
    let start = line_end + 2;
    if data.len() < start + size + 2 {
        return None;
    }
    Some((data[start..start + size].to_vec(), start + size + 2))
}

// ---------------------------------------------------------------------------
// 网关脚手架（本项目：solidway_api + solidway_core 同进程装配；配置来自 DB）
// ---------------------------------------------------------------------------

// 各集成测试只用到本模块的子集：未使用项由文件顶部的 allow(dead_code) 与下面的 allow 覆盖。
#[allow(unused_imports)]
use std::path::{Path, PathBuf};

use solidway_api::db::{api_keys, provider_keys, routes};
use solidway_api::state::now_unix;
use solidway_api::{Api, ApiConfig};
use solidway_core::{Core, hash_api_key};

/// 每个 provider 一份的 mock 上游集合。
pub struct Mocks {
    /// provider A 的上游。
    pub a: MockUpstream,
    /// provider B 的上游。
    pub b: MockUpstream,
    /// provider C 的上游（备用）。
    pub c: MockUpstream,
}

/// 启动三个 mock 上游（一 provider 一个实例）。
pub async fn start_mocks() -> Mocks {
    start_mocks_at("/v1").await
}

/// 以指定基路径启动三个 mock 上游。
pub async fn start_mocks_at(base_path: &str) -> Mocks {
    Mocks {
        a: MockUpstream::start(base_path).await,
        b: MockUpstream::start(base_path).await,
        c: MockUpstream::start(base_path).await,
    }
}

/// 测试统一使用的客户端 API Key 明文。
pub const TENANT_KEY: &str = "gw-tenant";

/// 测试用 admin 口令（`build_harness` 会把它写进 admin 账号）。
pub const ADMIN_PASSWORD: &str = "admin-test-pass";

/// 从 `extra`（键=值，形如 `affinity_ttl = "5s"`）取某键的值。
fn extra_value<'a>(extra: &'a str, key: &str) -> Option<&'a str> {
    extra.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim())
    })
}

/// 解析 `1ms` / `5s` / `30m` / `1h` / 裸秒数。
fn parse_duration(raw: &str) -> Option<Duration> {
    let raw = raw.trim().trim_matches('"');
    if let Some(v) = raw.strip_suffix("ms") {
        return v.parse::<u64>().ok().map(Duration::from_millis);
    }
    for (suffix, factor) in [("s", 1u64), ("m", 60), ("h", 3600), ("d", 86_400)] {
        if let Some(v) = raw.strip_suffix(suffix) {
            return v
                .parse::<u64>()
                .ok()
                .map(|n| Duration::from_secs(n * factor));
        }
    }
    raw.parse::<u64>().ok().map(Duration::from_secs)
}

/// 生成临时 data_root。
pub fn temp_data_root(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("solidway-it-{tag}-{}-{nanos}", std::process::id()))
}

/// 把 `extra` 里的网络超时写入 DB 的 `network` 设置节（替代已废弃的 config.toml）。
async fn apply_network_settings(api: &Api, extra: &str) {
    let connect = extra_value(extra, "connect_timeout")
        .and_then(parse_duration)
        .unwrap_or(Duration::from_secs(10))
        .as_secs();
    let first_byte = extra_value(extra, "first_byte_timeout")
        .and_then(parse_duration)
        .unwrap_or(Duration::from_secs(60))
        .as_secs();
    if connect == 10 && first_byte == 60 {
        return;
    }
    let mut network = solidway_api::settings::NetworkSettings::default();
    network.connect_timeout_secs = connect;
    network.first_byte_timeout_secs = first_byte;
    solidway_api::settings::save_network(&api.state().pool, &network, "test", 0)
        .await
        .expect("save network settings");
    api.state()
        .reload_settings()
        .await
        .expect("reload settings");
}

/// 把 `extra` 里的冷却 / 粘性 TTL 写入 DB 的 `gateway` 设置节。
async fn apply_gateway_settings(api: &Api, extra: &str) {
    let mut gateway = solidway_api::settings::GatewaySettings::default();
    if let Some(secs) = extra_value(extra, "cooldown_auth").and_then(parse_duration) {
        gateway.cooldown_auth_secs = secs.as_secs();
    }
    if let Some(secs) = extra_value(extra, "cooldown_rate_limit").and_then(parse_duration) {
        gateway.cooldown_rate_limit_secs = secs.as_secs();
    }
    if let Some(secs) = extra_value(extra, "cooldown_unavailable").and_then(parse_duration) {
        gateway.cooldown_unavailable_secs = secs.as_secs();
    }
    if let Some(secs) = extra_value(extra, "affinity_ttl").and_then(parse_duration) {
        gateway.affinity_ttl_secs = secs.as_secs();
    }
    // `updated_by` 是指向 users 的外键：用真实 admin id。
    let admin_id: String = sqlx::query_scalar("SELECT id FROM users WHERE role = 'admin' LIMIT 1")
        .fetch_one(&api.state().pool)
        .await
        .expect("admin id");
    solidway_api::settings::save_gateway(&api.state().pool, &gateway, &admin_id, now_unix())
        .await
        .expect("save gateway settings");
}

/// provider 种子。
pub struct SeedProvider {
    pub id: String,
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<String>,
}

/// 路由种子（含其 provider 与 API Key）。
pub struct SeedRoute {
    pub id: String,
    pub name: String,
    pub providers: Vec<SeedProvider>,
    pub keys: Vec<(String, String)>,
}

impl SeedRoute {
    pub fn new(id: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            providers: Vec::new(),
            keys: Vec::new(),
        }
    }

    /// 追加 provider（api_key 取 `sk-provider-<后缀>`，如 p-a → sk-provider-a）。
    pub fn provider(mut self, id: &str, base_url: String, models: &[&str]) -> Self {
        let suffix = id.strip_prefix("p-").unwrap_or(id);
        self.providers.push(SeedProvider {
            id: id.to_string(),
            base_url,
            api_key: format!("sk-provider-{suffix}"),
            models: models.iter().map(|m| m.to_string()).collect(),
        });
        self
    }

    /// 追加 provider（显式 api_key）。
    pub fn provider_with_key(
        mut self,
        id: &str,
        base_url: String,
        api_key: &str,
        models: &[&str],
    ) -> Self {
        self.providers.push(SeedProvider {
            id: id.to_string(),
            base_url,
            api_key: api_key.to_string(),
            models: models.iter().map(|m| m.to_string()).collect(),
        });
        self
    }

    /// 追加一把客户端 API Key。
    pub fn key(mut self, id: &str, plaintext: &str) -> Self {
        self.keys.push((id.to_string(), plaintext.to_string()));
        self
    }
}

async fn insert_provider(state: &solidway_api::AppState, route_id: &str, seed: &SeedProvider) {
    let now = now_unix();
    let aad = format!("provider_key|{}", seed.id);
    provider_keys::insert(
        &state.pool,
        &provider_keys::ProviderKeyRow {
            id: seed.id.clone(),
            route_id: route_id.to_string(),
            provider: "openai".to_string(),
            name: Some(seed.id.clone()),
            base_url: Some(seed.base_url.clone()),
            api_formats: r#"["chat_completions","responses"]"#.to_string(),
            auth_scheme: "bearer".to_string(),
            credential_kind: "api_key".to_string(),
            credential_ciphertext: state.cipher.encrypt(&seed.api_key, &aad).expect("encrypt"),
            credential_masked: solidway_api::crypto::Cipher::mask(&seed.api_key),
            credential_hash: state
                .cipher
                .fingerprint(&seed.api_key)
                .expect("fingerprint"),
            options: None,
            models: serde_json::to_string(
                &seed
                    .models
                    .iter()
                    .map(|name| solidway_core::ModelMapping {
                        name: name.clone(),
                        upstream: name.clone(),
                    })
                    .collect::<Vec<_>>(),
            )
            .expect("models json"),
            priority: 1,
            status: "active".to_string(),
            created_at: now,
            updated_at: now,
        },
    )
    .await
    .expect("insert provider");
}

async fn insert_api_key(state: &solidway_api::AppState, id: &str, route_id: &str, plaintext: &str) {
    let now = now_unix();
    api_keys::insert(
        &state.pool,
        &api_keys::ApiKeyRow {
            id: id.to_string(),
            route_id: route_id.to_string(),
            name: None,
            key_prefix: plaintext.chars().take(8).collect(),
            key_masked: "****test".to_string(),
            key_hash: hash_api_key(plaintext),
            status: "active".to_string(),
            expires_at: None,
            last_used_at: None,
            created_at: now,
            updated_at: now,
        },
    )
    .await
    .expect("insert api key");
}

async fn seed(state: &solidway_api::AppState, routes: Vec<SeedRoute>) {
    let now = now_unix();
    let owner_id = sqlx::query_scalar::<_, String>(
        "SELECT id FROM users WHERE role = 'admin' ORDER BY created_at ASC, id ASC LIMIT 1",
    )
    .fetch_optional(&state.pool)
    .await
    .expect("find admin");
    for route in routes {
        routes::create(
            &state.pool,
            &routes::Route {
                id: route.id.clone(),
                owner_id: owner_id.clone(),
                name: route.name.clone(),
                description: None,
                enabled: 1,
                created_at: now,
                updated_at: now,
                selection_strategy: "round_robin".to_string(),
                selection_config: "{}".to_string(),
                archived_at: None,
                archived_by: None,
            },
        )
        .await
        .expect("create route");
        for provider in &route.providers {
            insert_provider(state, &route.id, provider).await;
        }
        for (id, plaintext) in &route.keys {
            insert_api_key(state, id, &route.id, plaintext).await;
        }
    }
}

/// 装配：`Api`（含 DB 配置）→ `Core` → 合并路由并监听随机端口。
async fn assemble(api: &Api) -> (String, Arc<Core>) {
    let core = api.build_core().await.expect("build core");
    let app = api.router().merge(core.router());
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind gateway");
    let addr = listener.local_addr().expect("gateway addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{addr}"), core)
}

/// 通用脚手架：给定 mock 上游基路径与路由种子。
pub async fn build_harness<F>(tag: &str, extra: &str, base_path: &str, routes: F) -> Harness
where
    F: FnOnce(&Mocks) -> Vec<SeedRoute>,
{
    build_harness_with_mode(
        tag,
        extra,
        base_path,
        solidway_api::RunMode::Standalone,
        routes,
    )
    .await
}

/// 与 [`build_harness`] 相同，但允许指定运行模式（如桌面模式的重启用例）。
pub async fn build_harness_with_mode<F>(
    tag: &str,
    extra: &str,
    base_path: &str,
    mode: solidway_api::RunMode,
    routes: F,
) -> Harness
where
    F: FnOnce(&Mocks) -> Vec<SeedRoute>,
{
    let mocks = start_mocks_at(base_path).await;
    let data_root = temp_data_root(tag);
    let _ = std::fs::remove_dir_all(&data_root);
    let api = Api::init(ApiConfig {
        data_root: data_root.clone(),
        master_key: None,
        mode,
        web_assets: None,
    })
    .await
    .expect("init api");
    apply_network_settings(&api, extra).await;

    // 把 admin 口令改成已知值，便于管理面 HTTP 级测试。
    let hash = solidway_api::auth::password::hash_password(ADMIN_PASSWORD).expect("hash admin");
    sqlx::query("UPDATE users SET password_hash = ?1 WHERE role = 'admin'")
        .bind(hash)
        .execute(&api.state().pool)
        .await
        .expect("set admin password");

    seed(api.state(), routes(&mocks)).await;
    apply_gateway_settings(&api, extra).await;
    // 测试固定使用 UTC+08:00，避免测试断言依赖 runner 的系统时区。
    let admin_id: String = sqlx::query_scalar("SELECT id FROM users WHERE role = 'admin' LIMIT 1")
        .fetch_one(&api.state().pool)
        .await
        .expect("admin id");
    let usage = solidway_api::settings::UsageSettings {
        timezone_offset_minutes: 480,
        ..Default::default()
    };
    solidway_api::settings::save_usage(&api.state().pool, &usage, &admin_id, now_unix())
        .await
        .expect("seed usage settings");

    let (base_url, core) = assemble(&api).await;
    let db_path = data_root.join("solidway.db");
    Harness {
        mocks,
        client: RawClient::new(&base_url),
        base_url,
        data_root,
        api,
        gateway: core,
        db_path,
    }
}

/// 网关脚手架：mock 上游 + 进程内真实监听的装配服务 + 裸客户端。
pub struct Harness {
    /// mock 上游集合。
    pub mocks: Mocks,
    /// 指向网关的裸客户端。
    pub client: RawClient,
    /// 网关基地址。
    pub base_url: String,
    /// data_root（重启场景复用）。
    pub data_root: PathBuf,
    /// 控制面句柄。
    pub api: Api,
    /// 数据面句柄（观测探针等内部计数）。
    pub gateway: Arc<Core>,
    /// 数据库文件路径（重启 / 断言复用）。
    pub db_path: PathBuf,
}

impl Harness {
    /// 单路由 + 两个 provider（A / B 都提供 `gpt-5` 与 `gpt-5-mini`）。
    ///
    /// 两个模型名的候选集合都是 [p-a, p-b]，便于验证「按模型独立轮询」与换号。
    pub async fn two_providers(tag: &str, extra: &str) -> Self {
        build_harness(tag, extra, "/v1", |mocks| {
            vec![
                SeedRoute::new("r-main", "main")
                    .provider("p-a", mocks.a.base_url(), &["gpt-5", "gpt-5-mini"])
                    .provider("p-b", mocks.b.base_url(), &["gpt-5", "gpt-5-mini"])
                    .key("k-main", TENANT_KEY),
            ]
        })
        .await
    }

    /// 用同一份 data_root 重启（模拟进程重启），返回新客户端与新的 core 句柄。
    pub async fn restart(&self, _extra: &str) -> (RawClient, Arc<Core>) {
        let (base_url, core) = assemble(&self.api).await;
        (RawClient::new(&base_url), core)
    }

    /// 数据库文件路径。
    pub fn db_path(&self) -> PathBuf {
        self.data_root.join("solidway.db")
    }

    /// 以 admin 登录并返回可直接放进 `cookie` 头的会话串。
    pub async fn admin_cookie(&self) -> String {
        let response = self
            .client
            .send(ClientRequest::post_json(
                "/api/auth/login",
                &serde_json::json!({"username": "admin", "password": ADMIN_PASSWORD}),
            ))
            .await;
        assert_eq!(response.status, 200, "admin 登录应成功");
        response
            .header("set-cookie")
            .expect("应有 set-cookie")
            .split(';')
            .next()
            .expect("cookie 段")
            .to_string()
    }
}

/// 等待条件成立（带超时）。
pub async fn wait_for<F>(timeout: Duration, mut condition: F) -> bool
where
    F: FnMut() -> bool,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if condition() {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// 在超时时间内反复读健康表，直到 `predicate` 成立；返回最终一次读到的结果。
pub async fn wait_for_rows<F>(
    db_path: &std::path::Path,
    timeout: Duration,
    mut predicate: F,
) -> Vec<(String, String, Option<i64>)>
where
    F: FnMut(&[(String, String, Option<i64>)]) -> bool,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let rows = health_rows(db_path).await;
        if predicate(&rows) || tokio::time::Instant::now() >= deadline {
            return rows;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// 直接读库里的健康标记（人工恢复 / 落库断言用）。
pub async fn health_rows(db_path: &std::path::Path) -> Vec<(String, String, Option<i64>)> {
    use sqlx::Row;
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(db_path)
                .create_if_missing(false),
        )
        .await
        .expect("打开测试库（断言用）");
    let rows = sqlx::query(
        "SELECT provider_id, state, cooldown_until FROM provider_health ORDER BY provider_id",
    )
    .fetch_all(&pool)
    .await
    .expect("查询健康表");
    rows.into_iter()
        .map(|row| {
            (
                row.get::<String, _>("provider_id"),
                row.get::<String, _>("state"),
                row.get::<Option<i64>, _>("cooldown_until"),
            )
        })
        .collect()
}

/// 直接执行 SQL（人工恢复 / 清库场景）。
pub async fn exec_sql(db_path: &std::path::Path, sql: String) {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(db_path)
                .create_if_missing(false),
        )
        .await
        .expect("打开测试库");
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .execute(&pool)
        .await
        .expect("执行 SQL");
}

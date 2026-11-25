use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use fomeg_windows_app_lib::core::ai::{self, AiConfig, AiConfigUpdate, Chats};
use serde_json::{json, Value};

const ANTHROPIC_REPLY: &str = concat!(
    "event: message_start\ndata: {\"message\":{\"usage\":{\"input_tokens\":3}}}\n\n",
    "event: content_block_start\ndata: {\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
    "event: content_block_delta\ndata: {\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"private\"}}\n\n",
    "event: content_block_delta\ndata: {\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"signed\"}}\n\n",
    "event: content_block_stop\ndata: {\"index\":0}\n\n",
    "event: content_block_start\ndata: {\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
    "event: content_block_delta\ndata: {\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Claude reply\"}}\n\n",
    "event: content_block_stop\ndata: {\"index\":1}\n\n",
    "event: message_delta\ndata: {\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":4}}\n\n",
    "event: message_stop\ndata: {}\n\n",
);

const OPENAI_REPLY: &str = concat!(
    ": keep-alive\r\n\r\n",
    "data:{\"choices\":[{\"delta\":{\"reasoning_content\":\"private\"}}]}\r\n\r\n",
    "data: {\"choices\":[{\"delta\":{\"reasoning\":\"more\",\"content\":\"Custom reply\"}}]}\n\n",
    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
    "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":6}}\n\n",
    "data:[DONE]\n\n",
);

#[test]
fn anthropic_custom_and_anthropic_again_share_text_and_preserve_raw_blocks() {
    let (base, server) = mock_server(&[
        ("200 OK", "text/event-stream", ANTHROPIC_REPLY),
        ("200 OK", "text/event-stream", OPENAI_REPLY),
        ("200 OK", "text/event-stream", ANTHROPIC_REPLY),
    ]);
    let config = TempConfig::new();
    let chats = Chats::default();
    ai::set_config_at(
        &config.0,
        AiConfigUpdate {
            include_context: Some(true),
            base_url: Some(format!("{base}/")),
            api_key: Some("anthropic-test-key".into()),
            ..AiConfigUpdate::default()
        },
    )
    .unwrap();
    assert_eq!(
        send(&config.0, &chats, "first", Some("snapshot")),
        json!([
            {"chat_id":"chat", "kind":"thinking"},
            {"chat_id":"chat", "kind":"delta", "text":"Claude reply"},
            {"chat_id":"chat", "kind":"done", "stop_reason":"end_turn", "usage":{"input_tokens":3,"output_tokens":4}}
        ])
    );

    ai::set_config_at(
        &config.0,
        AiConfigUpdate {
            provider: Some("custom".into()),
            model: Some("local-model".into()),
            base_url: Some(base),
            ..AiConfigUpdate::default()
        },
    )
    .unwrap();
    assert_eq!(
        send(&config.0, &chats, "second", Some("new snapshot")),
        json!([
            {"chat_id":"chat", "kind":"thinking"},
            {"chat_id":"chat", "kind":"delta", "text":"Custom reply"},
            {"chat_id":"chat", "kind":"done", "stop_reason":"stop", "usage":{"input_tokens":5,"output_tokens":6}}
        ])
    );

    ai::set_config_at(
        &config.0,
        AiConfigUpdate {
            provider: Some("anthropic".into()),
            ..AiConfigUpdate::default()
        },
    )
    .unwrap();
    assert_eq!(send(&config.0, &chats, "third", None)[2]["kind"], "done");

    let requests = server.join().unwrap();
    let snapshot = "<system_snapshot>\nsnapshot\n</system_snapshot>";
    assert_eq!(requests[0].line, "POST /v1/messages HTTP/1.1");
    assert_eq!(requests[0].headers["x-api-key"], "anthropic-test-key");
    assert_eq!(requests[0].headers["anthropic-version"], "2026-10-21");
    assert_eq!(
        requests[0].headers["anthropic-beta"],
        "server-side-fallback-2026-07-01"
    );
    assert_eq!(
        requests[0].body["messages"][0]["content"],
        json!([
            {"type": "text", "text": snapshot}, {"type": "text", "text": "first"}
        ])
    );

    assert_eq!(requests[1].line, "POST /v1/chat/completions HTTP/1.1");
    assert!(!requests[1].headers.contains_key("authorization"));
    assert_eq!(requests[1].body["model"], "local-model");
    assert_eq!(requests[1].body["messages"][0]["role"], "system");
    assert_eq!(requests[1].body["messages"].as_array().unwrap()[1..], json!([
        {"role": "user", "content": format!("{snapshot}\n\nfirst")},
        {"role": "assistant", "content": "Claude reply"},
        {"role": "user", "content": "<system_snapshot>\nnew snapshot\n</system_snapshot>\n\nsecond"},
    ]).as_array().unwrap()[..]);
    assert!(requests[1].body.get("stream_options").is_none());

    let anth_history = requests[2].body["messages"].as_array().unwrap();
    assert_eq!(anth_history.len(), 5);
    assert_eq!(
        anth_history[1]["content"],
        json!([
            {"type":"thinking", "thinking":"private", "signature":"signed"},
            {"type":"text", "text":"Claude reply"}
        ])
    );
    assert_eq!(
        anth_history[3],
        json!({"role":"assistant", "content":"Custom reply"})
    );
}

const ANTHROPIC_TOOL_USE: &str = concat!(
    "event: message_start\ndata: {\"message\":{\"usage\":{\"input_tokens\":10}}}\n\n",
    "event: content_block_start\ndata: {\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
    "event: content_block_delta\ndata: {\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Checking.\"}}\n\n",
    "event: content_block_stop\ndata: {\"index\":0}\n\n",
    "event: content_block_start\ndata: {\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"tu_1\",\"name\":\"process_tree\",\"input\":{}}}\n\n",
    "event: content_block_delta\ndata: {\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"pid\\\":\"}}\n\n",
    "event: content_block_delta\ndata: {\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"42}\"}}\n\n",
    "event: content_block_stop\ndata: {\"index\":1}\n\n",
    "event: message_delta\ndata: {\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":7}}\n\n",
    "event: message_stop\ndata: {}\n\n",
);

const OPENAI_TOOL_CALL: &str = concat!(
    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"type\":\"function\",\"function\":{\"name\":\"services\",\"arguments\":\"\"}}]}}]}\n\n",
    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"name\\\":\\\"x\\\"}\"}}]}}]}\n\n",
    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
    "data:[DONE]\n\n",
);

#[test]
fn tool_loop_runs_read_only_tools_and_resends_results_on_both_wires() {
    let (base, server) = mock_server(&[
        ("200 OK", "text/event-stream", ANTHROPIC_TOOL_USE),
        ("200 OK", "text/event-stream", ANTHROPIC_REPLY),
        ("200 OK", "text/event-stream", OPENAI_TOOL_CALL),
        ("200 OK", "text/event-stream", OPENAI_REPLY),
    ]);
    let config = TempConfig::new();
    let chats = Chats::default();
    ai::set_config_at(
        &config.0,
        AiConfigUpdate {
            base_url: Some(base.clone()),
            api_key: Some("anthropic-test-key".into()),
            ..AiConfigUpdate::default()
        },
    )
    .unwrap();
    let events = send_with_tools(&config.0, &chats, "how much RAM");
    let kinds: Vec<&str> = events.as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["delta", "tool", "tool", "delta", "thinking", "delta", "done"]);
    assert_eq!(events[1]["phase"], "start");
    assert_eq!(events[1]["name"], "process_tree");
    assert_eq!(events[1]["args"], json!({"pid": 42}));
    assert_eq!(events[2]["phase"], "done");
    assert_eq!(events[2]["ok"], true);
    assert_eq!(events[3]["text"], "\n\n");
    assert_eq!(events[6]["usage"], json!({"input_tokens": 13, "output_tokens": 11}));

    ai::set_config_at(
        &config.0,
        AiConfigUpdate {
            provider: Some("custom".into()),
            target: Some("custom".into()),
            model: Some("local-model".into()),
            base_url: Some(base),
            ..AiConfigUpdate::default()
        },
    )
    .unwrap();
    let events = send_with_tools(&config.0, &chats, "services?");
    let kinds: Vec<&str> = events.as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["tool", "tool", "thinking", "delta", "done"]);
    assert_eq!(events[0]["args"], json!({"name": "x"}));

    let requests = server.join().unwrap();
    // Claude: mô tả tool + lời dặn tool trong system, vòng 2 gửi lại tool_use và tool_result
    assert_eq!(requests[0].body["tools"][0]["name"], "process_tree");
    assert!(requests[0].body["tools"][0]["input_schema"].is_object());
    assert!(requests[0].body["system"][0]["text"].as_str().unwrap().contains("CHỈ ĐỌC"));
    let m = requests[1].body["messages"].as_array().unwrap();
    assert_eq!(m.len(), 3);
    assert_eq!(m[1]["content"][1]["type"], "tool_use");
    assert_eq!(m[1]["content"][1]["input"], json!({"pid": 42}));
    assert_eq!(m[2]["content"][0], json!({"type":"tool_result","tool_use_id":"tu_1","content":"RESULT process_tree","is_error":false}));
    // OpenAI-compatible: lượt Claude trước chỉ còn câu hỏi + câu trả lời; vòng 2 có tool_calls + role tool
    assert_eq!(requests[2].body["tools"][0]["type"], "function");
    let m = requests[2].body["messages"].as_array().unwrap();
    assert_eq!(m.len(), 4);
    assert_eq!(m[2], json!({"role":"assistant","content":"Checking.\n\nClaude reply"}));
    let m = requests[3].body["messages"].as_array().unwrap();
    assert_eq!(m[4]["tool_calls"][0]["function"]["name"], "services");
    assert_eq!(m[5], json!({"role":"tool","tool_call_id":"call_1","content":"RESULT services"}));
}

fn send_with_tools(config: &Path, chats: &Chats, text: &str) -> Value {
    let events = RefCell::new(Vec::new());
    let prepared = AiConfig::load(config).unwrap().prepare_send().unwrap();
    let mut run = |name: &str, _: &Value| Ok(format!("RESULT {name}"));
    let defs = vec![
        json!({"name": "process_tree", "description": "tree", "inputSchema": {"type": "object", "properties": {"pid": {"type": "integer"}}}}),
        json!({"name": "services", "description": "services", "inputSchema": {"type": "object", "properties": {}}}),
    ];
    let kit = ai::ToolKit { defs, run: &mut run, label: |_| "dữ liệu" };
    chats.send(prepared, "chat", text, None, &[], Some(kit), &AtomicBool::new(false), &|event| {
        events.borrow_mut().push(serde_json::to_value(event).unwrap());
    });
    Value::Array(events.into_inner())
}

#[test]
fn listing_uses_saved_provider_credentials_and_maps_http_errors() {
    let (base, server) = mock_server(&[
        (
            "200 OK",
            "application/json",
            r#"{"models":["z",{"id":"a"},{"name":"z"},{"model":"b"}]}"#,
        ),
        (
            "401 Unauthorized",
            "application/json",
            r#"{"error":{"message":"Rejected custom-test-key"}}"#,
        ),
        (
            "200 OK",
            "application/json",
            r#"{"data":[{"id":"claude-z"},{"id":"claude-a"}]}"#,
        ),
    ]);
    let config = TempConfig::new();
    for (provider, key) in [
        ("anthropic", "anthropic-test-key"),
        ("custom", "custom-test-key"),
    ] {
        ai::set_config_at(
            &config.0,
            AiConfigUpdate {
                target: Some(provider.into()),
                base_url: Some(base.clone()),
                api_key: Some(key.into()),
                ..AiConfigUpdate::default()
            },
        )
        .unwrap();
    }

    let config = AiConfig::load(&config.0).unwrap();
    assert_eq!(config.list_models("custom").unwrap(), ["a", "b", "z"]);
    assert_eq!(
        config.list_models("custom").unwrap_err(),
        "API key không hợp lệ. Rejected [đã ẩn]"
    );
    assert_eq!(
        config.list_models("anthropic").unwrap(),
        ["claude-a", "claude-z"]
    );

    let requests = server.join().unwrap();
    for request in &requests {
        assert_eq!(request.line, "GET /v1/models HTTP/1.1");
    }
    assert_eq!(
        requests[0].headers["authorization"],
        "Bearer custom-test-key"
    );
    assert_eq!(requests[2].headers["x-api-key"], "anthropic-test-key");
    assert_eq!(requests[2].headers["anthropic-version"], "2026-10-21");
    assert!(!requests[2].headers.contains_key("anthropic-beta"));
}

fn send(config: &Path, chats: &Chats, text: &str, context: Option<&str>) -> Value {
    let events = RefCell::new(Vec::new());
    let prepared = AiConfig::load(config).unwrap().prepare_send().unwrap();
    chats.send(
        prepared,
        "chat",
        text,
        context.map(str::to_owned),
        &[],
        None,
        &AtomicBool::new(false),
        &|event| {
            events
                .borrow_mut()
                .push(serde_json::to_value(event).unwrap());
        },
    );
    Value::Array(events.into_inner())
}

struct TempConfig(PathBuf);

// Bộ đếm để hai test chạy song song không trùng thư mục (đồng hồ Windows có độ phân giải thấp).
static TEMP_SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

impl TempConfig {
    fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = TEMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("fomeg-ai-http-{}-{stamp}-{seq}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        Self(dir.join("ai.json"))
    }
}

impl Drop for TempConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
        let _ = fs::remove_dir(self.0.parent().unwrap());
    }
}

struct CapturedRequest {
    line: String,
    headers: BTreeMap<String, String>,
    body: Value,
}

type MockReply = (&'static str, &'static str, &'static str);

fn mock_server(replies: &'static [MockReply]) -> (String, JoinHandle<Vec<CapturedRequest>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/v1", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        replies.iter().map(|(status, mime, body)| {
            let mut stream = accept(&listener);
            let request = read_request(&mut stream);
            write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            request
        }).collect()
    });
    (url, worker)
}

fn accept(listener: &TcpListener) -> TcpStream {
    let timeout = Duration::from_secs(5);
    let deadline = Instant::now() + timeout;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                stream.set_read_timeout(Some(timeout)).unwrap();
                stream.set_write_timeout(Some(timeout)).unwrap();
                return stream;
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(5));
            }
            Err(err) => panic!("Máy chủ thử nghiệm không nhận được yêu cầu: {err}"),
        }
    }
}

fn read_request(stream: &mut TcpStream) -> CapturedRequest {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let mut headers = BTreeMap::new();
    loop {
        let mut header = String::new();
        assert_ne!(reader.read_line(&mut header).unwrap(), 0);
        if header == "\r\n" {
            break;
        }
        let (key, value) = header.split_once(':').unwrap();
        headers.insert(key.to_ascii_lowercase(), value.trim().to_owned());
    }
    let size = headers
        .get("content-length")
        .map_or(0, |length| length.parse().unwrap());
    assert!(size <= 64 * 1024);
    let mut body = vec![0; size];
    reader.read_exact(&mut body).unwrap();
    let body = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap()
    };
    CapturedRequest {
        line: line.trim_end().to_owned(),
        headers,
        body,
    }
}

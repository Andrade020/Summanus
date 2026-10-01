use crate::{
    config::{ServerConfig, Settings},
    storage::Message,
};
use eframe::egui;
use serde_json::{json, Value};
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    net::TcpListener,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub struct ServerHandle {
    pub child: Child,
    pub port: u16,
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub enum Event {
    Recommended(Result<crate::profiles::Recommendation, String>),
    Loaded(Result<ServerHandle, String>),
    ContextMeasured {
        fingerprint: u64,
        result: Result<usize, String>,
    },
    Delta {
        generation: u64,
        content: String,
        reasoning: String,
    },
    Finished {
        generation: u64,
        result: Result<Option<f64>, String>,
        stopped: bool,
    },
}

pub fn measure_context(
    port: u16,
    fingerprint: u64,
    messages: Vec<Message>,
    thinking: bool,
    tx: Sender<Event>,
    ctx: egui::Context,
) {
    thread::spawn(move || {
        let result = count_context_tokens(port, &messages, thinking);
        let _ = tx.send(Event::ContextMeasured {
            fingerprint,
            result,
        });
        ctx.request_repaint();
    });
}

fn count_context_tokens(port: u16, messages: &[Message], thinking: bool) -> Result<usize, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(2))
        .timeout_read(Duration::from_secs(10))
        .build();
    let turns = messages
        .iter()
        .map(|message| json!({"role": message.role, "content": message.content}))
        .collect::<Vec<_>>();
    let template: Value = agent
        .post(&format!("http://127.0.0.1:{port}/apply-template"))
        .send_json(
            json!({"messages": turns, "chat_template_kwargs": {"enable_thinking": thinking}}),
        )
        .map_err(|error| error.to_string())?
        .into_json()
        .map_err(|error| error.to_string())?;
    let prompt = template
        .get("prompt")
        .and_then(Value::as_str)
        .ok_or("Servidor não retornou o prompt formatado")?;
    let tokens: Value = agent
        .post(&format!("http://127.0.0.1:{port}/tokenize"))
        .send_json(json!({"content": prompt, "add_special": false}))
        .map_err(|error| error.to_string())?
        .into_json()
        .map_err(|error| error.to_string())?;
    tokens
        .get("tokens")
        .and_then(Value::as_array)
        .map(Vec::len)
        .ok_or_else(|| "Servidor não retornou a contagem de tokens".into())
}

fn free_port() -> Result<u16, String> {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|addr| addr.port())
        .map_err(|e| e.to_string())
}

pub fn load_model(config: ServerConfig, tx: Sender<Event>, ctx: egui::Context) {
    thread::spawn(move || {
        let result = start_server(config);
        let _ = tx.send(Event::Loaded(result));
        ctx.request_repaint();
    });
}

fn start_server(config: ServerConfig) -> Result<ServerHandle, String> {
    if !config.executable.is_file() {
        return Err(format!(
            "llama-server não encontrado: {}",
            config.executable.display()
        ));
    }
    if !config.model.is_file() {
        return Err(format!(
            "Modelo GGUF não encontrado: {}",
            config.model.display()
        ));
    }
    let mut header = [0u8; 4];
    File::open(&config.model)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|e| format!("Não foi possível ler o modelo: {e}"))?;
    if &header != b"GGUF" {
        return Err("O arquivo selecionado não é um modelo GGUF válido".into());
    }
    let port = free_port()?;
    fs::create_dir_all("logs").map_err(|e| e.to_string())?;
    let log = File::create("logs/llama-server.log").map_err(|e| e.to_string())?;
    let log_err = log.try_clone().map_err(|e| e.to_string())?;
    let mut cmd = Command::new(&config.executable);
    cmd.arg("-m")
        .arg(&config.model)
        .args(["--host", "127.0.0.1", "--port"])
        .arg(port.to_string())
        .arg("-c")
        .arg(config.n_ctx.to_string())
        .arg("-ngl")
        .arg(config.n_gpu_layers.to_string())
        .args(["-fa", "on", "-np", "1", "--no-webui"])
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    if config.n_threads > 0 {
        cmd.arg("-t").arg(config.n_threads.to_string());
    }
    if config.n_threads_batch > 0 {
        cmd.arg("-tb").arg(config.n_threads_batch.to_string());
    }
    if config.n_cpu_moe > 0 {
        cmd.arg("--n-cpu-moe").arg(config.n_cpu_moe.to_string());
    }
    cmd.args(&config.extra_args);
    cmd.arg("--no-context-shift");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let child = cmd
        .spawn()
        .map_err(|e| format!("Falha ao iniciar llama-server: {e}"))?;
    let mut handle = ServerHandle { child, port };
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(2))
        .timeout_read(Duration::from_secs(2))
        .build();
    let deadline = Instant::now() + Duration::from_secs(600);
    while Instant::now() < deadline {
        if let Some(status) = handle.child.try_wait().map_err(|e| e.to_string())? {
            let tail = fs::read_to_string("logs/llama-server.log").unwrap_or_default();
            let tail = tail
                .lines()
                .rev()
                .take(12)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join("\n");
            return Err(format!("llama-server encerrou ({status}).\n{tail}"));
        }
        if agent
            .get(&format!("http://127.0.0.1:{port}/health"))
            .call()
            .is_ok()
        {
            return Ok(handle);
        }
        thread::sleep(Duration::from_millis(350));
    }
    Err("Tempo esgotado aguardando o modelo (10 minutos). Veja logs/llama-server.log".into())
}

pub fn generate(
    port: u16,
    generation: u64,
    messages: Vec<Message>,
    settings: Settings,
    stop: Arc<AtomicBool>,
    tx: Sender<Event>,
    ctx: egui::Context,
) {
    thread::spawn(move || {
        let result = stream(port, generation, &messages, &settings, &stop, &tx, &ctx);
        let stopped = stop.load(Ordering::Relaxed);
        let _ = tx.send(Event::Finished {
            generation,
            result,
            stopped,
        });
        ctx.request_repaint();
    });
}

fn stream(
    port: u16,
    generation: u64,
    messages: &[Message],
    settings: &Settings,
    stop: &AtomicBool,
    tx: &Sender<Event>,
    ctx: &egui::Context,
) -> Result<Option<f64>, String> {
    let payload = request_payload(messages, settings);
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(5))
        .timeout_read(Duration::from_secs(600))
        .build();
    let response = agent
        .post(&format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .send_json(payload)
        .map_err(|e| match e {
            ureq::Error::Status(code, resp) => format!(
                "Servidor HTTP {code}: {}",
                resp.into_string().unwrap_or_default()
            ),
            other => other.to_string(),
        })?;
    let mut reader = BufReader::new(response.into_reader());
    let mut line = String::new();
    let mut rate = None;
    let mut done = false;
    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        line.clear();
        if reader.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            break;
        }
        if let Some(error) = stream_error_line(&line) {
            return Err(format!("Servidor: {error}"));
        }
        let Some(data) = line.trim().strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data == "[DONE]" {
            done = true;
            break;
        }
        let Ok(chunk) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if let Some(error) = chunk.get("error") {
            return Err(format!("Servidor: {}", error_message(error)));
        }
        if let Some(value) = chunk
            .pointer("/timings/predicted_per_second")
            .and_then(Value::as_f64)
        {
            rate = Some(value);
        }
        for choice in chunk
            .get("choices")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let delta = &choice["delta"];
            let content = delta["content"].as_str().unwrap_or("");
            let reasoning = delta["reasoning_content"].as_str().unwrap_or("");
            if !content.is_empty() || !reasoning.is_empty() {
                if tx
                    .send(Event::Delta {
                        generation,
                        content: content.into(),
                        reasoning: reasoning.into(),
                    })
                    .is_err()
                {
                    return Ok(rate);
                }
                ctx.request_repaint_after(Duration::from_millis(30));
            }
        }
    }
    if !done && !stop.load(Ordering::Relaxed) {
        return Err("Conexão encerrada antes de completar a resposta".into());
    }
    Ok(rate)
}

fn stream_error_line(line: &str) -> Option<String> {
    let payload = line.trim().strip_prefix("error:")?.trim();
    Some(
        serde_json::from_str::<Value>(payload)
            .map(|value| error_message(&value))
            .unwrap_or_else(|_| payload.to_owned()),
    )
}

fn error_message(value: &Value) -> String {
    value
        .get("message")
        .or_else(|| value.pointer("/error/message"))
        .and_then(Value::as_str)
        .unwrap_or("erro desconhecido do servidor")
        .to_owned()
}

fn request_payload(messages: &[Message], settings: &Settings) -> Value {
    json!({
        "messages": messages.iter().map(|m| json!({"role": m.role, "content": m.content})).collect::<Vec<_>>(),
        "stream": true,
        "max_tokens": settings.max_tokens,
        "temperature": settings.temperature,
        "top_p": settings.top_p,
        "top_k": 20,
        "repeat_penalty": settings.repeat_penalty,
        "presence_penalty": settings.presence_penalty,
        "chat_template_kwargs": {"enable_thinking": settings.thinking},
        "timings_per_token": false,
        "cache_prompt": true
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn request_includes_presence_penalty() {
        let mut settings = Settings::default();
        settings.thinking = false;
        settings.presence_penalty = 1.5;
        let payload = request_payload(&[], &settings);
        assert_eq!(payload["presence_penalty"], 1.5);
        assert_eq!(payload["chat_template_kwargs"]["enable_thinking"], false);
    }

    #[test]
    fn reads_content_reasoning_and_timings_from_sse() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            loop {
                let mut chunk = [0u8; 4096];
                let received = socket.read(&mut chunk).unwrap();
                if received == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..received]);
                if let Some(header_end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n")
                {
                    let header = String::from_utf8_lossy(&request[..header_end]);
                    let body_len = header
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= header_end + 4 + body_len {
                        break;
                    }
                }
            }
            let body = concat!(
                "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"pensando\"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"resposta\"}}],\"timings\":{\"predicted_per_second\":12.5}}\n\n",
                "data: [DONE]\n\n"
            );
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            socket.write_all(response.as_bytes()).unwrap();
        });
        let (tx, rx) = std::sync::mpsc::channel();
        let messages = vec![Message {
            role: "user".into(),
            content: "teste".into(),
            reasoning: String::new(),
            speed_tps: None,
        }];
        let rate = stream(
            port,
            7,
            &messages,
            &Settings::default(),
            &AtomicBool::new(false),
            &tx,
            &egui::Context::default(),
        )
        .unwrap();
        server.join().unwrap();
        assert_eq!(rate, Some(12.5));
        let events: Vec<_> = rx.try_iter().collect();
        assert!(
            matches!(&events[0], Event::Delta { generation: 7, reasoning, .. } if reasoning == "pensando")
        );
        assert!(
            matches!(&events[1], Event::Delta { generation: 7, content, .. } if content == "resposta")
        );
    }

    #[test]
    fn reports_context_overflow_stream_error() {
        assert_eq!(
            stream_error_line("error: {\"code\":400,\"message\":\"the request exceeds the available context size\"}\n"),
            Some("the request exceeds the available context size".into())
        );
    }
}

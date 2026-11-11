//! A tiny HTTP/1.1 server for the HTTP client's tests: one scripted reply per connection,
//! every request recorded.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

pub(super) enum Reply {
    /// A complete response.
    Full {
        status: u16,
        headers: Vec<(&'static str, String)>,
        body: String,
    },
    /// An event stream: `chunks`, then the connection stays open until the client goes away,
    /// which is reported on `disconnected`.
    StreamThenHold {
        chunks: Vec<String>,
        disconnected: oneshot::Sender<()>,
    },
}

pub(super) fn json(status: u16, body: &str) -> Reply {
    Reply::Full {
        status,
        headers: vec![("Content-Type", "application/json".to_owned())],
        body: body.to_owned(),
    }
}

pub(super) fn json_with(status: u16, body: &str, headers: Vec<(&'static str, String)>) -> Reply {
    let mut all = vec![("Content-Type", "application/json".to_owned())];
    all.extend(headers);
    Reply::Full {
        status,
        headers: all,
        body: body.to_owned(),
    }
}

pub(super) fn event_stream(events: &[&str]) -> Reply {
    let body = events
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect::<String>();
    Reply::Full {
        status: 200,
        headers: vec![("Content-Type", "text/event-stream".to_owned())],
        body,
    }
}

#[derive(Debug, Clone)]
pub(super) struct Recorded {
    pub method: String,
    pub path: String,
    /// Lowercase names.
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl Recorded {
    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).unwrap()
    }
}

pub(super) struct TestServer {
    pub url: String,
    requests: Arc<Mutex<Vec<Recorded>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl TestServer {
    pub async fn start(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let replies = Arc::new(Mutex::new(VecDeque::from(replies)));
        let recorded = requests.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((socket, _)) = listener.accept().await else {
                    return;
                };
                let reply = replies.lock().unwrap().pop_front();
                let recorded = recorded.clone();
                tokio::spawn(async move { serve(socket, reply, recorded).await });
            }
        });
        Self {
            url,
            requests,
            task,
        }
    }

    pub fn requests(&self) -> Vec<Recorded> {
        self.requests.lock().unwrap().clone()
    }
}

async fn serve(mut socket: TcpStream, reply: Option<Reply>, recorded: Arc<Mutex<Vec<Recorded>>>) {
    let Some(request) = read_request(&mut socket).await else {
        return;
    };
    recorded.lock().unwrap().push(request);
    let reply = reply.unwrap_or_else(|| json(500, r#"{"error":{"message":"no reply scripted"}}"#));
    match reply {
        Reply::Full {
            status,
            headers,
            body,
        } => {
            let mut head = format!(
                "HTTP/1.1 {status} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                reason(status),
                body.len()
            );
            for (name, value) in headers {
                head.push_str(&format!("{name}: {value}\r\n"));
            }
            head.push_str("\r\n");
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(body.as_bytes()).await.unwrap();
            socket.shutdown().await.unwrap();
        }
        Reply::StreamThenHold {
            chunks,
            disconnected,
        } => {
            let head =
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";
            socket.write_all(head.as_bytes()).await.unwrap();
            for chunk in chunks {
                socket.write_all(chunk.as_bytes()).await.unwrap();
            }
            // Keep-alive comments until the client hangs up.
            loop {
                tokio::time::sleep(Duration::from_millis(10)).await;
                if socket.write_all(b": ping\n").await.is_err() {
                    break;
                }
            }
            disconnected.send(()).unwrap();
        }
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

async fn read_request(socket: &mut TcpStream) -> Option<Recorded> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break end;
        }
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let method = request_line.next()?.to_owned();
    let path = request_line.next()?.to_owned();
    let headers: HashMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length: usize = headers
        .get("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let mut body = buffer[header_end + 4..].to_vec();
    while body.len() < length {
        let read = socket.read(&mut chunk).await.ok()?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    Some(Recorded {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

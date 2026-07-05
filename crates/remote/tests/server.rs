//! End-to-end tests against a real `Server` on an ephemeral port: plain
//! HTTP for the assets and health check, tokio-tungstenite for the
//! WebSocket protocol.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use miditool_remote::{Backend, MonitorEvent, Server, Status};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

/// A backend with just enough state to observe what the server did.
#[derive(Default)]
struct StubBackend {
    active: AtomicUsize,
    panicked: AtomicBool,
    queue: Mutex<Vec<MonitorEvent>>,
}

impl Backend for StubBackend {
    fn status(&self) -> Status {
        Status {
            scenes: vec!["alpha".to_string(), "beta".to_string()],
            active: self.active.load(Ordering::Relaxed),
            dropped: 7,
        }
    }

    fn set_scene(&self, idx: usize) -> Result<(), String> {
        if idx >= 2 {
            return Err(format!("no scene {idx}"));
        }
        self.active.store(idx, Ordering::Relaxed);
        Ok(())
    }

    fn panic(&self) {
        self.panicked.store(true, Ordering::Relaxed);
    }

    fn drain_events(&self) -> Vec<MonitorEvent> {
        std::mem::take(&mut self.queue.lock().unwrap())
    }
}

fn start() -> (Server, Arc<StubBackend>) {
    let backend = Arc::new(StubBackend::default());
    let server = Server::start(0, Arc::clone(&backend) as Arc<dyn Backend>)
        .expect("server should bind an ephemeral port");
    (server, backend)
}

/// Minimal HTTP/1.1 GET; returns (status line, headers, body).
fn http_get(server: &Server, path: &str) -> (String, String, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", server.addr().port())).unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: t\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    let (status, headers) = head.split_once("\r\n").unwrap_or((head, ""));
    (status.to_string(), headers.to_lowercase(), body.to_string())
}

#[test]
fn health_answers_ok() {
    let (server, _) = start();
    let (status, _, body) = http_get(&server, "/health");
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert_eq!(body, "ok");
}

#[test]
fn assets_have_correct_content_types() {
    let (server, _) = start();
    for (path, content_type, marker) in [
        ("/", "text/html", "<!doctype html>"),
        ("/app.js", "text/javascript", "WebSocket"),
        ("/style.css", "text/css", ":root"),
    ] {
        let (status, headers, body) = http_get(&server, path);
        assert_eq!(status, "HTTP/1.1 200 OK", "{path}");
        assert!(headers.contains(content_type), "{path}: {headers}");
        assert!(body.contains(marker), "{path} should contain {marker:?}");
    }
}

type Client =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn ws_connect(server: &Server) -> Client {
    let url = format!("ws://127.0.0.1:{}/ws", server.addr().port());
    let (client, _) = tokio_tungstenite::connect_async(url)
        .await
        .expect("websocket should connect");
    client
}

/// Next JSON text frame of the given type, skipping others (a monitor
/// frame can land between two status pushes).
async fn next_frame(client: &mut Client, wanted: &str) -> Value {
    let deadline = Duration::from_secs(5);
    loop {
        let msg = tokio::time::timeout(deadline, client.next())
            .await
            .expect("timed out waiting for a frame")
            .expect("connection should stay open")
            .expect("frame should be readable");
        if let Message::Text(text) = msg {
            let value: Value = serde_json::from_str(&text).unwrap();
            if value["type"] == wanted {
                return value;
            }
        }
    }
}

#[tokio::test]
async fn ws_pushes_status_on_connect() {
    let (server, _) = start();
    let mut client = ws_connect(&server).await;
    let status = next_frame(&mut client, "status").await;
    assert_eq!(status["scenes"], json!(["alpha", "beta"]));
    assert_eq!(status["active"], 0);
    assert_eq!(status["dropped"], 7);
}

#[tokio::test]
async fn set_scene_updates_backend_and_notifies_all_clients() {
    let (server, backend) = start();
    let mut first = ws_connect(&server).await;
    let mut second = ws_connect(&server).await;
    next_frame(&mut first, "status").await;
    next_frame(&mut second, "status").await;

    first
        .send(Message::text(
            json!({"type": "set_scene", "idx": 1}).to_string(),
        ))
        .await
        .unwrap();

    for client in [&mut first, &mut second] {
        let status = next_frame(client, "status").await;
        assert_eq!(status["active"], 1);
    }
    assert_eq!(backend.active.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn failed_set_scene_still_pushes_status() {
    let (server, backend) = start();
    let mut client = ws_connect(&server).await;
    next_frame(&mut client, "status").await;

    client
        .send(Message::text(
            json!({"type": "set_scene", "idx": 99}).to_string(),
        ))
        .await
        .unwrap();

    let status = next_frame(&mut client, "status").await;
    assert_eq!(status["active"], 0, "the active scene is unchanged");
    assert_eq!(backend.active.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn panic_reaches_the_backend() {
    let (server, backend) = start();
    let mut client = ws_connect(&server).await;
    next_frame(&mut client, "status").await;

    client
        .send(Message::text(json!({"type": "panic"}).to_string()))
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_secs(5), async {
        while !backend.panicked.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("panic should reach the backend");
}

#[tokio::test]
async fn drained_events_are_broadcast() {
    let (server, backend) = start();
    let mut client = ws_connect(&server).await;
    next_frame(&mut client, "status").await;

    backend.queue.lock().unwrap().push(MonitorEvent {
        t_ms: 1234,
        kind: "note-on".to_string(),
        ch: 3,
        detail: "C4 vel 96".to_string(),
    });

    let frame = next_frame(&mut client, "events").await;
    let events = frame["events"].as_array().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["t_ms"], 1234);
    assert_eq!(events[0]["kind"], "note-on");
    assert_eq!(events[0]["ch"], 3);
    assert_eq!(events[0]["detail"], "C4 vel 96");
}

#[tokio::test]
async fn malformed_frames_are_ignored() {
    let (server, _) = start();
    let mut client = ws_connect(&server).await;
    next_frame(&mut client, "status").await;

    client.send(Message::text("not json")).await.unwrap();
    client
        .send(Message::text(json!({"type": "reboot"}).to_string()))
        .await
        .unwrap();

    // The connection survives and still answers commands.
    client
        .send(Message::text(
            json!({"type": "set_scene", "idx": 1}).to_string(),
        ))
        .await
        .unwrap();
    let status = next_frame(&mut client, "status").await;
    assert_eq!(status["active"], 1);
}

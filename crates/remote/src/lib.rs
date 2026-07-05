//! The web remote: a phone-sized control surface served from inside the
//! miditool process.
//!
//! During a performance the player parks a phone on the music stand,
//! points it at this server, and gets three things: the scene list, a
//! panic button, and a live monitor of outgoing MIDI. The crate knows
//! nothing about MIDI or the engine; the host implements [`Backend`] over
//! whatever it has (the CLI wraps its engine handle) and calls
//! [`Server::start`]. The UI is embedded in the binary, so there is
//! nothing to deploy.
//!
//! # Wire protocol
//!
//! One WebSocket per client at `/ws`, JSON text frames both ways.
//!
//! Server to client:
//! - `{"type":"status","scenes":[...],"active":0,"dropped":0}` on connect
//!   and after every scene change, to every client.
//! - `{"type":"events","events":[{"t_ms":..,"kind":..,"ch":..,"detail":..}]}`
//!   at roughly 30 Hz, only when the backend produced something.
//!
//! Client to server:
//! - `{"type":"set_scene","idx":2}`
//! - `{"type":"panic"}`
//!
//! A single drain loop feeds all clients through a bounded broadcast
//! channel, so a slow phone loses monitor frames instead of stalling the
//! loop or its neighbors; a lagged client gets a fresh status frame so it
//! never displays a stale scene.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::header::CONTENT_TYPE;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tokio::sync::{broadcast, oneshot};
use tokio::time::MissedTickBehavior;

/// How often the drain loop asks the backend for fresh monitor events.
const DRAIN_INTERVAL: Duration = Duration::from_millis(33);

/// Frames buffered per subscriber. A client that falls this far behind
/// starts losing frames rather than blocking anyone else.
const BROADCAST_CAPACITY: usize = 256;

/// WebSocket clients allowed at once. Way beyond any music stand, but a
/// ceiling keeps a misbehaving peer from piling up connections until the
/// process runs out of descriptors.
const MAX_CLIENTS: usize = 256;

const INDEX_HTML: &str = include_str!("../ui/index.html");
const APP_JS: &str = include_str!("../ui/app.js");
const STYLE_CSS: &str = include_str!("../ui/style.css");

/// One monitor entry, already humanized by the backend.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MonitorEvent {
    /// Engine time of the send, milliseconds.
    pub t_ms: u64,
    /// "note-on" | "note-off" | "cc" | "bend" | "program" | "pressure".
    pub kind: String,
    /// MIDI channel, 1-16.
    pub ch: u8,
    /// Human-readable payload, e.g. "C4 vel 96" or "cc64 = 127".
    pub detail: String,
}

/// A snapshot of what the remote shows above the monitor: the scene
/// list, which scene is live, and how many monitor events the backend
/// had to drop.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Status {
    /// Scene names, in switcher order.
    pub scenes: Vec<String>,
    /// Index into `scenes` of the active scene.
    pub active: usize,
    /// Monitor events dropped so far (a health signal, not an error).
    pub dropped: u64,
}

/// What the host must provide. All methods are called from the server's
/// own runtime threads and should return quickly; `drain_events` in
/// particular sits on the ~30 Hz monitor path.
pub trait Backend: Send + Sync + 'static {
    /// Current scenes, active index, and dropped-event count.
    fn status(&self) -> Status;
    /// Switch to scene `idx`. Errors are logged server-side; either way
    /// every client receives a fresh status push.
    fn set_scene(&self, idx: usize) -> Result<(), String>;
    /// Release everything, now. No confirmation, no result.
    fn panic(&self);
    /// Drain whatever accumulated since the last call (called ~30 Hz).
    fn drain_events(&self) -> Vec<MonitorEvent>;
    /// Throw away whatever accumulated, without formatting it. Called at
    /// the same ~30 Hz while no client is connected, so the backend's
    /// buffer never silently fills. The default drains and drops; hosts
    /// with a cheaper raw path should override it.
    fn discard_events(&self) {
        let _ = self.drain_events();
    }
}

/// The running HTTP/WebSocket server. Binds on construction, serves
/// until dropped.
pub struct Server {
    addr: SocketAddr,
    shutdown: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    /// Binds `addr` (port 0 picks a free one), spawns its own tokio
    /// runtime on a background thread, and serves until the returned
    /// `Server` is dropped. Bind loopback to keep the remote on this
    /// machine; bind `0.0.0.0` to open it to the local network.
    ///
    /// Binding happens synchronously so an occupied port fails here,
    /// not later on the server thread.
    pub fn start(addr: SocketAddr, backend: Arc<dyn Backend>) -> io::Result<Server> {
        let listener = std::net::TcpListener::bind(addr)?;
        // axum's acceptor drives the listener with the tokio reactor.
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let thread = std::thread::Builder::new()
            .name("miditool-remote".into())
            .spawn(move || serve(listener, backend, shutdown_rx))?;
        Ok(Server {
            addr,
            shutdown: Some(shutdown_tx),
            thread: Some(thread),
        })
    }

    /// The bound address. With `port = 0`, this is where the OS put us.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Everything the handlers share: the host backend, the one channel that
/// fans frames out to every connected client, and the in-flight client
/// count that enforces [`MAX_CLIENTS`].
struct AppState {
    backend: Arc<dyn Backend>,
    tx: broadcast::Sender<String>,
    clients: AtomicUsize,
}

/// Server-to-client frames. Internal tagging puts `"type"` alongside the
/// payload fields, which keeps the client's dispatch to one switch.
#[derive(serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Push {
    Status(Status),
    Events { events: Vec<MonitorEvent> },
}

/// Client-to-server frames.
#[derive(serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Command {
    SetScene { idx: usize },
    Panic,
}

/// Body of the server thread: build a runtime, serve until told to stop.
fn serve(
    listener: std::net::TcpListener,
    backend: Arc<dyn Backend>,
    shutdown: oneshot::Receiver<()>,
) {
    // A handful of light connections on a control surface: one thread is
    // plenty, and it keeps the runtime off the process's other cores.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build the remote's tokio runtime");
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::from_std(listener)
            .expect("failed to register the remote's listener with tokio");
        let (tx, _) = broadcast::channel(BROADCAST_CAPACITY);
        let state = Arc::new(AppState {
            backend,
            tx,
            clients: AtomicUsize::new(0),
        });
        tokio::spawn(drain_loop(Arc::clone(&state)));
        let app = router(state);
        let result = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown.await;
            })
            .await;
        if let Err(err) = result {
            eprintln!("remote: server error: {err}");
        }
    });
}

fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route(
            "/",
            get(|| async { asset("text/html; charset=utf-8", INDEX_HTML) }),
        )
        .route(
            "/app.js",
            get(|| async { asset("text/javascript; charset=utf-8", APP_JS) }),
        )
        .route(
            "/style.css",
            get(|| async { asset("text/css; charset=utf-8", STYLE_CSS) }),
        )
        .route("/health", get(|| async { "ok" }))
        .route("/ws", get(ws_upgrade))
        .with_state(state)
}

/// An embedded UI asset with its content type.
fn asset(content_type: &'static str, body: &'static str) -> Response {
    ([(CONTENT_TYPE, content_type)], body).into_response()
}

/// The one interval task feeding the monitor. Runs for the life of the
/// server; `broadcast::Sender::send` never blocks, so a stuck client
/// cannot reach back into this loop.
async fn drain_loop(state: Arc<AppState>) {
    let mut tick = tokio::time::interval(DRAIN_INTERVAL);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tick.tick().await;
        // With no one watching, skip the humanize-and-serialize work but
        // still empty the backend's buffer so it cannot silently fill.
        if state.tx.receiver_count() == 0 {
            state.backend.discard_events();
            continue;
        }
        let events = state.backend.drain_events();
        if events.is_empty() {
            continue;
        }
        let frame = serde_json::to_string(&Push::Events { events })
            .expect("monitor events always serialize");
        // Err just means everybody disconnected since the count above.
        let _ = state.tx.send(frame);
    }
}

async fn ws_upgrade(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> Response {
    // Take a slot before upgrading; the guard gives it back when the
    // client task ends, or right here when the house is full.
    let slot = ClientSlot::take(Arc::clone(&state));
    if slot.is_none() {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "too many clients\n",
        )
            .into_response();
    }
    ws.on_upgrade(move |socket| async move {
        let _slot = slot;
        client(socket, state).await;
    })
}

/// A held connection slot; dropping it releases the count.
struct ClientSlot(Arc<AppState>);

impl ClientSlot {
    fn take(state: Arc<AppState>) -> Option<ClientSlot> {
        state
            .clients
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                (n < MAX_CLIENTS).then_some(n + 1)
            })
            .ok()
            .map(|_| ClientSlot(state))
    }
}

impl Drop for ClientSlot {
    fn drop(&mut self) {
        self.0.clients.fetch_sub(1, Ordering::AcqRel);
    }
}

/// One connected client: forward broadcast frames out, accept commands
/// in, and disappear quietly when either side closes.
async fn client(mut socket: WebSocket, state: Arc<AppState>) {
    // Subscribe before the initial status so nothing slips between them.
    let mut rx = state.tx.subscribe();
    if send_text(&mut socket, status_frame(&state)).await.is_err() {
        return;
    }
    loop {
        tokio::select! {
            pushed = rx.recv() => match pushed {
                Ok(frame) => {
                    if send_text(&mut socket, frame).await.is_err() {
                        return;
                    }
                }
                // This client fell behind and lost frames. Monitor
                // events stay lost, but resync the status so it never
                // shows a stale scene.
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    if send_text(&mut socket, status_frame(&state)).await.is_err() {
                        return;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => return,
            },
            received = socket.recv() => match received {
                Some(Ok(Message::Text(text))) => handle_command(&text, &state),
                // Pings are answered by axum; anything else is noise.
                Some(Ok(_)) => {}
                Some(Err(_)) | None => return,
            },
        }
    }
}

async fn send_text(socket: &mut WebSocket, frame: String) -> Result<(), axum::Error> {
    socket.send(Message::Text(frame.into())).await
}

/// Apply one client command. Malformed frames are dropped on the floor;
/// a stale or buggy client must not be able to wedge the server.
fn handle_command(text: &str, state: &AppState) {
    match serde_json::from_str::<Command>(text) {
        Ok(Command::SetScene { idx }) => {
            if let Err(err) = state.backend.set_scene(idx) {
                eprintln!("remote: set_scene({idx}): {err}");
            }
            // Push fresh status to everyone either way, so an optimistic
            // client snaps to whatever actually happened.
            let _ = state.tx.send(status_frame(state));
        }
        Ok(Command::Panic) => state.backend.panic(),
        Err(_) => {}
    }
}

fn status_frame(state: &AppState) -> String {
    serde_json::to_string(&Push::Status(state.backend.status())).expect("status always serializes")
}

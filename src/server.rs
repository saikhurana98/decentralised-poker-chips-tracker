use std::{
    collections::{BTreeSet, HashMap},
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{
        ConnectInfo, DefaultBodyLimit, Query, Request, State, WebSocketUpgrade,
        ws::{CloseFrame, Message, WebSocket},
    },
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::{SinkExt, StreamExt};
use qrcode::{QrCode, render::svg};
use serde::{Deserialize, Serialize};
use tokio::{
    net::TcpListener,
    sync::{OwnedSemaphorePermit, Semaphore, mpsc, watch},
    time::{MissedTickBehavior, timeout},
};
use uuid::Uuid;

use crate::{
    engine::{self, RuleError},
    model::{
        ClientMessage, CreateRequest, Credential, GameState, JoinRequest, ServerMessage, Snapshot,
    },
    store::{Result, Store, StoreError, valid_code},
};

const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

#[derive(Clone)]
pub struct Config {
    pub address: SocketAddr,
    pub origin: String,
    pub database: PathBuf,
    pub public_directory: PathBuf,
    pub trusted_proxies: BTreeSet<IpAddr>,
    pub release_sha: String,
}

#[derive(Clone)]
struct App {
    config: Arc<Config>,
    core: Arc<Mutex<Core>>,
    slots: Arc<Semaphore>,
    shutdown: watch::Receiver<bool>,
}

#[derive(Clone)]
struct Authentication {
    code: String,
    player: String,
}
struct Client {
    sender: mpsc::Sender<Outbound>,
    cancel: watch::Sender<bool>,
    auth: Option<Authentication>,
    lobby: bool,
}

#[derive(Clone)]
enum Outbound {
    Text(Arc<str>),
    Close(u16, &'static str),
}
struct Limit {
    count: u32,
    until: Instant,
}
struct Core {
    store: Store,
    clients: HashMap<Uuid, Client>,
    limits: HashMap<String, Limit>,
}

impl Core {
    fn limited(&mut self, identity: &str, key: &str, maximum: u32, window: Duration) -> bool {
        let now = Instant::now();
        let limit = self
            .limits
            .entry(format!("{identity}:{key}"))
            .or_insert(Limit {
                count: 0,
                until: now + window,
            });
        if now > limit.until {
            limit.count = 0;
            limit.until = now + window;
        }
        limit.count = limit.count.saturating_add(1);
        limit.count > maximum
    }

    fn emit(&self, client: Uuid, data: Outbound) {
        if let Some(connection) = self.clients.get(&client)
            && connection.sender.try_send(data).is_err()
        {
            // A bounded queue prevents a stalled phone from buffering unbounded snapshots.
            let _ = connection.cancel.send(true);
        }
    }

    fn send(&self, client: Uuid, message: &ServerMessage) -> Result<()> {
        self.emit(
            client,
            Outbound::Text(Arc::from(serde_json::to_string(message)?)),
        );
        Ok(())
    }

    fn snapshot(&self, game: &GameState, code: &str, viewer: &str) -> Result<Snapshot> {
        engine::ensure(
            game.players.iter().any(|p| p.id == viewer),
            "Seat no longer available",
        )?;
        let online = self
            .clients
            .values()
            .filter_map(|c| c.auth.as_ref())
            .filter(|a| a.code == code)
            .map(|a| a.player.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut visible = game.clone();
        let undo_count = visible.history.len();
        visible.history.clear();
        Ok(Snapshot {
            game: visible,
            undo_count,
            code: code.to_owned(),
            you: viewer.to_owned(),
            pots: engine::pots(game),
            legal: engine::legal(game, viewer),
            balances: engine::balances(game)?,
            settlement: engine::settlement(game)?,
            online,
        })
    }

    fn broadcast_lobby(&self) -> Result<()> {
        if !self.clients.values().any(|c| c.lobby) {
            return Ok(());
        }
        let message = ServerMessage::Lobby {
            tables: self.store.lobby()?,
        };
        let encoded: Arc<str> = Arc::from(serde_json::to_string(&message)?);
        for (&id, client) in &self.clients {
            if client.lobby {
                self.emit(id, Outbound::Text(encoded.clone()));
            }
        }
        Ok(())
    }

    fn broadcast(&self, code: &str) -> Result<()> {
        self.broadcast_lobby()?;
        let game = self.store.get(code)?;
        for (&id, client) in &self.clients {
            if let Some(auth) = &client.auth
                && auth.code == code
            {
                match self.snapshot(&game, code, &auth.player) {
                    Ok(state) => self.send(
                        id,
                        &ServerMessage::State {
                            state: Box::new(state),
                        },
                    )?,
                    Err(_) => self.emit(id, Outbound::Close(4003, "Seat removed")),
                }
            }
        }
        Ok(())
    }

    fn identify(&mut self, client: Uuid, message: ClientMessage) -> Result<()> {
        match message {
            ClientMessage::Lobby => {
                if let Some(connection) = self.clients.get_mut(&client) {
                    connection.lobby = true;
                }
                self.send(
                    client,
                    &ServerMessage::Lobby {
                        tables: self.store.lobby()?,
                    },
                )
            }
            ClientMessage::Auth { code, token } => {
                let player = self.store.authenticate(&code, &token)?;
                let game = self.store.get(&code)?;
                engine::ensure(
                    game.players.iter().any(|p| p.id == player),
                    "Seat no longer available",
                )?;
                if let Some(connection) = self.clients.get_mut(&client) {
                    connection.auth = Some(Authentication {
                        code: code.clone(),
                        player,
                    });
                    connection.lobby = false;
                }
                self.broadcast(&code)
            }
            ClientMessage::Command { .. } => Err(RuleError("Authenticate first".into()).into()),
        }
    }

    fn process_message(&mut self, client: Uuid, ip: &str, text: &str) -> bool {
        #[derive(Deserialize)]
        struct Envelope {
            #[serde(default)]
            id: Option<String>,
        }
        let envelope = serde_json::from_str::<Envelope>(text).ok();
        let message_id = envelope.and_then(|e| e.id);
        let auth = self.clients.get(&client).and_then(|c| c.auth.clone());
        let identity = auth.as_ref().map_or(ip, |a| a.player.as_str());
        let outcome = (|| -> Result<()> {
            engine::ensure(
                !self.limited(ip, "messages", 1200, Duration::from_secs(60))
                    && !self.limited(identity, "playerMessages", 180, Duration::from_secs(60)),
                "Rate limit exceeded",
            )?;
            let message: ClientMessage = serde_json::from_str(text)
                .map_err(|_| RuleError("Invalid command envelope".into()))?;
            if let Some(auth) = &auth {
                let ClientMessage::Command {
                    id,
                    revision,
                    command,
                } = message
                else {
                    return Err(RuleError("Invalid command envelope".into()).into());
                };
                let accepted =
                    self.store
                        .execute(&auth.code, &auth.player, &id, revision, &command)?;
                // The snapshot and deduplication entry are committed before acknowledgement.
                self.send(
                    client,
                    &ServerMessage::Ack {
                        id,
                        revision: accepted.revision,
                        duplicate: accepted.duplicate,
                    },
                )?;
                self.broadcast(&auth.code)
            } else {
                self.identify(client, message)
            }
        })();
        if let Err(error) = outcome {
            if !matches!(error, StoreError::Rule(_)) {
                eprintln!("WebSocket operation failed: {error}");
            }
            let _ = self.send(
                client,
                &ServerMessage::Error {
                    id: message_id,
                    error: error.public_message(),
                },
            );
            if let Some(auth) = auth {
                match self
                    .store
                    .get(&auth.code)
                    .and_then(|game| self.snapshot(&game, &auth.code, &auth.player))
                {
                    Ok(state) => {
                        let _ = self.send(
                            client,
                            &ServerMessage::State {
                                state: Box::new(state),
                            },
                        );
                    }
                    Err(_) => self.emit(client, Outbound::Close(4003, "Seat removed")),
                }
            } else {
                self.emit(client, Outbound::Close(4003, "Invalid credentials"));
            }
        }
        self.clients
            .get(&client)
            .is_some_and(|c| c.auth.is_some() || c.lobby)
    }

    fn unregister(&mut self, client: Uuid) {
        if let Some(connection) = self.clients.remove(&client)
            && let Some(auth) = connection.auth
            && let Err(error) = self.broadcast(&auth.code)
        {
            eprintln!("Disconnect broadcast failed: {error}");
        }
    }

    fn maintain(&mut self, expire: bool) -> Result<()> {
        let now = Instant::now();
        self.limits.retain(|_, limit| now <= limit.until);
        if expire {
            let active = self
                .clients
                .values()
                .filter_map(|c| c.auth.as_ref().map(|a| a.code.clone()))
                .collect();
            if self.store.clean_expired(&active)? > 0 {
                self.broadcast_lobby()?;
            }
        }
        Ok(())
    }
}

impl App {
    async fn run<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Core) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let core = self.core.clone();
        tokio::task::spawn_blocking(move || {
            let mut locked = core
                .lock()
                .map_err(|_| StoreError::Worker("State lock unavailable".into()))?;
            operation(&mut locked)
        })
        .await
        .map_err(|error| StoreError::Worker(error.to_string()))?
    }

    fn ip(&self, peer: SocketAddr, headers: &HeaderMap) -> String {
        let peer_ip = match peer.ip() {
            IpAddr::V6(ip) => ip.to_ipv4_mapped().map_or(IpAddr::V6(ip), IpAddr::V4),
            ip => ip,
        };
        if self.config.trusted_proxies.contains(&peer_ip)
            && let Some(value) = headers.get("x-forwarded-for").and_then(|h| h.to_str().ok())
            && let Some(ip) = value
                .rsplit(',')
                .next()
                .and_then(|ip| ip.trim().parse::<IpAddr>().ok())
        {
            return ip.to_string();
        }
        peer_ip.to_string()
    }

    fn origin_allowed(&self, headers: &HeaderMap) -> bool {
        headers.get(header::ORIGIN).and_then(|v| v.to_str().ok())
            == Some(self.config.origin.as_str())
    }
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}
fn error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        Json(ErrorBody {
            error: message.into(),
        }),
    )
        .into_response()
}
fn storage_error(failure: &StoreError) -> Response {
    let status = if matches!(failure, StoreError::Rule(_)) {
        StatusCode::BAD_REQUEST
    } else {
        eprintln!("HTTP storage operation failed: {failure}");
        StatusCode::INTERNAL_SERVER_ERROR
    };
    error(status, failure.public_message())
}

async fn security_headers(request: Request<Body>, next: Next) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert("content-security-policy", HeaderValue::from_static(CSP));
    response
}

async fn health(State(app): State<App>) -> Response {
    #[derive(Serialize)]
    struct Health<'a> {
        ok: bool,
        version: &'a str,
        commit: &'a str,
    }
    let healthy = app.run(|core| core.store.health()).await.is_ok();
    (
        if healthy {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        [(header::CACHE_CONTROL, "no-store")],
        Json(Health {
            ok: healthy,
            version: env!("CARGO_PKG_VERSION"),
            commit: &app.config.release_sha,
        }),
    )
        .into_response()
}

async fn asset(app: App, filename: &'static str, content_type: &'static str) -> Response {
    match tokio::fs::read(app.config.public_directory.join(filename)).await {
        Ok(body) => (
            [
                (header::CONTENT_TYPE, content_type),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            body,
        )
            .into_response(),
        Err(failure) => {
            eprintln!("Asset read failed for {filename}: {failure}");
            error(StatusCode::NOT_FOUND, "Not found")
        }
    }
}
async fn index(State(app): State<App>) -> Response {
    asset(app, "index.html", "text/html; charset=utf-8").await
}
async fn script(State(app): State<App>) -> Response {
    asset(app, "app.js", "text/javascript; charset=utf-8").await
}
async fn stylesheet(State(app): State<App>) -> Response {
    asset(app, "style.css", "text/css; charset=utf-8").await
}

#[derive(Deserialize)]
struct QrQuery {
    #[serde(default)]
    code: String,
}
async fn qr(State(app): State<App>, Query(query): Query<QrQuery>) -> Response {
    if !valid_code(&query.code) {
        return error(StatusCode::BAD_REQUEST, "Invalid code");
    }
    let url = format!("{}/?table={}", app.config.origin, query.code);
    match QrCode::new(url.as_bytes()) {
        Ok(code) => (
            [
                (header::CONTENT_TYPE, "image/svg+xml"),
                (header::CACHE_CONTROL, "public,max-age=3600"),
            ],
            code.render::<svg::Color>()
                .min_dimensions(220, 220)
                .quiet_zone(true)
                .build(),
        )
            .into_response(),
        Err(_) => error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Could not generate invite QR",
        ),
    }
}

async fn api_create(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    api_entry(app, peer, headers, body, true).await
}
async fn api_join(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    api_entry(app, peer, headers, body, false).await
}
async fn api_entry(
    app: App,
    peer: SocketAddr,
    headers: HeaderMap,
    body: Bytes,
    create: bool,
) -> Response {
    if !app.origin_allowed(&headers) {
        return error(StatusCode::FORBIDDEN, "Origin rejected");
    }
    let ip = app.ip(peer, &headers);
    let outcome = app
        .run(move |core| {
            if core.limited(&ip, "requests", 30, Duration::from_secs(60))
                || core.limited(
                    &ip,
                    if create { "create" } else { "join" },
                    if create { 8 } else { 25 },
                    Duration::from_secs(3600),
                )
            {
                return Ok(None);
            }
            let credentials: Credential = if create {
                let request = serde_json::from_slice::<CreateRequest>(&body)
                    .map_err(|_| RuleError("Invalid table details".into()))?;
                core.store.create(&request)?
            } else {
                let request = serde_json::from_slice::<JoinRequest>(&body)
                    .map_err(|_| RuleError("Invalid join request".into()))?;
                core.store.join(&request)?
            };
            core.broadcast(&credentials.code)?;
            Ok(Some(credentials))
        })
        .await;
    match outcome {
        Ok(Some(credentials)) => {
            ([(header::CACHE_CONTROL, "no-store")], Json(credentials)).into_response()
        }
        Ok(None) => error(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many requests. Please try later.",
        ),
        Err(failure) => storage_error(&failure),
    }
}

async fn upgrade(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    socket: WebSocketUpgrade,
) -> Response {
    if !app.origin_allowed(&headers) {
        return error(StatusCode::FORBIDDEN, "Origin rejected");
    }
    let ip = app.ip(peer, &headers);
    let limit_ip = ip.clone();
    if !matches!(
        app.run(move |core| Ok(!core.limited(&limit_ip, "ws", 60, Duration::from_secs(60))))
            .await,
        Ok(true)
    ) {
        return error(StatusCode::FORBIDDEN, "Too many connections");
    }
    let Ok(permit) = app.slots.clone().try_acquire_owned() else {
        return error(StatusCode::FORBIDDEN, "Connection limit reached");
    };
    socket
        .max_message_size(8192)
        .max_frame_size(8192)
        .max_write_buffer_size(1024 * 1024)
        .on_upgrade(move |socket| session(socket, app, ip, permit))
}

async fn send_socket<S>(sender: &mut S, message: Message) -> bool
where
    S: futures_util::Sink<Message> + Unpin,
{
    matches!(
        timeout(Duration::from_secs(2), sender.send(message)).await,
        Ok(Ok(()))
    )
}
async fn close_socket<S>(sender: &mut S, code: u16, reason: &'static str)
where
    S: futures_util::Sink<Message> + Unpin,
{
    let _ = send_socket(
        sender,
        Message::Close(Some(CloseFrame {
            code,
            reason: reason.into(),
        })),
    )
    .await;
}

async fn session(socket: WebSocket, app: App, ip: String, _permit: OwnedSemaphorePermit) {
    let id = Uuid::new_v4();
    let (tx, mut outgoing) = mpsc::channel(16);
    let (cancel, mut cancelled) = watch::channel(false);
    if app
        .run(move |core| {
            core.clients.insert(
                id,
                Client {
                    sender: tx,
                    cancel,
                    auth: None,
                    lobby: false,
                },
            );
            Ok(())
        })
        .await
        .is_err()
    {
        return;
    }
    let (mut sender, mut incoming) = socket.split();
    let authentication = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(authentication);
    let mut identified = false;
    let mut alive = true;
    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(30),
        Duration::from_secs(30),
    );
    heartbeat.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut shutdown = app.shutdown.clone();
    loop {
        tokio::select! {
            _ = shutdown.changed() => { close_socket(&mut sender, 1001, "Server restarting").await; break; }
            _ = cancelled.changed() => { close_socket(&mut sender, 1013, "Client is too slow").await; break; }
            () = &mut authentication, if !identified => { close_socket(&mut sender, 4001, "Authentication required").await; break; }
            _ = heartbeat.tick() => {
                if !alive { break; } alive = false;
                if !send_socket(&mut sender, Message::Ping(Bytes::new())).await { break; }
            }
            message = outgoing.recv() => match message {
                Some(Outbound::Text(text)) => { if !send_socket(&mut sender, Message::Text(text.as_ref().into())).await { break; } }
                Some(Outbound::Close(code, reason)) => { close_socket(&mut sender, code, reason).await; break; }
                None => break,
            },
            message = incoming.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    let address = ip.clone();
                    match app.run(move |core| Ok(core.process_message(id, &address, &text))).await {
                        Ok(value) => identified = value,
                        Err(failure) => { eprintln!("Socket worker failed: {failure}"); break; }
                    }
                }
                Some(Ok(Message::Pong(_))) => alive = true,
                Some(Ok(Message::Ping(data))) => { if !send_socket(&mut sender, Message::Pong(data)).await { break; } }
                Some(Ok(Message::Binary(_))) => { close_socket(&mut sender, 1003, "Text messages required").await; break; }
                Some(Ok(Message::Close(_)) | Err(_)) | None => break,
            }
        }
    }
    if let Err(error) = app
        .run(move |core| {
            core.unregister(id);
            Ok(())
        })
        .await
    {
        eprintln!("Socket cleanup failed: {error}");
    }
}

pub async fn serve(
    config: Config,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for file in ["index.html", "app.js", "style.css"] {
        tokio::fs::metadata(config.public_directory.join(file)).await?;
    }
    let mut store = Store::open(&config.database)?;
    store.clean_expired(&BTreeSet::new())?;
    let (stop, shutdown) = watch::channel(false);
    let app = App {
        config: Arc::new(config),
        core: Arc::new(Mutex::new(Core {
            store,
            clients: HashMap::new(),
            limits: HashMap::new(),
        })),
        slots: Arc::new(Semaphore::new(500)),
        shutdown,
    };
    let maintenance = app.clone();
    let maintenance_task = tokio::spawn(async move {
        let mut minutes = 0u32;
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            minutes = (minutes + 1) % 60;
            if let Err(error) = maintenance
                .run(move |core| core.maintain(minutes == 0))
                .await
            {
                eprintln!("Maintenance failed: {error}");
            }
        }
    });
    let router = Router::new()
        .route("/", get(index))
        .route("/app.js", get(script))
        .route("/style.css", get(stylesheet))
        .route("/healthz", get(health))
        .route("/qr", get(qr))
        .route("/api/create", post(api_create))
        .route("/api/join", post(api_join))
        .route("/ws", get(upgrade))
        .fallback(|| async { error(StatusCode::NOT_FOUND, "Not found") })
        .layer(DefaultBodyLimit::max(2048))
        .layer(middleware::from_fn(security_headers))
        .with_state(app.clone());
    let listener = TcpListener::bind(app.config.address).await?;
    println!("Poker Bank listening on {}", listener.local_addr()?);
    let result = axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_signal().await;
        let _ = stop.send(true);
    })
    .await;
    maintenance_task.abort();
    result?;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
            }
            Err(error) => {
                eprintln!("Could not listen for SIGTERM: {error}");
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

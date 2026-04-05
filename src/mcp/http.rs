use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use axum::Router;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Value, json};
use tokio::sync::{RwLock as AsyncRwLock, broadcast, watch};

use crate::indexer::{self, WorkspaceConfig};
use crate::model::WorkspaceIndex;
use crate::parser::ParserRegistry;

use super::{JsonRpcRequest, JsonRpcResponse, Transport, err_response, process_jsonrpc_request};

// ---------------------------------------------------------------------------
// Shared application state
// ---------------------------------------------------------------------------

struct AppState {
    index: RwLock<WorkspaceIndex>,
    config: WorkspaceConfig,
    registry: ParserRegistry,
    /// Broadcast channel for server-initiated SSE notifications (file changes).
    notify_tx: broadcast::Sender<SseEvent>,
    /// Active sessions (async-safe — accessed from async handlers without spawn_blocking).
    sessions: AsyncRwLock<HashMap<String, SessionInfo>>,
    /// Whether to expose all tools (including extended/specialized ones).
    all_tools: bool,
    /// Wiki store (loaded at startup if available).
    wiki_store: super::WikiStoreOption,
}

struct SessionInfo {
    /// Updated on every valid access — sessions expire after [`SESSION_TTL`] of *inactivity*.
    last_accessed: Instant,
    /// Signals SSE streams to close when the session is terminated.
    close_tx: watch::Sender<bool>,
}

/// Maximum inactivity before a session is considered expired (sliding window).
const SESSION_TTL: std::time::Duration = std::time::Duration::from_secs(3600);
/// How often to refresh `last_accessed` under a write lock. Requests within this
/// window are validated with a cheaper read lock.
const REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
/// Maximum number of concurrent sessions.
const MAX_SESSIONS: usize = 1000;

#[derive(Clone, Debug)]
struct SseEvent {
    id: Option<String>,
    event: Option<String>,
    data: String,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub async fn run_http_server(
    workspace: WorkspaceIndex,
    config: WorkspaceConfig,
    watch: bool,
    debounce_ms: u64,
    addr: &str,
    all_tools: bool,
) -> anyhow::Result<()> {
    let (notify_tx, _) = broadcast::channel::<SseEvent>(256);

    // Load wiki store if available
    #[cfg(feature = "wiki")]
    let wiki_store: super::WikiStoreOption = {
        let wiki_dir = workspace.root.join(".indxr").join("wiki");
        if wiki_dir.exists() {
            crate::wiki::store::WikiStore::load(&wiki_dir).ok()
        } else {
            None
        }
    };
    #[cfg(not(feature = "wiki"))]
    let wiki_store: super::WikiStoreOption = ();

    let state = Arc::new(AppState {
        index: RwLock::new(workspace),
        config,
        registry: ParserRegistry::new(),
        notify_tx: notify_tx.clone(),
        sessions: AsyncRwLock::new(HashMap::new()),
        all_tools,
        wiki_store,
    });

    // Optionally spawn file watcher
    if watch {
        spawn_file_watcher(Arc::clone(&state), debounce_ms)?;
    }

    // Resolve address — allow `:8080` shorthand for `127.0.0.1:8080`
    let bind_addr = if addr.starts_with(':') {
        format!("127.0.0.1{addr}")
    } else {
        addr.to_string()
    };

    let app = Router::new()
        .route("/mcp", post(handle_post).delete(handle_delete))
        .route("/mcp", get(handle_get))
        .layer(DefaultBodyLimit::max(1_048_576)) // 1 MB
        .with_state(Arc::clone(&state));

    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;
    eprintln!("Listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// POST /mcp — main JSON-RPC request handler (single and batch)
// ---------------------------------------------------------------------------

async fn handle_post(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    // Validate Content-Type per MCP Streamable HTTP spec
    let content_type_ok = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.starts_with("application/json"))
        .unwrap_or(false);
    if !content_type_ok {
        let resp = err_response(
            Value::Null,
            -32700,
            "Unsupported Content-Type; expected application/json".to_string(),
        );
        return json_response(StatusCode::UNSUPPORTED_MEDIA_TYPE, &resp, None);
    }

    // Parse body as generic JSON to support both single and batch requests
    let parsed: Value = match serde_json::from_str(body.trim()) {
        Ok(v) => v,
        Err(e) => {
            let resp = err_response(Value::Null, -32700, format!("Parse error: {e}"));
            return json_response(StatusCode::OK, &resp, None);
        }
    };

    match parsed {
        Value::Array(items) if !items.is_empty() => handle_batch(state, &headers, items).await,
        Value::Array(_) => {
            // JSON-RPC 2.0: clients MUST NOT send an empty array
            let resp = err_response(Value::Null, -32600, "Empty batch request".to_string());
            json_response(StatusCode::OK, &resp, None)
        }
        value @ Value::Object(_) => handle_single(state, &headers, value).await,
        _ => {
            let resp = err_response(
                Value::Null,
                -32700,
                "Expected JSON object or array".to_string(),
            );
            json_response(StatusCode::OK, &resp, None)
        }
    }
}

// ---------------------------------------------------------------------------
// Single JSON-RPC request
// ---------------------------------------------------------------------------

async fn handle_single(state: Arc<AppState>, headers: &HeaderMap, value: Value) -> Response {
    let request: JsonRpcRequest = match serde_json::from_value(value) {
        Ok(r) => r,
        Err(e) => {
            let resp = err_response(Value::Null, -32700, format!("Parse error: {e}"));
            return json_response(StatusCode::OK, &resp, None);
        }
    };

    let is_initialize = request.method == "initialize";
    let is_notification = request.id.is_none();

    // Guard: `initialize` must be a request (with an id), not a notification.
    if is_initialize && is_notification {
        let resp = err_response(
            Value::Null,
            -32600,
            "initialize must be a request with an id, not a notification".to_string(),
        );
        return json_response(StatusCode::OK, &resp, None);
    }

    // Session enforcement: all requests except `initialize` must include a valid session.
    let session_id = if is_initialize {
        match create_session(&state).await {
            Ok(id) => Some(id),
            Err(()) => {
                let resp = err_response(
                    request.id.unwrap_or(Value::Null),
                    -32000,
                    "Too many active sessions".to_string(),
                );
                return json_response(StatusCode::SERVICE_UNAVAILABLE, &resp, None);
            }
        }
    } else {
        match validate_session(&state, headers).await {
            Ok(id) => Some(id),
            Err(resp) => return *resp,
        }
    };

    // Notifications (no id) -> 202 Accepted
    if is_notification {
        let mut builder = Response::builder().status(StatusCode::ACCEPTED);
        if let Some(ref sid) = session_id {
            builder = builder.header("Mcp-Session-Id", sid.as_str());
        }
        return builder
            .body(axum::body::Body::empty())
            .unwrap()
            .into_response();
    }

    // Dispatch the pre-parsed request via spawn_blocking to avoid blocking the
    // async runtime. All paths currently acquire a write lock because
    // process_jsonrpc_request takes &mut. A future refactor could split
    // read/write paths for better concurrency.
    let state2 = Arc::clone(&state);
    let response = match tokio::task::spawn_blocking(move || {
        let mut index = state2.index.write().unwrap_or_else(|e| {
            eprintln!("WARNING: index lock was poisoned, recovering");
            e.into_inner()
        });
        process_jsonrpc_request(
            request,
            &mut index,
            &state2.config,
            &state2.registry,
            Transport::Http,
            state2.all_tools,
            &state2.wiki_store,
        )
    })
    .await
    {
        Ok(Some(resp)) => resp,
        Ok(None) => {
            // Should not happen — notifications are handled before dispatch.
            // Clean up the session created during this initialize to avoid orphans.
            if is_initialize {
                if let Some(ref sid) = session_id {
                    cleanup_session(&state, sid).await;
                }
            }
            let resp = err_response(Value::Null, -32603, "Internal error".to_string());
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &resp,
                session_id.as_deref(),
            );
        }
        Err(_) => {
            // Handler panicked — clean up the session to avoid orphans.
            if is_initialize {
                if let Some(ref sid) = session_id {
                    cleanup_session(&state, sid).await;
                }
            }
            let resp = err_response(
                Value::Null,
                -32603,
                "Internal error: handler panicked".to_string(),
            );
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &resp,
                session_id.as_deref(),
            );
        }
    };

    json_response(StatusCode::OK, &response, session_id.as_deref())
}

// ---------------------------------------------------------------------------
// Batch JSON-RPC request (JSON-RPC 2.0 section 6)
// ---------------------------------------------------------------------------

async fn handle_batch(state: Arc<AppState>, headers: &HeaderMap, items: Vec<Value>) -> Response {
    // Check if the batch contains an initialize request
    let has_initialize = items
        .iter()
        .any(|v| v.get("method").and_then(Value::as_str) == Some("initialize"));

    // Session enforcement: validate from header, or create if batch has initialize
    let has_session_header = headers.contains_key("mcp-session-id");
    let session_id: Option<String> = if has_session_header {
        match validate_session(&state, headers).await {
            Ok(id) => Some(id),
            Err(resp) => return *resp,
        }
    } else if has_initialize {
        match create_session(&state).await {
            Ok(id) => Some(id),
            Err(()) => {
                let resp =
                    err_response(Value::Null, -32000, "Too many active sessions".to_string());
                return json_response(StatusCode::SERVICE_UNAVAILABLE, &resp, None);
            }
        }
    } else {
        return unauthorized_response(
            "Missing Mcp-Session-Id header. Send an initialize request first.",
        );
    };

    // Parse all items, preserving the id from raw JSON for better error responses
    let parsed: Vec<Result<JsonRpcRequest, (Value, String)>> = items
        .into_iter()
        .map(|item| {
            let id = item.get("id").cloned().unwrap_or(Value::Null);
            serde_json::from_value::<JsonRpcRequest>(item)
                .map_err(|e| (id, format!("Parse error: {e}")))
        })
        .collect();

    // Process all requests in a single spawn_blocking call (they share the
    // index write lock, so parallelism wouldn't help).
    let state2 = Arc::clone(&state);
    let responses = tokio::task::spawn_blocking(move || {
        let mut index = state2.index.write().unwrap_or_else(|e| {
            eprintln!("WARNING: index lock was poisoned, recovering");
            e.into_inner()
        });
        parsed
            .into_iter()
            .map(|item| match item {
                Err((id, msg)) => Some(err_response(id, -32700, msg)),
                Ok(request) if request.id.is_none() => None, // notification
                Ok(request) => process_jsonrpc_request(
                    request,
                    &mut index,
                    &state2.config,
                    &state2.registry,
                    Transport::Http,
                    state2.all_tools,
                    &state2.wiki_store,
                ),
            })
            .collect::<Vec<_>>()
    })
    .await;

    let responses = match responses {
        Ok(r) => r,
        Err(_) => {
            // Handler panicked — clean up if we created the session
            if has_initialize && !has_session_header {
                if let Some(ref sid) = session_id {
                    cleanup_session(&state, sid).await;
                }
            }
            let resp = err_response(
                Value::Null,
                -32603,
                "Internal error: handler panicked".to_string(),
            );
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &resp,
                session_id.as_deref(),
            );
        }
    };

    // Collect non-None responses (skip notifications)
    let responses: Vec<&JsonRpcResponse> = responses.iter().filter_map(|r| r.as_ref()).collect();

    // If all were notifications, return 202 Accepted
    if responses.is_empty() {
        let mut builder = Response::builder().status(StatusCode::ACCEPTED);
        if let Some(ref sid) = session_id {
            builder = builder.header("Mcp-Session-Id", sid.as_str());
        }
        return builder
            .body(axum::body::Body::empty())
            .unwrap()
            .into_response();
    }

    // Return array of responses
    let json = serde_json::to_string(&responses).unwrap();
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json");
    if let Some(ref sid) = session_id {
        builder = builder.header("Mcp-Session-Id", sid.as_str());
    }
    builder
        .body(axum::body::Body::from(json))
        .unwrap()
        .into_response()
}

// ---------------------------------------------------------------------------
// GET /mcp — SSE stream for server-to-client notifications
// ---------------------------------------------------------------------------

async fn handle_get(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let sid = match validate_session(&state, &headers).await {
        Ok(id) => id,
        Err(resp) => return *resp,
    };

    // Obtain a close-signal receiver for this session so the stream terminates
    // promptly when the session is deleted (instead of waiting for a timeout).
    let mut close_rx = {
        let sessions = state.sessions.read().await;
        match sessions.get(&sid) {
            Some(info) => info.close_tx.subscribe(),
            None => {
                return (
                    StatusCode::UNAUTHORIZED,
                    "Invalid or expired Mcp-Session-Id.",
                )
                    .into_response();
            }
        }
    };

    let mut rx = state.notify_tx.subscribe();

    let stream = async_stream::stream! {
        loop {
            tokio::select! {
                // Session was terminated (DELETE /mcp or expiry) — close immediately
                _ = close_rx.changed() => break,
                // Wait for a broadcast event (with timeout for liveness)
                result = tokio::time::timeout(std::time::Duration::from_secs(60), rx.recv()) => {
                    match result {
                        Ok(Ok(sse_event)) => {
                            let mut event = Event::default().data(sse_event.data);
                            if let Some(id) = sse_event.id {
                                event = event.id(id);
                            }
                            if let Some(name) = sse_event.event {
                                event = event.event(name);
                            }
                            yield Ok::<_, Infallible>(event);
                        }
                        Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                        Ok(Err(broadcast::error::RecvError::Closed)) => break,
                        Err(_) => {
                            // Timeout — continue; close_rx handles session termination
                            continue;
                        }
                    }
                }
            }
        }
    };

    let mut response = Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response();
    response.headers_mut().insert(
        "mcp-session-id",
        HeaderValue::from_str(&sid).expect("session ID is valid ASCII"),
    );
    response
}

// ---------------------------------------------------------------------------
// DELETE /mcp — session termination
// ---------------------------------------------------------------------------

async fn handle_delete(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    match validate_session(&state, &headers).await {
        Ok(sid) => {
            cleanup_session(&state, &sid).await;
            StatusCode::OK.into_response()
        }
        Err(resp) => *resp,
    }
}

// ---------------------------------------------------------------------------
// File watcher bridge (sync notify -> async broadcast)
// ---------------------------------------------------------------------------

fn spawn_file_watcher(state: Arc<AppState>, debounce_ms: u64) -> anyhow::Result<()> {
    let root = std::fs::canonicalize(&state.config.workspace.root)?;
    let output_path = root.join("INDEX.md");
    let cache_dir = std::fs::canonicalize(root.join(&state.config.template.cache_dir))
        .unwrap_or_else(|_| root.join(&state.config.template.cache_dir));

    let (watch_rx, guard) =
        crate::watch::spawn_watcher(&root, &cache_dir, &output_path, debounce_ms)?;

    eprintln!("File watcher enabled (debounce: {debounce_ms}ms)");

    // Bridge the sync mpsc::Receiver into the async world.
    // The guard must stay alive so the OS-level file subscription remains active.
    tokio::task::spawn_blocking(move || {
        let _guard = guard; // prevent drop
        while watch_rx.recv().is_ok() {
            eprintln!("File change detected, auto-reindexing...");

            // Re-index outside the lock, then swap in.
            let new_ws = match indexer::regenerate_workspace_index(&state.config) {
                Ok(ws) => ws,
                Err(e) => {
                    eprintln!("Auto-reindex failed: {e}");
                    continue;
                }
            };

            let file_count = new_ws.stats.total_files;
            *state.index.write().unwrap_or_else(|e| {
                eprintln!("WARNING: index lock was poisoned, recovering");
                e.into_inner()
            }) = new_ws;
            eprintln!("Auto-reindex complete ({file_count} files)");

            // Broadcast notification to all SSE listeners
            let notification = json!({
                "jsonrpc": "2.0",
                "method": "notifications/resources/updated",
                "params": { "uri": "index" }
            });
            let _ = state.notify_tx.send(SseEvent {
                id: Some(uuid::Uuid::new_v4().to_string()),
                event: Some("message".to_string()),
                data: serde_json::to_string(&notification).unwrap(),
            });
        }
    });

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Validate that the request includes a valid, non-expired Mcp-Session-Id header.
/// Uses a read lock for the fast path (session valid and recently refreshed),
/// falling back to a write lock to refresh `last_accessed` or evict expired sessions.
async fn validate_session(state: &AppState, headers: &HeaderMap) -> Result<String, Box<Response>> {
    let sid = headers
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            Box::new(
                (
                    StatusCode::UNAUTHORIZED,
                    "Missing Mcp-Session-Id header. Send an initialize request first.",
                )
                    .into_response(),
            )
        })?;

    // Fast path: read lock — valid session that was recently refreshed
    {
        let sessions = state.sessions.read().await;
        if let Some(info) = sessions.get(sid) {
            let elapsed = info.last_accessed.elapsed();
            if elapsed < SESSION_TTL && elapsed < REFRESH_INTERVAL {
                return Ok(sid.to_string());
            }
        }
    }

    // Slow path: write lock — refresh last_accessed or evict
    let mut sessions = state.sessions.write().await;
    match sessions.get_mut(sid) {
        Some(info) if info.last_accessed.elapsed() < SESSION_TTL => {
            info.last_accessed = Instant::now();
            Ok(sid.to_string())
        }
        Some(_) => {
            // Expired — evict and signal SSE streams to close
            if let Some(info) = sessions.remove(sid) {
                let _ = info.close_tx.send(true);
            }
            Err(Box::new(
                (
                    StatusCode::UNAUTHORIZED,
                    "Invalid or expired Mcp-Session-Id.",
                )
                    .into_response(),
            ))
        }
        None => Err(Box::new(
            (
                StatusCode::UNAUTHORIZED,
                "Invalid or expired Mcp-Session-Id.",
            )
                .into_response(),
        )),
    }
}

/// Create a new session, evicting expired ones first. Returns the session ID.
async fn create_session(state: &AppState) -> Result<String, ()> {
    let id = uuid::Uuid::new_v4().to_string();
    let mut sessions = state.sessions.write().await;
    evict_expired_sessions(&mut sessions);
    if sessions.len() >= MAX_SESSIONS {
        return Err(());
    }
    let (close_tx, _) = watch::channel(false);
    sessions.insert(
        id.clone(),
        SessionInfo {
            last_accessed: Instant::now(),
            close_tx,
        },
    );
    Ok(id)
}

/// Remove a session and signal any SSE streams to close.
async fn cleanup_session(state: &AppState, sid: &str) {
    let mut sessions = state.sessions.write().await;
    if let Some(info) = sessions.remove(sid) {
        let _ = info.close_tx.send(true);
    }
}

/// Remove sessions that have exceeded the TTL, signaling their SSE streams to close.
fn evict_expired_sessions(sessions: &mut HashMap<String, SessionInfo>) {
    sessions.retain(|_, info| {
        if info.last_accessed.elapsed() < SESSION_TTL {
            true
        } else {
            let _ = info.close_tx.send(true);
            false
        }
    });
}

/// Build an UNAUTHORIZED response.
fn unauthorized_response(message: &str) -> Response {
    (StatusCode::UNAUTHORIZED, message.to_string()).into_response()
}

/// Build a JSON response with optional Mcp-Session-Id header.
fn json_response(status: StatusCode, body: &JsonRpcResponse, session_id: Option<&str>) -> Response {
    let json = serde_json::to_string(body).unwrap();
    let mut builder = Response::builder()
        .status(status)
        .header("Content-Type", "application/json");
    if let Some(sid) = session_id {
        builder = builder.header("Mcp-Session-Id", sid);
    }
    builder
        .body(axum::body::Body::from(json))
        .unwrap()
        .into_response()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn test_config() -> WorkspaceConfig {
        use crate::indexer::IndexConfig;
        let template = IndexConfig {
            root: std::path::PathBuf::from("."),
            cache_dir: std::path::PathBuf::from(".indxr-cache"),
            max_file_size: 512,
            max_depth: Some(1),
            exclude: vec![],
            no_gitignore: false,
        };
        WorkspaceConfig {
            workspace: crate::workspace::single_root_workspace(
                &std::fs::canonicalize(".").unwrap(),
            ),
            template,
        }
    }

    fn test_app() -> (Router, Arc<AppState>) {
        let config = test_config();
        let workspace = crate::indexer::build_workspace_index(&config).unwrap();
        let (notify_tx, _) = broadcast::channel::<SseEvent>(16);
        #[cfg(feature = "wiki")]
        let wiki_store: super::WikiStoreOption = None;
        #[cfg(not(feature = "wiki"))]
        let wiki_store: super::WikiStoreOption = ();
        let state = Arc::new(AppState {
            index: RwLock::new(workspace),
            config,
            registry: ParserRegistry::new(),
            notify_tx,
            sessions: AsyncRwLock::new(HashMap::new()),
            all_tools: true,
            wiki_store,
        });
        let app = Router::new()
            .route("/mcp", post(handle_post).delete(handle_delete))
            .route("/mcp", get(handle_get))
            .with_state(Arc::clone(&state));
        (app, state)
    }

    async fn send_post(
        app: &Router,
        body: &str,
        session_id: Option<&str>,
    ) -> axum::http::Response<Body> {
        let mut builder = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json");
        if let Some(sid) = session_id {
            builder = builder.header("Mcp-Session-Id", sid);
        }
        let req = builder.body(Body::from(body.to_string())).unwrap();
        app.clone().oneshot(req).await.unwrap()
    }

    async fn body_json(resp: axum::http::Response<Body>) -> Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    async fn do_initialize(app: &Router) -> String {
        let resp = send_post(
            app,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            None,
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        resp.headers()
            .get("mcp-session-id")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn initialize_returns_session_and_protocol() {
        let (app, _) = test_app();
        let resp = send_post(
            &app,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            None,
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().contains_key("mcp-session-id"));

        let body = body_json(resp).await;
        assert_eq!(body["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(body["result"]["serverInfo"]["name"], "indxr");
    }

    #[tokio::test]
    async fn tools_list_requires_session() {
        let (app, _) = test_app();
        let resp = send_post(
            &app,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
            None,
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn tools_list_with_valid_session() {
        let (app, _) = test_app();
        let sid = do_initialize(&app).await;

        let resp = send_post(
            &app,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
            Some(&sid),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = body_json(resp).await;
        let tools = body["result"]["tools"].as_array().unwrap();
        assert!(!tools.is_empty());
    }

    #[tokio::test]
    async fn tools_call_get_stats() {
        let (app, _) = test_app();
        let sid = do_initialize(&app).await;

        let resp = send_post(
            &app,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_stats","arguments":{}}}"#,
            Some(&sid),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = body_json(resp).await;
        assert!(body["result"]["content"].is_array());
    }

    #[tokio::test]
    async fn notification_returns_202() {
        let (app, _) = test_app();
        let sid = do_initialize(&app).await;

        let resp = send_post(
            &app,
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            Some(&sid),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn invalid_session_rejected() {
        let (app, _) = test_app();
        let resp = send_post(
            &app,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#,
            Some("bogus-session-id"),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn delete_session_invalidates_it() {
        let (app, _) = test_app();
        let sid = do_initialize(&app).await;

        // DELETE the session
        let req = Request::builder()
            .method("DELETE")
            .uri("/mcp")
            .header("Mcp-Session-Id", &sid)
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Session should now be invalid
        let resp = send_post(
            &app,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
            Some(&sid),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn parse_error_returns_json_rpc_error() {
        let (app, _) = test_app();
        let resp = send_post(&app, "not valid json", None).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = body_json(resp).await;
        assert_eq!(body["error"]["code"], -32700);
    }

    #[tokio::test]
    async fn wrong_content_type_returns_415() {
        let (app, _) = test_app();
        let req = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header("Content-Type", "text/plain")
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

        let body = body_json(resp).await;
        assert_eq!(body["error"]["code"], -32700);
    }

    #[tokio::test]
    async fn missing_content_type_returns_415() {
        let (app, _) = test_app();
        let req = Request::builder()
            .method("POST")
            .uri("/mcp")
            .body(Body::from(
                r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            ))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn delete_without_session_returns_unauthorized() {
        let (app, _) = test_app();
        let req = Request::builder()
            .method("DELETE")
            .uri("/mcp")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn delete_bogus_session_returns_unauthorized() {
        let (app, _) = test_app();
        let req = Request::builder()
            .method("DELETE")
            .uri("/mcp")
            .header("Mcp-Session-Id", "bogus-id")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn initialize_as_notification_rejected() {
        let (app, _) = test_app();
        // initialize without an id is a notification — should be rejected
        let resp = send_post(
            &app,
            r#"{"jsonrpc":"2.0","method":"initialize","params":{}}"#,
            None,
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(!resp.headers().contains_key("mcp-session-id"));

        let body = body_json(resp).await;
        assert_eq!(body["error"]["code"], -32600);
    }

    #[tokio::test]
    async fn max_sessions_returns_503() {
        let (app, state) = test_app();

        // Fill sessions to the limit
        {
            let mut sessions = state.sessions.write().await;
            for i in 0..MAX_SESSIONS {
                let (close_tx, _) = watch::channel(false);
                sessions.insert(
                    format!("fake-session-{i}"),
                    SessionInfo {
                        last_accessed: Instant::now(),
                        close_tx,
                    },
                );
            }
        }

        // Next initialize should be rejected with 503
        let resp = send_post(
            &app,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            None,
        )
        .await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);

        let body = body_json(resp).await;
        assert_eq!(body["error"]["code"], -32000);
    }

    #[tokio::test]
    async fn expired_session_rejected_and_evicted() {
        let (app, state) = test_app();

        // Insert a session that is already expired
        let expired_sid = "expired-session-id".to_string();
        {
            let mut sessions = state.sessions.write().await;
            let (close_tx, _) = watch::channel(false);
            sessions.insert(
                expired_sid.clone(),
                SessionInfo {
                    last_accessed: Instant::now() - SESSION_TTL - std::time::Duration::from_secs(1),
                    close_tx,
                },
            );
        }

        // Request with expired session should be rejected
        let resp = send_post(
            &app,
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#,
            Some(&expired_sid),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

        // The expired session should have been eagerly removed
        let sessions = state.sessions.read().await;
        assert!(
            !sessions.contains_key(&expired_sid),
            "Expired session should be evicted on access"
        );
    }

    #[tokio::test]
    async fn sse_get_requires_session() {
        let (app, _) = test_app();
        let req = Request::builder()
            .method("GET")
            .uri("/mcp")
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn sse_get_with_valid_session_returns_stream() {
        let (app, _) = test_app();
        let sid = do_initialize(&app).await;

        let req = Request::builder()
            .method("GET")
            .uri("/mcp")
            .header("Mcp-Session-Id", &sid)
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get("content-type")
                .unwrap()
                .to_str()
                .unwrap(),
            "text/event-stream"
        );
        assert_eq!(
            resp.headers()
                .get("mcp-session-id")
                .unwrap()
                .to_str()
                .unwrap(),
            sid,
            "SSE response should include Mcp-Session-Id header"
        );
    }

    // -----------------------------------------------------------------------
    // Batch JSON-RPC tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn batch_initialize_and_tools_list() {
        let (app, _) = test_app();
        let batch = serde_json::to_string(&json!([
            {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}
        ]))
        .unwrap();

        let resp = send_post(&app, &batch, None).await;
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().contains_key("mcp-session-id"));

        let body = body_json(resp).await;
        let arr = body.as_array().expect("batch response should be an array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["result"]["protocolVersion"], "2025-03-26");
        assert!(arr[1]["result"]["tools"].is_array());
    }

    #[tokio::test]
    async fn batch_with_session() {
        let (app, _) = test_app();
        let sid = do_initialize(&app).await;

        let batch = serde_json::to_string(&json!([
            {"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "get_stats", "arguments": {}}}
        ]))
        .unwrap();

        let resp = send_post(&app, &batch, Some(&sid)).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = body_json(resp).await;
        let arr = body.as_array().expect("batch response should be an array");
        assert_eq!(arr.len(), 2);
    }

    #[tokio::test]
    async fn batch_without_session_no_initialize_returns_401() {
        let (app, _) = test_app();
        let batch = serde_json::to_string(&json!([
            {"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}
        ]))
        .unwrap();

        let resp = send_post(&app, &batch, None).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn empty_batch_returns_error() {
        let (app, _) = test_app();
        let resp = send_post(&app, "[]", None).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = body_json(resp).await;
        assert_eq!(body["error"]["code"], -32600);
    }

    #[tokio::test]
    async fn batch_all_notifications_returns_202() {
        let (app, _) = test_app();
        let sid = do_initialize(&app).await;

        let batch = serde_json::to_string(&json!([
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            {"jsonrpc": "2.0", "method": "notifications/cancelled"}
        ]))
        .unwrap();

        let resp = send_post(&app, &batch, Some(&sid)).await;
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn batch_with_parse_errors() {
        let (app, _) = test_app();
        let sid = do_initialize(&app).await;

        // Mix of valid request and invalid (missing method field)
        let batch = r#"[
            {"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}},
            {"jsonrpc": "2.0", "id": 2}
        ]"#;

        let resp = send_post(&app, batch, Some(&sid)).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let body = body_json(resp).await;
        let arr = body.as_array().expect("batch response should be an array");
        assert_eq!(arr.len(), 2);
        // First should succeed
        assert!(arr[0]["result"].is_object());
        // Second should be a parse error with preserved id
        assert_eq!(arr[1]["id"], 2);
        assert_eq!(arr[1]["error"]["code"], -32700);
    }
}

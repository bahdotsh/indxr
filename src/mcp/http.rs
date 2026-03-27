use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use axum::Router;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde_json::{Value, json};
use tokio::sync::broadcast;

use crate::indexer::{self, IndexConfig};
use crate::model::CodebaseIndex;
use crate::parser::ParserRegistry;

use super::{JsonRpcRequest, JsonRpcResponse, Transport, err_response, process_jsonrpc_request};

// ---------------------------------------------------------------------------
// Shared application state
// ---------------------------------------------------------------------------

struct AppState {
    index: RwLock<CodebaseIndex>,
    config: IndexConfig,
    registry: ParserRegistry,
    /// Broadcast channel for server-initiated SSE notifications (file changes).
    notify_tx: broadcast::Sender<SseEvent>,
    /// Active sessions.
    sessions: RwLock<HashMap<String, SessionInfo>>,
}

struct SessionInfo {
    /// Used for session TTL enforcement.
    created_at: Instant,
}

/// Maximum session age before it's considered expired.
const SESSION_TTL: std::time::Duration = std::time::Duration::from_secs(3600);
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
    index: CodebaseIndex,
    config: IndexConfig,
    watch: bool,
    debounce_ms: u64,
    addr: &str,
) -> anyhow::Result<()> {
    let (notify_tx, _) = broadcast::channel::<SseEvent>(256);

    let state = Arc::new(AppState {
        index: RwLock::new(index),
        config,
        registry: ParserRegistry::new(),
        notify_tx: notify_tx.clone(),
        sessions: RwLock::new(HashMap::new()),
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
// POST /mcp — main JSON-RPC request handler
// ---------------------------------------------------------------------------

async fn handle_post(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: String,
) -> Response {
    // Parse request early to determine if it's an initialize (which creates a session)
    let request: JsonRpcRequest = match serde_json::from_str(body.trim()) {
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
        let id = uuid::Uuid::new_v4().to_string();
        let mut sessions = state.sessions.write().unwrap_or_else(|e| e.into_inner());
        // Evict expired sessions and enforce max limit
        evict_expired_sessions(&mut sessions);
        if sessions.len() >= MAX_SESSIONS {
            let resp = err_response(
                request.id.unwrap_or(Value::Null),
                -32000,
                "Too many active sessions".to_string(),
            );
            return json_response(StatusCode::SERVICE_UNAVAILABLE, &resp, None);
        }
        sessions.insert(
            id.clone(),
            SessionInfo {
                created_at: Instant::now(),
            },
        );
        Some(id)
    } else {
        match validate_session(&state, &headers) {
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
        let mut index = state2.index.write().unwrap_or_else(|e| e.into_inner());
        process_jsonrpc_request(
            request,
            &mut index,
            &state2.config,
            &state2.registry,
            Transport::Http,
        )
    })
    .await
    {
        Ok(Some(resp)) => resp,
        Ok(None) => {
            // Should not happen — notifications are handled before dispatch
            let resp = err_response(Value::Null, -32603, "Internal error".to_string());
            return json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &resp,
                session_id.as_deref(),
            );
        }
        Err(_) => {
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
// GET /mcp — SSE stream for server-to-client notifications
// ---------------------------------------------------------------------------

async fn handle_get(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let sid = match validate_session(&state, &headers) {
        Ok(id) => id,
        Err(resp) => return *resp,
    };

    let mut rx = state.notify_tx.subscribe();
    let state_for_stream = Arc::clone(&state);
    let sid_for_stream = sid.clone();

    let stream = async_stream::stream! {
        loop {
            // Check for events with a timeout so we can periodically verify the session
            match tokio::time::timeout(std::time::Duration::from_secs(60), rx.recv()).await {
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
                    // Timeout — check if session is still valid
                    let sessions = state_for_stream.sessions.read().unwrap_or_else(|e| e.into_inner());
                    if !matches!(sessions.get(&sid_for_stream), Some(info) if info.created_at.elapsed() < SESSION_TTL) {
                        break;
                    }
                }
            }
        }
    };

    let mut response = Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response();
    response
        .headers_mut()
        .insert("mcp-session-id", sid.parse().unwrap());
    response
}

// ---------------------------------------------------------------------------
// DELETE /mcp — session termination
// ---------------------------------------------------------------------------

async fn handle_delete(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    match validate_session(&state, &headers) {
        Ok(sid) => {
            state
                .sessions
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&sid);
            StatusCode::OK.into_response()
        }
        Err(resp) => *resp,
    }
}

// ---------------------------------------------------------------------------
// File watcher bridge (sync notify -> async broadcast)
// ---------------------------------------------------------------------------

fn spawn_file_watcher(state: Arc<AppState>, debounce_ms: u64) -> anyhow::Result<()> {
    let root = std::fs::canonicalize(&state.config.root)?;
    let output_path = root.join("INDEX.md");
    let cache_dir = std::fs::canonicalize(root.join(&state.config.cache_dir))
        .unwrap_or_else(|_| root.join(&state.config.cache_dir));

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
            let new_index = match indexer::regenerate_index_file(&state.config) {
                Ok(idx) => idx,
                Err(e) => {
                    eprintln!("Auto-reindex failed: {e}");
                    continue;
                }
            };

            let file_count = new_index.files.len();
            *state.index.write().unwrap_or_else(|e| e.into_inner()) = new_index;
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
/// Uses a read lock in the happy path; only upgrades to a write lock to evict expired sessions.
fn validate_session(state: &AppState, headers: &HeaderMap) -> Result<String, Box<Response>> {
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

    // Fast path: read lock for valid sessions
    {
        let sessions = state.sessions.read().unwrap_or_else(|e| e.into_inner());
        match sessions.get(sid) {
            Some(info) if info.created_at.elapsed() < SESSION_TTL => {
                return Ok(sid.to_string());
            }
            _ => {}
        }
    }

    // Slow path: write lock to evict expired session (if it exists)
    {
        let mut sessions = state.sessions.write().unwrap_or_else(|e| e.into_inner());
        if matches!(sessions.get(sid), Some(info) if info.created_at.elapsed() >= SESSION_TTL) {
            sessions.remove(sid);
        }
    }

    Err(Box::new(
        (
            StatusCode::UNAUTHORIZED,
            "Invalid or expired Mcp-Session-Id.",
        )
            .into_response(),
    ))
}

/// Remove sessions that have exceeded the TTL.
fn evict_expired_sessions(sessions: &mut HashMap<String, SessionInfo>) {
    sessions.retain(|_, info| info.created_at.elapsed() < SESSION_TTL);
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

    fn test_config() -> IndexConfig {
        IndexConfig {
            root: std::path::PathBuf::from("."),
            cache_dir: std::path::PathBuf::from(".indxr-cache"),
            max_file_size: 512,
            max_depth: Some(1),
            exclude: vec![],
            no_gitignore: false,
        }
    }

    fn test_app() -> (Router, Arc<AppState>) {
        let config = test_config();
        let index = crate::indexer::build_index(&config).unwrap();
        let (notify_tx, _) = broadcast::channel::<SseEvent>(16);
        let state = Arc::new(AppState {
            index: RwLock::new(index),
            config,
            registry: ParserRegistry::new(),
            notify_tx,
            sessions: RwLock::new(HashMap::new()),
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
            let mut sessions = state.sessions.write().unwrap();
            for i in 0..MAX_SESSIONS {
                sessions.insert(
                    format!("fake-session-{i}"),
                    SessionInfo {
                        created_at: Instant::now(),
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
            let mut sessions = state.sessions.write().unwrap();
            sessions.insert(
                expired_sid.clone(),
                SessionInfo {
                    created_at: Instant::now() - SESSION_TTL - std::time::Duration::from_secs(1),
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
        let sessions = state.sessions.read().unwrap();
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
}

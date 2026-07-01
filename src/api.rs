use crate::app::{
    AllowSlaveFlag, ApiBindState, ApiDevice, BindStateFlag, BoundPort, ReadOnlyFlag, StatusFlag,
};
use crate::modbus::ModbusDevice;
use crate::register::RegisterType;
use crate::state::ConnectionStatus;
use crate::writes_log::{self, SharedWritesLog, WriteKind};
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::Ordering;
use tokio::net::{TcpListener, TcpSocket};

#[derive(Clone)]
struct ApiState {
    device: ApiDevice,
    writes_log: SharedWritesLog,
    read_only: ReadOnlyFlag,
    allow_slave_id: AllowSlaveFlag,
    status: StatusFlag,
}

#[derive(Deserialize)]
struct ReadRequest {
    #[serde(rename = "type")]
    register_type: RegisterType,
    address: u16,
    count: u16,
    #[serde(default)]
    slave_id: Option<u8>,
}

#[derive(Serialize)]
struct ReadResponse {
    values: Vec<u16>,
}

#[derive(Deserialize)]
struct WriteRequest {
    #[serde(rename = "type")]
    register_type: RegisterType,
    address: u16,
    values: Vec<u16>,
    #[serde(default)]
    slave_id: Option<u8>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    device_present: bool,
    read_only: bool,
}

#[allow(clippy::too_many_arguments)]
pub async fn serve(
    port: u16,
    device: ApiDevice,
    bound: BoundPort,
    writes_log: SharedWritesLog,
    read_only: ReadOnlyFlag,
    allow_slave_id: AllowSlaveFlag,
    status: StatusFlag,
    bind: BindStateFlag,
) {
    let router = Router::new()
        .route("/read", post(read_handler))
        .route("/write", post(write_handler))
        .route("/health", get(health_handler))
        .layer(middleware::from_fn(log_requests))
        .with_state(ApiState {
            device,
            writes_log,
            read_only,
            allow_slave_id,
            status,
        });

    let listener = match bind_reusable(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port))) {
        Ok(listener) => listener,
        Err(e) => {
            log::error!("API server failed to bind port {port}: {e}");
            bind.store(ApiBindState::Failed.code(), Ordering::Relaxed);
            return;
        }
    };
    bind.store(ApiBindState::Bound.code(), Ordering::Relaxed);
    match listener.local_addr() {
        Ok(addr) => {
            bound.store(addr.port(), Ordering::Relaxed);
            log::info!("API server listening on {addr}");
        }
        Err(_) => log::info!("API server listening"),
    }

    if let Err(e) = axum::serve(listener, router).await {
        log::error!("API server error: {e}");
    }
}

async fn log_requests(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    log::info!("API in: {method} {uri}");
    let response = next.run(request).await;
    log::info!("API out: {method} {uri} {}", response.status());
    response
}

async fn read_handler(State(state): State<ApiState>, Json(request): Json<ReadRequest>) -> Response {
    log::info!(
        "API read {:?}@{}:{} slave={}",
        request.register_type,
        request.address,
        request.count,
        describe_slave(request.slave_id)
    );
    if slave_override_forbidden(&state, request.slave_id) {
        log::warn!("API read rejected: slave id override is disabled");
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(device) = current(&state.device) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let result = device
        .read_typed(
            request.slave_id,
            request.register_type,
            request.address,
            request.count,
        )
        .await;
    match result {
        Ok(values) => Json(ReadResponse { values }).into_response(),
        Err(e) => {
            log::error!("API read failed: {e}");
            StatusCode::BAD_GATEWAY.into_response()
        }
    }
}

async fn write_handler(
    State(state): State<ApiState>,
    Json(request): Json<WriteRequest>,
) -> StatusCode {
    log::info!(
        "API write {:?}@{} slave={} {:?}",
        request.register_type,
        request.address,
        describe_slave(request.slave_id),
        request.values
    );
    if state.read_only.load(Ordering::Relaxed) {
        log::warn!("API write rejected due to read-only mode");
        return StatusCode::FORBIDDEN;
    }
    if slave_override_forbidden(&state, request.slave_id) {
        log::warn!("API write rejected: slave id override is disabled");
        return StatusCode::FORBIDDEN;
    }
    if !request.register_type.is_writable() {
        log::warn!(
            "API write rejected: {:?} is read-only",
            request.register_type
        );
        return StatusCode::BAD_REQUEST;
    }
    let Some(device) = current(&state.device) else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let result = device
        .write_typed(
            request.slave_id,
            request.register_type,
            request.address,
            &request.values,
        )
        .await;
    match result {
        Ok(()) => {
            writes_log::append(
                &state.writes_log,
                request.address,
                WriteKind::Multiple(request.values),
                None,
            );
            StatusCode::NO_CONTENT
        }
        Err(e) => {
            log::error!("API write failed: {e}");
            StatusCode::BAD_GATEWAY
        }
    }
}

async fn health_handler(State(state): State<ApiState>) -> Response {
    let device_present = current(&state.device).is_some();
    let code = state.status.load(Ordering::Relaxed);
    let read_only = state.read_only.load(Ordering::Relaxed);

    let body = Json(HealthResponse {
        status: ConnectionStatus::label_from_code(code),
        device_present,
        read_only,
    });

    let http = if device_present && ConnectionStatus::code_serving(code) {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (http, body).into_response()
}

fn bind_reusable(addr: SocketAddr) -> std::io::Result<TcpListener> {
    let socket = TcpSocket::new_v4()?;
    socket.set_reuseaddr(true)?;
    socket.bind(addr)?;
    socket.listen(1024)
}

fn current(device: &ApiDevice) -> Option<ModbusDevice> {
    device.lock().ok().and_then(|guard| guard.clone())
}

fn describe_slave(slave: Option<u8>) -> String {
    slave.map_or_else(|| "default".to_string(), |id| id.to_string())
}

fn slave_override_forbidden(state: &ApiState, slave: Option<u8>) -> bool {
    slave.is_some() && !state.allow_slave_id.load(Ordering::Relaxed)
}

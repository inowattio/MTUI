use crate::app::{
    AllowRawFlag, AllowUnitFlag, ApiBindState, ApiDevice, BindStateFlag, BoundPort, CommStats,
    ReadOnlyFlag, SharedStats, StatTarget, StatusFlag,
};
use crate::compat::Elapsed;
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
use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::sync::{MutexGuard, PoisonError};
use tokio::net::{TcpListener, TcpSocket};
use tokio_modbus::ExceptionCode;

pub const DEFAULT_PORT: u16 = 26127;

#[derive(Clone)]
pub struct ApiState {
    pub device: ApiDevice,
    pub bound: BoundPort,
    pub writes_log: SharedWritesLog,
    pub read_only: ReadOnlyFlag,
    pub allow_unit_id: AllowUnitFlag,
    pub allow_raw: AllowRawFlag,
    pub status: StatusFlag,
    pub bind: BindStateFlag,
    pub stats: SharedStats,
}

#[derive(Deserialize)]
struct ReadRequest {
    #[serde(rename = "type")]
    register_type: RegisterType,
    address: u16,
    count: u16,
    #[serde(default)]
    unit_id: Option<u8>,
}

#[derive(Serialize)]
struct ReadResponse {
    values: Vec<u16>,
}

#[derive(Serialize)]
struct FailureResponse {
    exception: Option<u8>,
    timed_out: bool,
}

impl FailureResponse {
    fn from_error(error: &anyhow::Error) -> Self {
        Self {
            exception: error
                .downcast_ref::<ExceptionCode>()
                .map(|&code| code.into()),
            timed_out: error.downcast_ref::<Elapsed>().is_some(),
        }
    }
}

fn failure(error: &anyhow::Error) -> Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(FailureResponse::from_error(error)),
    )
        .into_response()
}

#[derive(Deserialize)]
struct WriteRequest {
    #[serde(rename = "type")]
    register_type: RegisterType,
    address: u16,
    values: Vec<u16>,
    #[serde(default)]
    unit_id: Option<u8>,
}

#[derive(Deserialize)]
struct RawRequest {
    code: u8,
    #[serde(default)]
    data: Vec<u8>,
    #[serde(default)]
    unit_id: Option<u8>,
}

#[derive(Serialize, Deserialize)]
struct RawResponse {
    data: Vec<u8>,
}

#[derive(Serialize)]
struct HealthResponse {
    version: &'static str,
    status: &'static str,
    device_present: bool,
    unit_id: Option<u8>,
    read_only: bool,
    unit_id_override: bool,
    raw_function: bool,
}

pub async fn serve(addr: SocketAddr, state: ApiState) {
    let bound = state.bound.clone();
    let bind = state.bind.clone();
    let router = Router::new()
        .route("/read", post(read_handler))
        .route("/write", post(write_handler))
        .route("/raw", post(raw_handler))
        .route("/health", get(health_handler))
        .layer(middleware::from_fn(log_requests))
        .with_state(state);

    let listener = match bind_requested(addr) {
        Ok(listener) => listener,
        Err(e) => {
            log::error!("API server failed to bind {addr}: {e}");
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
        "API read {:?}@{}:{} unit={}",
        request.register_type,
        request.address,
        request.count,
        describe_unit(request.unit_id)
    );
    if unit_override_forbidden(&state, request.unit_id) {
        log::warn!("API read rejected: unit id override is disabled");
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(device) = current(&state.device) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let result = device
        .read_timed(
            request.unit_id,
            request.register_type,
            request.address,
            request.count,
        )
        .await;
    match result {
        Ok((values, latency)) => {
            record_outcome(&state, true);
            stats(&state).reads.record_ok(latency);
            Json(ReadResponse { values }).into_response()
        }
        Err(_) if device.is_closed() => StatusCode::SERVICE_UNAVAILABLE.into_response(),
        Err(e) => {
            log::error!("API read failed: {e}");
            record_outcome(&state, false);
            stats(&state).reads.record_error(
                &e.to_string(),
                Some(StatTarget::Read(
                    request.register_type,
                    request.address,
                    request.count,
                )),
            );
            failure(&e)
        }
    }
}

async fn write_handler(
    State(state): State<ApiState>,
    Json(request): Json<WriteRequest>,
) -> Response {
    log::info!(
        "API write {:?}@{} unit={} {:?}",
        request.register_type,
        request.address,
        describe_unit(request.unit_id),
        request.values
    );
    if state.read_only.load(Ordering::Relaxed) {
        log::warn!("API write rejected due to read-only mode");
        return StatusCode::FORBIDDEN.into_response();
    }
    if unit_override_forbidden(&state, request.unit_id) {
        log::warn!("API write rejected: unit id override is disabled");
        return StatusCode::FORBIDDEN.into_response();
    }
    if !request.register_type.is_writable() {
        log::warn!(
            "API write rejected: {:?} is read-only",
            request.register_type
        );
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Some(device) = current(&state.device) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let result = device
        .write_timed(
            request.unit_id,
            request.register_type,
            request.address,
            &request.values,
        )
        .await;
    match result {
        Ok(((), latency)) => {
            record_outcome(&state, true);
            stats(&state).writes.record_ok(latency);
            writes_log::append(
                &state.writes_log,
                request.unit_id.unwrap_or_else(|| device.unit()),
                request.address,
                WriteKind::Multiple(request.values),
                None,
                if request.register_type == RegisterType::Coil {
                    0x0F
                } else {
                    0x10
                },
            );
            StatusCode::NO_CONTENT.into_response()
        }
        Err(_) if device.is_closed() => StatusCode::SERVICE_UNAVAILABLE.into_response(),
        Err(e) => {
            log::error!("API write failed: {e}");
            record_outcome(&state, false);
            stats(&state).writes.record_error(
                &e.to_string(),
                Some(StatTarget::Write(
                    request.register_type,
                    request.address,
                    request.values.len() as u16,
                )),
            );
            failure(&e)
        }
    }
}

async fn raw_handler(State(state): State<ApiState>, Json(request): Json<RawRequest>) -> Response {
    log::info!(
        "API raw {:#04X} unit={} {} byte(s)",
        request.code,
        describe_unit(request.unit_id),
        request.data.len()
    );
    if !state.allow_raw.load(Ordering::Relaxed) {
        log::warn!("API raw rejected: raw function is disabled");
        return StatusCode::NOT_IMPLEMENTED.into_response();
    }
    if unit_override_forbidden(&state, request.unit_id) {
        log::warn!("API raw rejected: unit id override is disabled");
        return StatusCode::UNPROCESSABLE_ENTITY.into_response();
    }
    let Some(device) = current(&state.device) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let result = device
        .custom_timed(request.unit_id, request.code, &request.data)
        .await;
    match result {
        Ok((data, latency)) => {
            record_outcome(&state, true);
            stats(&state).others.record_ok(latency);
            Json(RawResponse { data }).into_response()
        }
        Err(_) if device.is_closed() => StatusCode::SERVICE_UNAVAILABLE.into_response(),
        Err(e) => {
            log::error!("API raw failed: {e}");
            record_outcome(&state, false);
            stats(&state)
                .others
                .record_error(&e.to_string(), Some(StatTarget::Function(request.code)));
            failure(&e)
        }
    }
}

async fn health_handler(State(state): State<ApiState>) -> Response {
    let device = current(&state.device);
    let device_present = device.is_some();
    let code = state.status.load(Ordering::Relaxed);
    let read_only = state.read_only.load(Ordering::Relaxed);

    let body = Json(HealthResponse {
        version: env!("CARGO_PKG_VERSION"),
        status: ConnectionStatus::label_from_code(code),
        device_present,
        unit_id: device.map(|device| device.unit()),
        read_only,
        unit_id_override: state.allow_unit_id.load(Ordering::Relaxed),
        raw_function: state.allow_raw.load(Ordering::Relaxed),
    });

    let http = if device_present && ConnectionStatus::code_serving(code) {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (http, body).into_response()
}

fn bind_requested(addr: SocketAddr) -> std::io::Result<TcpListener> {
    if addr.port() != 0 {
        return bind_reusable(addr);
    }
    match bind_reusable(SocketAddr::new(addr.ip(), DEFAULT_PORT)) {
        Ok(listener) => Ok(listener),
        Err(e) => {
            log::warn!(
                "API server could not bind default port {DEFAULT_PORT}: {e}; picking any free port"
            );
            bind_reusable(SocketAddr::new(addr.ip(), 0))
        }
    }
}

fn bind_reusable(addr: SocketAddr) -> std::io::Result<TcpListener> {
    let socket = if addr.is_ipv4() {
        TcpSocket::new_v4()?
    } else {
        TcpSocket::new_v6()?
    };
    socket.set_reuseaddr(true)?;
    socket.bind(addr)?;
    socket.listen(1024)
}

fn current(device: &ApiDevice) -> Option<ModbusDevice> {
    device.lock().ok().and_then(|guard| guard.clone())
}

fn stats(state: &ApiState) -> MutexGuard<'_, CommStats> {
    state.stats.lock().unwrap_or_else(PoisonError::into_inner)
}

fn describe_unit(unit: Option<u8>) -> String {
    unit.map_or_else(|| "default".to_string(), |id| id.to_string())
}

fn record_outcome(state: &ApiState, ok: bool) {
    let status = if ok {
        ConnectionStatus::Connected
    } else {
        ConnectionStatus::Error(String::new())
    };
    state.status.store(status.code(), Ordering::Relaxed);
}

fn unit_override_forbidden(state: &ApiState, unit: Option<u8>) -> bool {
    unit.is_some() && !state.allow_unit_id.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::{
        ApiState, FailureResponse, RawRequest, RawResponse, ReadRequest, bind_requested,
        health_handler, raw_handler, read_handler,
    };
    use crate::app::{CommStats, StatTarget};
    use crate::compat::Elapsed;
    use crate::modbus::{DeviceConfig, ModbusDevice, exception_error};
    use crate::register::RegisterType;
    use crate::writes_log::WritesLogState;
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::{Json, body};
    use std::net::{Ipv4Addr, SocketAddr};
    use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio_modbus::ExceptionCode;

    async fn mock_state() -> ApiState {
        let device = ModbusDevice::new(&DeviceConfig::default()).await.unwrap();
        ApiState {
            device: Arc::new(Mutex::new(Some(device))),
            bound: Arc::new(AtomicU16::new(0)),
            writes_log: Arc::new(Mutex::new(WritesLogState::default())),
            read_only: Arc::new(AtomicBool::new(false)),
            allow_unit_id: Arc::new(AtomicBool::new(false)),
            allow_raw: Arc::new(AtomicBool::new(false)),
            status: Arc::new(AtomicU8::new(0)),
            bind: Arc::new(AtomicU8::new(0)),
            stats: Arc::new(Mutex::new(CommStats::default())),
        }
    }

    fn raw(code: u8, data: &[u8], unit_id: Option<u8>) -> Json<RawRequest> {
        Json(RawRequest {
            code,
            data: data.to_vec(),
            unit_id,
        })
    }

    #[tokio::test]
    async fn raw_is_not_implemented_until_the_setting_is_on() {
        let state = mock_state().await;
        let response = raw_handler(State(state.clone()), raw(0x08, &[0, 0], None)).await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);

        state.allow_raw.store(true, Ordering::Relaxed);
        let response = raw_handler(State(state), raw(0x08, &[0, 0], None)).await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn raw_echoes_the_mock_payload() {
        let state = mock_state().await;
        state.allow_raw.store(true, Ordering::Relaxed);
        let response = raw_handler(State(state), raw(0x08, &[0x12, 0x34], None)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: RawResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed.data, vec![0x12, 0x34]);
    }

    #[tokio::test]
    async fn raw_needs_the_unit_override_for_a_unit_id_but_ignores_read_only() {
        let state = mock_state().await;
        state.allow_raw.store(true, Ordering::Relaxed);

        let response = raw_handler(State(state.clone()), raw(0x08, &[], Some(5))).await;
        assert_eq!(
            response.status(),
            StatusCode::UNPROCESSABLE_ENTITY,
            "unit override off"
        );

        state.allow_unit_id.store(true, Ordering::Relaxed);
        let response = raw_handler(State(state.clone()), raw(0x08, &[], Some(5))).await;
        assert_eq!(response.status(), StatusCode::OK, "unit override on");

        state.read_only.store(true, Ordering::Relaxed);
        let response = raw_handler(State(state), raw(0x08, &[], None)).await;
        assert_eq!(response.status(), StatusCode::OK, "read-only is ignored");
    }

    #[tokio::test]
    async fn api_reads_count_towards_the_shared_stats() {
        let state = mock_state().await;
        let read = |address, count| {
            Json(ReadRequest {
                register_type: RegisterType::Holding,
                address,
                count,
                unit_id: None,
            })
        };

        read_handler(State(state.clone()), read(0, 1)).await;
        read_handler(State(state.clone()), read(1108, 2)).await;

        let reads = state.stats.lock().unwrap().reads.clone();
        assert_eq!((reads.ok, reads.errors), (1, 1));
        assert_eq!(
            reads.last_error().and_then(|e| e.target),
            Some(StatTarget::Read(RegisterType::Holding, 1108, 2))
        );
    }

    async fn health(state: ApiState) -> serde_json::Value {
        let response = health_handler(State(state)).await;
        let bytes = body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn health_reports_the_unit_and_api_settings() {
        let state = mock_state().await;
        let body = health(state.clone()).await;
        assert_eq!(body["unit_id"], 1);
        assert_eq!(body["unit_id_override"], false);
        assert_eq!(body["raw_function"], false);

        state.allow_unit_id.store(true, Ordering::Relaxed);
        state.allow_raw.store(true, Ordering::Relaxed);
        let body = health(state.clone()).await;
        assert_eq!(body["unit_id_override"], true);
        assert_eq!(body["raw_function"], true);

        *state.device.lock().unwrap() = None;
        let body = health(state).await;
        assert!(body["unit_id"].is_null());
    }

    #[tokio::test]
    async fn the_server_binds_to_the_configured_host() {
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = free.local_addr().unwrap().port();
        drop(free);

        let listener = bind_requested(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).unwrap();
        let bound = listener.local_addr().unwrap();
        assert_eq!(bound.ip(), Ipv4Addr::LOCALHOST);
        assert_eq!(bound.port(), port);
    }

    #[test]
    fn failures_expose_the_exception_number_when_there_is_one() {
        let custom = FailureResponse::from_error(&exception_error(ExceptionCode::Custom(0x8F)));
        assert_eq!(custom.exception, Some(143));
        assert!(!custom.timed_out);

        let known =
            FailureResponse::from_error(&exception_error(ExceptionCode::IllegalDataAddress));
        assert_eq!(known.exception, Some(2));

        let transport = FailureResponse::from_error(&anyhow::anyhow!("connection reset"));
        assert_eq!(transport.exception, None);
        assert!(!transport.timed_out);
    }

    #[test]
    fn timeouts_are_flagged_without_an_exception() {
        let timeout = FailureResponse::from_error(&anyhow::Error::from(Elapsed));
        assert_eq!(timeout.exception, None);
        assert!(timeout.timed_out);
    }
}

use crate::modbus::ModbusDevice;
use crate::register::RegisterType;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

pub type ApiDevice = Arc<Mutex<Option<ModbusDevice>>>;
pub type BoundPort = Arc<AtomicU16>;

#[derive(Deserialize)]
struct ReadRequest {
    #[serde(rename = "type")]
    register_type: RegisterType,
    address: u16,
    count: u16,
}

#[derive(Serialize)]
struct ReadResponse {
    values: Vec<u16>,
}

#[derive(Deserialize)]
struct WriteRequest {
    address: u16,
    values: Vec<u16>,
}

pub async fn serve(port: u16, device: ApiDevice, bound: BoundPort) {
    let router = Router::new()
        .route("/read", post(read_handler))
        .route("/write", post(write_handler))
        .with_state(device);

    let listener = match TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await {
        Ok(listener) => listener,
        Err(e) => {
            log::error!("API server failed to bind port {port}: {e}");
            return;
        }
    };
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

async fn read_handler(State(device): State<ApiDevice>, Json(request): Json<ReadRequest>) -> Response {
    log::info!("API read {:?}@{}:{}", request.register_type, request.address, request.count);
    let Some(device) = current(&device) else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let result = match request.register_type {
        RegisterType::Holding => device.holdings(request.address, request.count).await,
        RegisterType::Input => device.inputs(request.address, request.count).await,
    };
    match result {
        Ok(values) => Json(ReadResponse { values }).into_response(),
        Err(e) => {
            log::error!("API read failed: {e}");
            StatusCode::BAD_GATEWAY.into_response()
        }
    }
}

async fn write_handler(
    State(device): State<ApiDevice>,
    Json(request): Json<WriteRequest>,
) -> StatusCode {
    log::info!("API write {}:{:?}", request.address, request.values);
    let Some(device) = current(&device) else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    match device.write_registers(request.address, &request.values).await {
        Ok(()) => StatusCode::NO_CONTENT,
        Err(e) => {
            log::error!("API write failed: {e}");
            StatusCode::BAD_GATEWAY
        }
    }
}

fn current(device: &ApiDevice) -> Option<ModbusDevice> {
    device.lock().ok().and_then(|guard| guard.clone())
}

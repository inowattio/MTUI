use crate::api::{
    ApiState, RawRequest, ReadRequest, WriteRequest, health_handler, raw_handler, read_handler,
    write_handler,
};
use crate::app::RegisterMap;
use crate::custom::CustomRule;
use crate::modbus::{WordOrder, exception_message};
use crate::register::{RegisterCell, RegisterType};
use axum::Json;
use axum::body::{self, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;
use std::sync::PoisonError;
use std::sync::atomic::Ordering;
use tokio_modbus::ExceptionCode;

const PROTOCOL_VERSIONS: [&str; 3] = ["2025-11-25", "2025-06-18", "2024-11-05"];

const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

const INSTRUCTIONS: &str = "Tools act on the Modbus device currently connected in mtui. \
Addresses are 0-based Modbus protocol addresses. Call device_status first: it reports the \
connection state, the default unit id and which optional abilities (writes, unit id override, \
raw functions) the user has enabled in mtui's settings. Call list_registers to learn what the \
addresses mean: it returns the names and decoding rules the user configured, and read_registers \
with decode set applies those rules to the words it reads.";

const UNIT_OVERRIDE_DISABLED: &str =
    "unit_id is not allowed: enable \"API unit id override\" in mtui's settings or omit it";
const NO_DEVICE: &str = "no Modbus device is connected in mtui";

type RpcError = (i64, String);

pub async fn handler(State(state): State<ApiState>, headers: HeaderMap, body: Bytes) -> Response {
    if !state.allow_mcp.load(Ordering::Relaxed) {
        log::warn!("MCP request rejected: MCP is disabled");
        return StatusCode::NOT_FOUND.into_response();
    }
    if !origin_allowed(&headers) {
        log::warn!("MCP request rejected: foreign origin");
        return StatusCode::FORBIDDEN.into_response();
    }
    if let Some(version) = unsupported_version(&headers) {
        log::warn!("MCP request rejected: unsupported protocol version {version}");
        let message = format!("unsupported MCP-Protocol-Version {version}");
        return (
            StatusCode::BAD_REQUEST,
            Json(error(Value::Null, INVALID_REQUEST, &message)),
        )
            .into_response();
    }
    let message: Value = match serde_json::from_slice(&body) {
        Ok(message) => message,
        Err(e) => return Json(error(Value::Null, PARSE_ERROR, &e.to_string())).into_response(),
    };
    if !message.is_object() {
        return Json(error(
            Value::Null,
            INVALID_REQUEST,
            "expected a single JSON-RPC message",
        ))
        .into_response();
    }
    let id = message.get("id");
    let valid_id = id
        .filter(|id| id.is_string() || id.is_number())
        .cloned()
        .unwrap_or(Value::Null);
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Json(error(valid_id, INVALID_REQUEST, "jsonrpc must be \"2.0\"")).into_response();
    }
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        if message.get("result").is_some() || message.get("error").is_some() {
            return StatusCode::ACCEPTED.into_response();
        }
        return Json(error(valid_id, INVALID_REQUEST, "missing method")).into_response();
    };
    if id.is_none() {
        return StatusCode::ACCEPTED.into_response();
    }
    if valid_id.is_null() {
        return Json(error(
            Value::Null,
            INVALID_REQUEST,
            "id must be a string or a number",
        ))
        .into_response();
    }
    let id = valid_id;

    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let outcome = match method {
        "initialize" => Ok(initialize(&params)),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => call(state, &params).await,
        _ => Err((METHOD_NOT_FOUND, format!("unknown method {method}"))),
    };
    let reply = match outcome {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message)) => error(id, code, &message),
    };
    Json(reply).into_response()
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn origin_allowed(headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    let Some((_, authority)) = origin.split_once("://") else {
        return false;
    };
    let host = match authority.find(']') {
        Some(end) => &authority[..=end],
        None => authority.split(':').next().unwrap_or(authority),
    };
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

fn unsupported_version(headers: &HeaderMap) -> Option<String> {
    let version = headers.get("mcp-protocol-version")?;
    let version = String::from_utf8_lossy(version.as_bytes());
    (!PROTOCOL_VERSIONS.contains(&version.as_ref())).then(|| version.into_owned())
}

fn initialize(params: &Value) -> Value {
    let version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .filter(|requested| PROTOCOL_VERSIONS.contains(requested))
        .unwrap_or(PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "mtui", "title": "MTUI", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

fn tools() -> Value {
    let unit_id = json!({
        "type": "integer",
        "minimum": 0,
        "maximum": 255,
        "description": "Target this unit for this call only; needs \"API unit id override\" enabled. Omit to use the configured unit."
    });
    json!([
        {
            "name": "device_status",
            "title": "Device status",
            "description": "Report mtui's version, the connection state, the default unit id and which optional abilities are enabled (read_only blocks writes, unit_id_override allows unit_id, raw_function allows raw_function).",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        },
        {
            "name": "list_registers",
            "title": "List registers",
            "description": "List the registers the user described in mtui: each has its `type`, `address`, an optional `name` and an optional decoding `rule`. A rule reads the words at `words` (in that order) as `repr`, assembled with `word_order`, then applies `ops` in sequence, maps the result through `enum` or names its set `bits`, and shows it with `decimals` and `unit`. Does not touch the device.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type": { "type": "string", "enum": ["holding", "input", "coil", "discrete"], "description": "Only list registers of this type." }
                },
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": false }
        },
        {
            "name": "read_registers",
            "title": "Read registers",
            "description": "Read `count` consecutive items of `type` starting at `address`. holding and input return raw 16-bit values; coil and discrete return 0 or 1 per bit. With `decode`, also returns `decoded`: one entry per configured rule whose words all fall inside the read range, with its `address`, `name`, `text` as mtui shows it and `value` when the result is a number.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type": { "type": "string", "enum": ["holding", "input", "coil", "discrete"] },
                    "address": { "type": "integer", "minimum": 0, "maximum": 65535 },
                    "count": { "type": "integer", "minimum": 1, "maximum": 65535 },
                    "decode": { "type": "boolean", "default": false, "description": "Apply the configured rules to the words read." },
                    "unit_id": unit_id
                },
                "required": ["type", "address", "count"],
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": true }
        },
        {
            "name": "write_registers",
            "title": "Write registers",
            "description": "Write consecutive holding registers (16-bit values) or coils (0 is off, anything else is on) starting at `address`. Rejected while mtui is in read-only mode.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type": { "type": "string", "enum": ["holding", "coil"] },
                    "address": { "type": "integer", "minimum": 0, "maximum": 65535 },
                    "values": {
                        "type": "array",
                        "items": { "type": "integer", "minimum": 0, "maximum": 65535 },
                        "minItems": 1
                    },
                    "unit_id": unit_id
                },
                "required": ["type", "address", "values"],
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": true, "idempotentHint": true, "openWorldHint": true }
        },
        {
            "name": "raw_function",
            "title": "Raw Modbus function",
            "description": "Send an arbitrary Modbus function `code` with `data` as the request bytes and return the response bytes, uninterpreted. Disabled unless \"API raw function\" is enabled; read-only mode does not apply.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "code": { "type": "integer", "minimum": 1, "maximum": 255 },
                    "data": {
                        "type": "array",
                        "items": { "type": "integer", "minimum": 0, "maximum": 255 }
                    },
                    "unit_id": unit_id
                },
                "required": ["code"],
                "additionalProperties": false
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": true, "idempotentHint": false, "openWorldHint": true }
        }
    ])
}

async fn call(state: ApiState, params: &Value) -> Result<Value, RpcError> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err((INVALID_PARAMS, "missing tool name".to_string()));
    };
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    log::info!("MCP tool call {name}");
    let outcome = match name {
        "device_status" => status(state).await,
        "list_registers" => list(&state, arguments),
        "read_registers" => read(state, arguments).await,
        "write_registers" => write(state, arguments).await,
        "raw_function" => raw(state, arguments).await,
        _ => return Err((INVALID_PARAMS, format!("unknown tool {name}"))),
    };
    Ok(match outcome {
        Ok(content) => json!({
            "content": [{ "type": "text", "text": content.to_string() }],
            "structuredContent": content,
            "isError": false
        }),
        Err(message) => json!({
            "content": [{ "type": "text", "text": message }],
            "isError": true
        }),
    })
}

async fn status(state: ApiState) -> Result<Value, String> {
    json_body(health_handler(State(state)).await).await
}

#[derive(Deserialize)]
struct ListArguments {
    #[serde(rename = "type", default)]
    register_type: Option<RegisterType>,
}

fn list(state: &ApiState, arguments: Value) -> Result<Value, String> {
    let ListArguments { register_type } = arguments_as(arguments)?;
    let map = state
        .registers
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    Ok(json!({ "registers": register_list(&map, register_type) }))
}

fn register_list(map: &RegisterMap, only: Option<RegisterType>) -> Vec<Value> {
    let cells: BTreeSet<RegisterCell> =
        map.labels.keys().chain(map.rules.keys()).copied().collect();
    cells
        .into_iter()
        .filter(|&(kind, _)| only.is_none_or(|only| only == kind))
        .map(|cell| {
            let mut entry = Map::new();
            entry.insert("type".into(), json!(cell.0));
            entry.insert("address".into(), json!(cell.1));
            if let Some(name) = map.labels.get(&cell) {
                entry.insert("name".into(), json!(name));
            }
            if let Some(rule) = map.rules.get(&cell) {
                entry.insert("rule".into(), rule_description(rule, map.word_order));
            }
            Value::Object(entry)
        })
        .collect()
}

fn rule_description(rule: &CustomRule, order: WordOrder) -> Value {
    let mut description = Map::new();
    description.insert("repr".into(), json!(rule.repr.label()));
    description.insert(
        "words".into(),
        json!(rule.word_addresses().collect::<Vec<_>>()),
    );
    description.insert("word_order".into(), json!(rule.word_order.unwrap_or(order)));
    if !rule.ops.is_empty() {
        let ops: Vec<String> = rule.ops.iter().map(|op| op.display()).collect();
        description.insert("ops".into(), json!(ops));
    }
    if !rule.enum_map.is_empty() {
        let entries: Map<String, Value> = rule
            .enum_map
            .iter()
            .map(|e| (e.value.to_string(), json!(e.text)))
            .collect();
        description.insert("enum".into(), Value::Object(entries));
    }
    if !rule.bits.is_empty() {
        let bits: Map<String, Value> = rule
            .bits
            .iter()
            .map(|e| (e.bit.to_string(), json!(e.name)))
            .collect();
        description.insert("bits".into(), Value::Object(bits));
    }
    if let Some(decimals) = rule.decimals {
        description.insert("decimals".into(), json!(decimals));
    }
    if !rule.prefix.trim().is_empty() {
        description.insert("prefix".into(), json!(rule.prefix.trim()));
    }
    if !rule.suffix.trim().is_empty() {
        description.insert("unit".into(), json!(rule.suffix.trim()));
    }
    Value::Object(description)
}

#[derive(Deserialize)]
struct ReadArguments {
    #[serde(flatten)]
    request: ReadRequest,
    #[serde(default)]
    decode: bool,
}

async fn read(state: ApiState, arguments: Value) -> Result<Value, String> {
    let ReadArguments { request, decode } = arguments_as(arguments)?;
    let (kind, start) = (request.register_type, request.address);
    let registers = state.registers.clone();
    let response = read_handler(State(state), Json(request)).await;
    match response.status() {
        StatusCode::OK => {
            let mut body = json_body(response).await?;
            if decode {
                let values: Vec<u16> =
                    serde_json::from_value(body["values"].take()).map_err(|e| e.to_string())?;
                let map = registers.lock().unwrap_or_else(PoisonError::into_inner);
                body["decoded"] = json!(decoded(&map, kind, start, &values));
                body["values"] = json!(values);
            }
            Ok(body)
        }
        StatusCode::FORBIDDEN => Err(UNIT_OVERRIDE_DISABLED.to_string()),
        _ => Err(device_failure(response).await),
    }
}

async fn write(state: ApiState, arguments: Value) -> Result<Value, String> {
    let request: WriteRequest = arguments_as(arguments)?;
    let written = request.values.len();
    let read_only = state.read_only.load(Ordering::Relaxed);
    let response = write_handler(State(state), Json(request)).await;
    match response.status() {
        StatusCode::NO_CONTENT => Ok(json!({ "written": written })),
        StatusCode::FORBIDDEN if read_only => {
            Err("mtui is in read-only mode, writes are disabled".to_string())
        }
        StatusCode::FORBIDDEN => Err(UNIT_OVERRIDE_DISABLED.to_string()),
        StatusCode::BAD_REQUEST => {
            Err("input and discrete registers cannot be written".to_string())
        }
        _ => Err(device_failure(response).await),
    }
}

async fn raw(state: ApiState, arguments: Value) -> Result<Value, String> {
    let request: RawRequest = arguments_as(arguments)?;
    let response = raw_handler(State(state), Json(request)).await;
    match response.status() {
        StatusCode::OK => json_body(response).await,
        StatusCode::NOT_IMPLEMENTED => Err(
            "raw functions are disabled: enable \"API raw function\" in mtui's settings"
                .to_string(),
        ),
        StatusCode::UNPROCESSABLE_ENTITY => Err(UNIT_OVERRIDE_DISABLED.to_string()),
        _ => Err(device_failure(response).await),
    }
}

fn decoded(map: &RegisterMap, kind: RegisterType, start: u16, values: &[u16]) -> Vec<Value> {
    map.rules
        .range((kind, 0)..=(kind, u16::MAX))
        .filter_map(|(&cell, rule)| {
            let words: Vec<u16> = rule
                .word_addresses()
                .map(|address| {
                    let offset = address.checked_sub(start)?;
                    values.get(usize::from(offset)).copied()
                })
                .collect::<Option<_>>()?;
            let mut entry = Map::new();
            entry.insert("address".into(), json!(cell.1));
            if let Some(name) = map.labels.get(&cell) {
                entry.insert("name".into(), json!(name));
            }
            entry.insert("text".into(), json!(rule.evaluate(&words, map.word_order)));
            if let Some(value) = rule.numeric(&words, map.word_order) {
                entry.insert("value".into(), json!(value));
            }
            Some(Value::Object(entry))
        })
        .collect()
}

fn arguments_as<T: serde::de::DeserializeOwned>(arguments: Value) -> Result<T, String> {
    serde_json::from_value(arguments).map_err(|e| format!("invalid arguments: {e}"))
}

async fn json_body(response: Response) -> Result<Value, String> {
    let bytes = body::to_bytes(response.into_body(), usize::MAX)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

async fn device_failure(response: Response) -> String {
    let status = response.status();
    if status == StatusCode::SERVICE_UNAVAILABLE {
        return NO_DEVICE.to_string();
    }
    if status != StatusCode::BAD_GATEWAY {
        return format!("rejected with HTTP {status}");
    }
    let body = json_body(response).await.unwrap_or_default();
    if body["timed_out"] == true {
        return "the device did not answer in time".to_string();
    }
    match body["exception"]
        .as_u64()
        .and_then(|code| u8::try_from(code).ok())
    {
        Some(code) => format!(
            "the device answered with an exception: {}",
            exception_message(ExceptionCode::new(code), false)
        ),
        None => "the device call failed (transport error)".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{decoded, handler, origin_allowed, register_list};
    use crate::api::ApiState;
    use crate::api::tests::mock_state;
    use crate::app::RegisterMap;
    use crate::custom::{BitEntry, CustomOp, CustomRepr, CustomRule, EnumEntry, OpKind};
    use crate::modbus::WordOrder;
    use crate::register::RegisterType;
    use axum::body::{self, Bytes};
    use axum::extract::State;
    use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
    use serde_json::{Value, json};
    use std::sync::atomic::Ordering;

    async fn mcp_state() -> ApiState {
        let state = mock_state().await;
        state.allow_mcp.store(true, Ordering::Relaxed);
        state
    }

    async fn send(state: ApiState, message: Value) -> (StatusCode, Value) {
        let response = handler(
            State(state),
            HeaderMap::new(),
            Bytes::from(message.to_string()),
        )
        .await;
        let status = response.status();
        let bytes = body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, body)
    }

    async fn call_tool(state: ApiState, name: &str, arguments: Value) -> Value {
        let message = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        });
        send(state, message).await.1["result"].clone()
    }

    #[tokio::test]
    async fn initialize_echoes_a_supported_version_and_falls_back_otherwise() {
        let state = mcp_state().await;
        let init = |version: &str| {
            json!({
                "jsonrpc": "2.0",
                "id": 7,
                "method": "initialize",
                "params": { "protocolVersion": version, "capabilities": {}, "clientInfo": { "name": "t", "version": "0" } }
            })
        };

        let (_, body) = send(state.clone(), init("2025-06-18")).await;
        assert_eq!(body["id"], 7);
        assert_eq!(body["result"]["protocolVersion"], "2025-06-18");
        assert!(body["result"]["capabilities"]["tools"].is_object());

        for unsupported in ["1999-01-01", "2025-03-26"] {
            let (_, body) = send(state.clone(), init(unsupported)).await;
            assert_eq!(body["result"]["protocolVersion"], "2025-11-25");
        }
    }

    #[tokio::test]
    async fn an_unsupported_protocol_version_header_is_a_bad_request() {
        let state = mcp_state().await;
        let ping = Bytes::from(json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }).to_string());
        let with = |version: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(
                "mcp-protocol-version",
                HeaderValue::from_str(version).unwrap(),
            );
            headers
        };

        let response = handler(State(state.clone()), with("2025-03-26"), ping.clone()).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let response = handler(State(state), with("2025-06-18"), ping).await;
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn malformed_ids_and_versions_are_invalid_requests() {
        let state = mcp_state().await;
        let invalid = [
            (
                json!({ "jsonrpc": "2.0", "id": null, "method": "ping" }),
                Value::Null,
            ),
            (
                json!({ "jsonrpc": "2.0", "id": [1], "method": "ping" }),
                Value::Null,
            ),
            (json!({ "id": 3, "method": "ping" }), json!(3)),
            (
                json!({ "jsonrpc": "1.0", "id": "a", "method": "ping" }),
                json!("a"),
            ),
        ];
        for (message, id) in invalid {
            let (status, body) = send(state.clone(), message.clone()).await;
            assert_eq!(status, StatusCode::OK, "{message}");
            assert_eq!(body["error"]["code"], -32600, "{message}");
            assert_eq!(body["id"], id, "{message}");
            assert!(body.get("result").is_none(), "{message}");
        }

        let (_, body) = send(
            state,
            json!({ "jsonrpc": "2.0", "id": "a", "method": "ping" }),
        )
        .await;
        assert_eq!(body["id"], "a");
        assert!(body["result"].is_object());
    }

    #[tokio::test]
    async fn the_endpoint_is_missing_while_the_setting_is_off() {
        let state = mcp_state().await;
        state.allow_mcp.store(false, Ordering::Relaxed);
        let message = json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" });
        let (status, _) = send(state, message).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn notifications_are_accepted_without_a_body() {
        let state = mcp_state().await;
        let message = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        let (status, _) = send(state, message).await;
        assert_eq!(status, StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn unknown_methods_and_malformed_bodies_are_json_rpc_errors() {
        let state = mcp_state().await;
        let message = json!({ "jsonrpc": "2.0", "id": 1, "method": "resources/list" });
        let (_, body) = send(state.clone(), message).await;
        assert_eq!(body["error"]["code"], -32601);

        let response = handler(State(state), HeaderMap::new(), Bytes::from("{")).await;
        let bytes = body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(body["error"]["code"], -32700);
    }

    #[tokio::test]
    async fn every_listed_tool_has_a_schema() {
        let state = mcp_state().await;
        let message = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" });
        let (_, body) = send(state, message).await;
        let tools = body["result"]["tools"].as_array().unwrap();
        let names: Vec<_> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            [
                "device_status",
                "list_registers",
                "read_registers",
                "write_registers",
                "raw_function"
            ]
        );
        assert!(tools.iter().all(|t| t["inputSchema"]["type"] == "object"));
    }

    #[tokio::test]
    async fn reads_return_values_and_device_errors_are_tool_errors() {
        let state = mcp_state().await;
        let ok = call_tool(
            state.clone(),
            "read_registers",
            json!({ "type": "holding", "address": 0, "count": 1 }),
        )
        .await;
        assert_eq!(ok["isError"], false);
        assert_eq!(
            ok["structuredContent"]["values"].as_array().unwrap().len(),
            1
        );

        let failed = call_tool(
            state.clone(),
            "read_registers",
            json!({ "type": "holding", "address": 1108, "count": 2 }),
        )
        .await;
        assert_eq!(failed["isError"], true);
        assert!(
            failed["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("0x20"),
            "{failed}"
        );

        let invalid = call_tool(state, "read_registers", json!({ "type": "holding" })).await;
        assert_eq!(invalid["isError"], true);
        assert!(
            invalid["content"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with("invalid arguments")
        );
    }

    #[tokio::test]
    async fn the_api_settings_gate_the_tools() {
        let state = mcp_state().await;
        state.read_only.store(true, Ordering::Relaxed);
        let write = call_tool(
            state.clone(),
            "write_registers",
            json!({ "type": "holding", "address": 0, "values": [1] }),
        )
        .await;
        assert_eq!(write["isError"], true);
        assert!(
            write["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("read-only")
        );

        let raw = call_tool(state.clone(), "raw_function", json!({ "code": 65 })).await;
        assert_eq!(raw["isError"], true);
        assert!(
            raw["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("disabled")
        );

        let unit = call_tool(
            state,
            "read_registers",
            json!({ "type": "holding", "address": 0, "count": 1, "unit_id": 5 }),
        )
        .await;
        assert!(
            unit["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("unit id override")
        );
    }

    #[tokio::test]
    async fn writes_go_through_when_allowed() {
        let state = mcp_state().await;
        let write = call_tool(
            state,
            "write_registers",
            json!({ "type": "holding", "address": 50, "values": [42] }),
        )
        .await;
        assert_eq!(write["isError"], false);
        assert_eq!(write["structuredContent"]["written"], 1);
    }

    fn rule(address: u16, repr: CustomRepr) -> CustomRule {
        CustomRule {
            address,
            repr,
            ..Default::default()
        }
    }

    fn sample_map() -> RegisterMap {
        let mut power = rule(40, CustomRepr::U32);
        power.ops = vec![CustomOp {
            op: OpKind::Div,
            v: 10.0,
        }];
        power.suffix = " W".to_string();
        let mut mode = rule(50, CustomRepr::U16);
        mode.enum_map = vec![EnumEntry {
            value: 2,
            text: "charging".to_string(),
        }];
        let mut alarms = rule(7, CustomRepr::U16);
        alarms.bits = vec![BitEntry {
            bit: 1,
            name: "overheat".to_string(),
        }];
        let mut map = RegisterMap::default();
        map.labels
            .insert((RegisterType::Holding, 40), "Grid power".to_string());
        map.labels
            .insert((RegisterType::Coil, 3), "Relay".to_string());
        map.rules.insert((RegisterType::Holding, 40), power);
        map.rules.insert((RegisterType::Holding, 50), mode);
        map.rules.insert((RegisterType::Input, 7), alarms);
        map
    }

    #[test]
    fn the_register_list_merges_labels_and_rules() {
        let map = sample_map();
        let all = register_list(&map, None);
        let cells: Vec<_> = all
            .iter()
            .map(|e| (e["type"].as_str().unwrap(), e["address"].as_u64().unwrap()))
            .collect();
        assert_eq!(
            cells,
            [("holding", 40), ("holding", 50), ("input", 7), ("coil", 3)]
        );

        assert_eq!(
            all[0],
            json!({
                "type": "holding",
                "address": 40,
                "name": "Grid power",
                "rule": { "repr": "u32", "words": [40, 41], "word_order": "abcd", "ops": ["/10"], "unit": "W" }
            })
        );
        assert_eq!(all[1]["rule"]["enum"], json!({ "2": "charging" }));
        assert!(all[1].get("name").is_none());
        assert_eq!(all[2]["rule"]["bits"], json!({ "1": "overheat" }));
        assert_eq!(
            all[3],
            json!({ "type": "coil", "address": 3, "name": "Relay" })
        );

        let inputs = register_list(&map, Some(RegisterType::Input));
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0]["address"], 7);
    }

    #[test]
    fn a_rule_word_order_overrides_the_device_order() {
        let mut map = sample_map();
        map.word_order = WordOrder::CDAB;
        map.rules
            .get_mut(&(RegisterType::Holding, 50))
            .unwrap()
            .word_order = Some(WordOrder::DCBA);
        let all = register_list(&map, Some(RegisterType::Holding));
        assert_eq!(all[0]["rule"]["word_order"], "cdab");
        assert_eq!(all[1]["rule"]["word_order"], "dcba");
    }

    #[test]
    fn only_rules_read_in_full_are_decoded() {
        let map = sample_map();
        let mut values = vec![0u16; 12];
        values[0] = 0x0001;
        values[1] = 0x0002;
        values[10] = 2;
        assert_eq!(
            decoded(&map, RegisterType::Holding, 40, &values),
            [
                json!({ "address": 40, "name": "Grid power", "text": "6553.8 W", "value": 6553.8 }),
                json!({ "address": 50, "text": "charging" })
            ]
        );

        assert_eq!(decoded(&map, RegisterType::Holding, 41, &values).len(), 1);
        assert!(decoded(&map, RegisterType::Holding, 40, &values[..1]).is_empty());
        assert_eq!(
            decoded(&map, RegisterType::Input, 7, &[0b10]),
            [json!({ "address": 7, "text": "overheat" })]
        );
    }

    #[tokio::test]
    async fn reads_decode_only_when_asked() {
        let state = mcp_state().await;
        state
            .registers
            .lock()
            .unwrap()
            .rules
            .insert((RegisterType::Holding, 0), rule(0, CustomRepr::U16));
        let read =
            |decode: bool| json!({ "type": "holding", "address": 0, "count": 1, "decode": decode });

        let plain = call_tool(state.clone(), "read_registers", read(false)).await;
        assert!(
            plain["structuredContent"].get("decoded").is_none(),
            "{plain}"
        );

        let decoded = call_tool(state, "read_registers", read(true)).await;
        let content = &decoded["structuredContent"];
        assert_eq!(content["decoded"][0]["address"], 0, "{decoded}");
        assert_eq!(
            content["decoded"][0]["value"].as_f64(),
            content["values"][0].as_f64()
        );
    }

    #[test]
    fn only_local_origins_are_allowed() {
        let with = |origin: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::ORIGIN, HeaderValue::from_str(origin).unwrap());
            origin_allowed(&headers)
        };
        assert!(origin_allowed(&HeaderMap::new()));
        assert!(with("http://localhost:6274"));
        assert!(with("http://127.0.0.1"));
        assert!(with("http://[::1]:6274"));
        assert!(!with("http://evil.example"));
        assert!(!with("http://localhost.evil.example"));
        assert!(!with("null"));
    }
}

//! Host JSON-RPC 契约冒烟测试。

#![cfg(feature = "host")]

use imgforge::host::{dispatch, HostState, RpcId, RpcRequest};
use serde_json::json;

fn req(method: &str, params: serde_json::Value) -> RpcRequest {
    RpcRequest {
        jsonrpc: "2.0".into(),
        id: Some(RpcId::Number(1)),
        method: method.into(),
        params,
    }
}

#[test]
fn app_ping_and_doctor() {
    let mut state = HostState::new().expect("host state");
    let ping = dispatch(&mut state, req("app.ping", json!({})), None);
    assert!(ping.error.is_none(), "{:?}", ping.error);
    let doctor = dispatch(&mut state, req("app.doctor", json!({})), None);
    assert!(doctor.error.is_none(), "{:?}", doctor.error);
    assert!(doctor.result.is_some());
}

#[test]
fn prefs_roundtrip() {
    let mut state = HostState::new().expect("host state");
    let get = dispatch(&mut state, req("prefs.get", json!({})), None);
    assert!(get.error.is_none(), "{:?}", get.error);
    let prefs = get.result.expect("prefs");
    let set = dispatch(&mut state, req("prefs.set", prefs), None);
    assert!(set.error.is_none(), "{:?}", set.error);
}

#[test]
fn formats_list_nonempty() {
    let mut state = HostState::new().expect("host state");
    let resp = dispatch(&mut state, req("app.formats", json!({})), None);
    assert!(resp.error.is_none(), "{:?}", resp.error);
    let result = resp.result.unwrap();
    let formats = result["formats"].as_array().unwrap();
    assert!(!formats.is_empty());
}

#[test]
fn unknown_method_returns_error() {
    let mut state = HostState::new().expect("host state");
    let resp = dispatch(&mut state, req("no.such.method", json!({})), None);
    assert!(resp.error.is_some());
}

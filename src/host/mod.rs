//! JSON-RPC host：供 Flutter / 外部壳调用的无 UI 控制面。

mod dispatch;
mod protocol;
mod state;

pub use dispatch::dispatch;
pub use protocol::{HostEvent, RpcError, RpcId, RpcRequest, RpcResponse};
pub use state::HostState;

use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};

/// 在 stdin/stdout 上运行 NDJSON JSON-RPC 循环。
pub fn run_stdio() -> eyre::Result<()> {
    let state = Arc::new(Mutex::new(HostState::new()?));
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let reader = BufReader::new(stdin.lock());

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line == "exit" || line == "quit" {
            break;
        }

        let request: RpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                let resp = RpcResponse::error(None, RpcError::parse_error(e.to_string()));
                writeln!(stdout, "{}", serde_json::to_string(&resp)?)?;
                stdout.flush()?;
                continue;
            }
        };

        let event_tx = {
            let out = Arc::new(Mutex::new(std::io::stdout()));
            Some(Arc::new(move |event: HostEvent| {
                let envelope = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "host.event",
                    "params": event,
                });
                if let Ok(mut guard) = out.lock() {
                    let _ = writeln!(guard, "{envelope}");
                    let _ = guard.flush();
                }
            }) as Arc<dyn Fn(HostEvent) + Send + Sync>)
        };

        let response = match state.lock() {
            Ok(mut guard) => dispatch(&mut guard, request, event_tx),
            Err(e) => RpcResponse::error(None, RpcError::internal(format!("host lock: {e}"))),
        };

        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }

    Ok(())
}

//! ImgForge JSON-RPC host（供 Flutter / 外部壳调用）。

fn main() -> eyre::Result<()> {
    color_eyre::install().ok();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    eprintln!(
        "imgforge-host {} — NDJSON JSON-RPC on stdin/stdout",
        env!("CARGO_PKG_VERSION")
    );
    imgforge::host::run_stdio()
}

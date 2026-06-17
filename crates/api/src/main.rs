//! Dashboard API server (T055 analyst queue + T066 simulation control).
//!
//! Serves the analyst review queue (`GET /queue`) and the simulation control plane
//! (`GET /sim/scenarios`, `GET /sim/stream`). Bind address is overridable via `ANALYST_API_ADDR`
//! (default `127.0.0.1:8081`).

use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = env::var("ANALYST_API_ADDR").unwrap_or_else(|_| "127.0.0.1:8081".to_string());
    let app = api::analyst::router(api::analyst::demo_snapshot()).merge(api::sim::router());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("dashboard API on http://{addr} (/queue, /sim/scenarios, /sim/stream)");
    axum::serve(listener, app).await?;
    Ok(())
}

//! Analyst dashboard API server (T055).
//!
//! Serves the risk-prioritised review queue over `GET /queue`. Bind address is overridable via
//! `ANALYST_API_ADDR` (default `127.0.0.1:8081`).

use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = env::var("ANALYST_API_ADDR").unwrap_or_else(|_| "127.0.0.1:8081".to_string());
    let app = api::analyst::router(api::analyst::demo_snapshot());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("analyst API listening on http://{addr}/queue");
    axum::serve(listener, app).await?;
    Ok(())
}

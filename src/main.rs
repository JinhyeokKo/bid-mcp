mod clients;
mod server;

use anyhow::{Context, Result};
use reqwest::Client;
use rmcp::{ServiceExt, transport::stdio};
use server::BidMcpServer;
use std::path::PathBuf;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    let g2b_key = std::env::var("G2B_API_KEY").unwrap_or_default();
    let bizinfo_key = std::env::var("BIZINFO_API_KEY").unwrap_or_default();
    let download_dir = std::env::var("BID_DOWNLOAD_DIR")
        .or_else(|_| std::env::var("G2B_DOWNLOAD_DIR"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./downloads"));

    let http = Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .context("HTTP 클라이언트 초기화 실패")?;

    let service = BidMcpServer::new(http, g2b_key, bizinfo_key, download_dir)
        .serve(stdio())
        .await?;

    service.waiting().await?;
    Ok(())
}
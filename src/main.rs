use anyhow::Context;
use clap::Parser;
use log::info;
use prometheus_client::registry::Registry;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

mod collector;
mod logging;
mod server;

use collector::BeesCollector;
use logging::init_logger;
use server::start_server;

#[derive(Debug, Parser)]
#[command(author, version, about)]
struct Args {
    /// Bees working directory path
    #[arg(short, long, default_value = "/run/bees")]
    pub bees_work_dir: PathBuf,

    /// Port to bind the HTTP server to
    #[arg(short, long, default_value = "8080")]
    pub port: u16,

    /// Address to bind the HTTP server to
    #[arg(short, long, default_value = "::0")]
    pub address: String,

    /// Logging level (error, warn, info, debug, trace)
    #[arg(short, long, default_value = "info")]
    pub log_level: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    init_logger(&args.log_level).context("Failed to initialize logger")?;

    info!("Starting bees prometheus exporter");
    info!("Stats directory: {:?}", args.bees_work_dir);
    info!("Binding to {}:{}", args.address, args.port);

    // Create the BeesCollector
    let collector = BeesCollector::new(args.bees_work_dir)
        .await
        .context("Failed to create BeesCollector")?;

    // Register the collector with the registry
    let mut registry = Registry::default();
    registry.register_collector(Box::new(collector));
    let registry = Arc::new(Mutex::new(registry));

    // Create and start the web server
    start_server(registry, &args.address, args.port).await?;

    Ok(())
}

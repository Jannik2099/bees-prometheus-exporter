use anyhow::Context;
use clap::Parser;
use log::info;
use prometheus_client::registry::Registry;
use std::sync::{Arc, Mutex};

mod collector;
mod config;
mod landlock;
mod logging;
mod server;

use collector::BeesCollector;
use config::Args;
use landlock::init_landlock;
use logging::init_logger;
use server::start_server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    init_logger(&args.log_level).context("Failed to initialize logger")?;
    init_landlock(&args)?;
    info!("Initialized landlock sandbox");

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

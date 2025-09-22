use clap::Parser;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(author, version, about)]
pub struct Args {
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

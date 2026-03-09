use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "locci-proxy",
    version,
    about = "Unified Load Balancer & API Gateway powered by Pingora"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Path to YAML config file
    #[arg(short, long, default_value = "config.yaml")]
    pub config: String,

    /// Override operation mode: lb | gateway | hybrid
    #[arg(long)]
    pub mode: Option<String>,

    /// Override bind address, e.g. 0.0.0.0:8080
    #[arg(long)]
    pub bind: Option<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the proxy (default)
    Start {
        #[arg(short, long)]
        config: Option<String>,
    },
    /// Print current running status via the control API
    Status,
    /// Add a gateway route
    AddRoute {
        name: String,
        path: String,
        upstream: String,
    },
    /// Remove a gateway route
    RemoveRoute { name: String },
    /// Add an upstream group
    AddUpstream { name: String, server: String },
    /// Remove a server from an upstream group
    RemoveUpstream { name: String, server: String },
}

pub fn parse_cli() -> Cli {
    Cli::parse()
}

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::{info, warn, error};

mod config;
mod exchanges;
mod blockchain;
mod arbitrage;
mod models;
mod database;
mod utils;

use crate::config::Config;
use crate::arbitrage::ArbitrageBot;

#[derive(Parser)]
#[command(name = "defi-arbitrage-bot")]
#[command(about = "A DeFi arbitrage bot for cross-chain trading opportunities")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Start {
        #[arg(short, long)]
        config: Option<String>,
        #[arg(short, long, default_value = "false")]
        dry_run: bool,
    },
    Scan {
        #[arg(short, long)]
        pair: Option<String>,
    },
    InitDb,
    Config,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    dotenv::dotenv().ok();

    let cli = Cli::parse();

    match cli.command {
        Commands::Start { config, dry_run } => {
            info!("Starting DeFi Arbitrage Bot");
            let config_path = config.unwrap_or_else(|| "config.toml".to_string());
            let config = Config::load(&config_path)?;
            
            let mut bot = ArbitrageBot::new(config).await?;
            bot.set_dry_run(dry_run);
            
            info!("Bot initialized, starting main loop...");
            bot.run().await?;
        },
        Commands::Scan { pair } => {
            info!("Scanning for arbitrage opportunities");
            let config = Config::load("config.toml")?;
            let bot = ArbitrageBot::new(config).await?;
            
            match pair {
                Some(trading_pair) => {
                    info!("Scanning pair: {}", trading_pair);
                    bot.scan_pair(&trading_pair).await?;
                },
                None => {
                    info!("Scanning all configured pairs");
                    bot.scan_all().await?;
                }
            }
        },
        Commands::InitDb => {
            info!("Initializing database");
            let config = Config::load("config.toml")?;
            database::init_database(&config.database_url).await?;
            info!("Database initialized successfully");
        },
        Commands::Config => {
            info!("Checking configuration");
            let config = Config::load("config.toml")?;
            println!("{:#?}", config);
        }
    }

    Ok(())
}
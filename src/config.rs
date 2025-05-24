use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub database_url: String,
    pub exchanges: HashMap<String, ExchangeConfig>,
    pub blockchain: BlockchainConfig,
    pub trading: TradingConfig,
    pub notifications: Option<NotificationConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExchangeConfig {
    pub name: String,
    pub api_key: String,
    pub api_secret: String,
    pub api_url: String,
    pub websocket_url: Option<String>,
    pub enabled: bool,
    pub trading_pairs: Vec<String>,
    pub min_trade_amount: rust_decimal::Decimal,
    pub max_trade_amount: rust_decimal::Decimal,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BlockchainConfig {
    pub ethereum: ChainConfig,
    pub bsc: ChainConfig,
    pub polygon: ChainConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ChainConfig {
    pub rpc_url: String,
    pub chain_id: u64,
    pub private_key: String,
    pub gas_price_gwei: u64,
    pub max_gas_limit: u64,
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TradingConfig {
    pub min_profit_threshold: rust_decimal::Decimal,
    pub max_slippage: rust_decimal::Decimal,
    pub check_interval_seconds: u64,
    pub max_concurrent_trades: usize,
    pub risk_management: RiskManagement,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RiskManagement {
    pub max_portfolio_exposure: rust_decimal::Decimal,
    pub stop_loss_percentage: rust_decimal::Decimal,
    pub position_size_limit: rust_decimal::Decimal,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NotificationConfig {
    pub telegram: Option<TelegramConfig>,
    pub discord: Option<DiscordConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TelegramConfig {
    pub bot_token: String,
    pub chat_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DiscordConfig {
    pub webhook_url: String,
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let config_str = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&config_str)?;
        
        config.validate()?;
        
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        let enabled_exchanges: Vec<_> = self.exchanges.values()
            .filter(|e| e.enabled)
            .collect();
        
        if enabled_exchanges.is_empty() {
            anyhow::bail!("At least one exchange must be enabled");
        }

        let blockchain_enabled = self.blockchain.ethereum.enabled 
            || self.blockchain.bsc.enabled 
            || self.blockchain.polygon.enabled;
        
        if !blockchain_enabled {
            anyhow::bail!("At least one blockchain must be enabled");
        }

        if self.trading.min_profit_threshold <= rust_decimal::Decimal::ZERO {
            anyhow::bail!("Minimum profit threshold must be positive");
        }

        Ok(())
    }

    pub fn get_enabled_exchanges(&self) -> HashMap<String, &ExchangeConfig> {
        self.exchanges.iter()
            .filter(|(_, config)| config.enabled)
            .collect()
    }
}
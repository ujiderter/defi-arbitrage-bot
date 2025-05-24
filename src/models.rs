use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingPair {
    pub base: String,
    pub quote: String,
    pub symbol: String,
}

impl TradingPair {
    pub fn new(base: &str, quote: &str) -> Self {
        Self {
            base: base.to_uppercase(),
            quote: quote.to_uppercase(),
            symbol: format!("{}/{}", base.to_uppercase(), quote.to_uppercase()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Price {
    pub exchange: String,
    pub pair: TradingPair,
    pub bid: Decimal,
    pub ask: Decimal,
    pub timestamp: DateTime<Utc>,
    pub volume_24h: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBook {
    pub exchange: String,
    pub pair: TradingPair,
    pub bids: Vec<OrderBookLevel>,
    pub asks: Vec<OrderBookLevel>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookLevel {
    pub price: Decimal,
    pub quantity: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbitrageOpportunity {
    pub id: uuid::Uuid,
    pub pair: TradingPair,
    pub buy_exchange: String,
    pub sell_exchange: String,
    pub buy_price: Decimal,
    pub sell_price: Decimal,
    pub profit_percentage: Decimal,
    pub profit_amount: Decimal,
    pub max_trade_size: Decimal,
    pub timestamp: DateTime<Utc>,
    pub status: OpportunityStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OpportunityStatus {
    Active,
    Executed,
    Expired,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub id: uuid::Uuid,
    pub opportunity_id: uuid::Uuid,
    pub exchange: String,
    pub pair: TradingPair,
    pub side: TradeSide,
    pub amount: Decimal,
    pub price: Decimal,
    pub status: TradeStatus,
    pub created_at: DateTime<Utc>,
    pub executed_at: Option<DateTime<Utc>>,
    pub tx_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TradeSide {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TradeStatus {
    Pending,
    Executed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Portfolio {
    pub total_value_usd: Decimal,
    pub balances: std::collections::HashMap<String, Balance>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Balance {
    pub asset: String,
    pub free: Decimal,
    pub locked: Decimal,
    pub total: Decimal,
    pub usd_value: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmartContractCall {
    pub contract_address: String,
    pub function_name: String,
    pub parameters: serde_json::Value,
    pub gas_limit: u64,
    pub gas_price: u64,
    pub chain_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainArbitrage {
    pub source_chain: String,
    pub target_chain: String,
    pub token_address: String,
    pub amount: Decimal,
    pub profit_estimate: Decimal,
    pub bridge_fees: Decimal,
    pub estimated_time_minutes: u32,
}
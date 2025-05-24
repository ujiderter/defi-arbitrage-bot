use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;

pub mod binance;
pub mod uniswap;

use crate::models::{Price, OrderBook, TradingPair, Balance, Trade};

#[async_trait]
pub trait Exchange: Send + Sync {
    fn name(&self) -> &str;
    
    async fn get_price(&self, pair: &TradingPair) -> Result<Price>;
    
    async fn get_order_book(&self, pair: &TradingPair, depth: usize) -> Result<OrderBook>;
    
    async fn get_balances(&self) -> Result<HashMap<String, Balance>>;
    
    async fn place_buy_order(&self, pair: &TradingPair, amount: rust_decimal::Decimal, price: Option<rust_decimal::Decimal>) -> Result<Trade>;
    
    async fn place_sell_order(&self, pair: &TradingPair, amount: rust_decimal::Decimal, price: Option<rust_decimal::Decimal>) -> Result<Trade>;
    
    async fn get_order_status(&self, order_id: &str) -> Result<Trade>;
    
    async fn cancel_order(&self, order_id: &str) -> Result<()>;
    
    fn supports_pair(&self, pair: &TradingPair) -> bool;
    
    async fn get_supported_pairs(&self) -> Result<Vec<TradingPair>>;
    
    async fn get_trading_fees(&self, pair: &TradingPair) -> Result<TradingFees>;
}

#[derive(Debug, Clone)]
pub struct TradingFees {
    pub maker_fee: rust_decimal::Decimal,
    pub taker_fee: rust_decimal::Decimal,
}

pub struct ExchangeManager {
    exchanges: HashMap<String, Box<dyn Exchange>>,
}

impl ExchangeManager {
    pub fn new() -> Self {
        Self {
            exchanges: HashMap::new(),
        }
    }
    
    pub fn add_exchange(&mut self, exchange: Box<dyn Exchange>) {
        let name = exchange.name().to_string();
        self.exchanges.insert(name, exchange);
    }
    
    pub fn get_exchange(&self, name: &str) -> Option<&dyn Exchange> {
        self.exchanges.get(name).map(|e| e.as_ref())
    }
    
    pub fn get_all_exchanges(&self) -> Vec<&dyn Exchange> {
        self.exchanges.values().map(|e| e.as_ref()).collect()
    }
    
    pub async fn get_all_prices(&self, pair: &TradingPair) -> Result<Vec<Price>> {
        let mut prices = Vec::new();
        
        for exchange in self.exchanges.values() {
            if exchange.supports_pair(pair) {
                match exchange.get_price(pair).await {
                    Ok(price) => prices.push(price),
                    Err(err) => {
                        tracing::warn!("Failed to get price from {}: {}", exchange.name(), err);
                    }
                }
            }
        }
        
        Ok(prices)
    }
    
    pub async fn find_best_buy_price(&self, pair: &TradingPair) -> Result<Option<Price>> {
        let prices = self.get_all_prices(pair).await?;
        Ok(prices.into_iter().min_by(|a, b| a.ask.cmp(&b.ask)))
    }
    
    pub async fn find_best_sell_price(&self, pair: &TradingPair) -> Result<Option<Price>> {
        let prices = self.get_all_prices(pair).await?;
        Ok(prices.into_iter().max_by(|a, b| a.bid.cmp(&b.bid)))
    }
}
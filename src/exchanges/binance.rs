use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

use crate::config::ExchangeConfig;
use crate::exchanges::{Exchange, TradingFees};
use crate::models::{Balance, OrderBook, OrderBookLevel, Price, Trade, TradingPair, TradeSide, TradeStatus};

pub struct BinanceExchange {
    config: ExchangeConfig,
    client: Client,
}

#[derive(Debug, Deserialize)]
struct BinanceTicker {
    symbol: String,
    #[serde(rename = "bidPrice")]
    bid_price: String,
    #[serde(rename = "askPrice")]
    ask_price: String,
    volume: String,
}

#[derive(Debug, Deserialize)]
struct BinanceOrderBook {
    bids: Vec<[String; 2]>,
    asks: Vec<[String; 2]>,
}

#[derive(Debug, Deserialize)]
struct BinanceBalance {
    asset: String,
    free: String,
    locked: String,
}

#[derive(Debug, Deserialize)]
struct BinanceAccountInfo {
    balances: Vec<BinanceBalance>,
}

#[derive(Debug, Serialize)]
struct BinanceOrderRequest {
    symbol: String,
    side: String,
    #[serde(rename = "type")]
    order_type: String,
    quantity: String,
    price: Option<String>,
    #[serde(rename = "timeInForce")]
    time_in_force: Option<String>,
    timestamp: u64,
}

#[derive(Debug, Deserialize)]
struct BinanceOrderResponse {
    #[serde(rename = "orderId")]
    order_id: u64,
    symbol: String,
    status: String,
    #[serde(rename = "executedQty")]
    executed_qty: String,
    price: String,
    side: String,
}

impl BinanceExchange {
    pub fn new(config: ExchangeConfig) -> Self {
        Self {
            config,
            client: Client::new(),
        }
    }

    fn create_signature(&self, query_string: &str) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        
        type HmacSha256 = Hmac<Sha256>;
        
        let mut mac = HmacSha256::new_from_slice(self.config.api_secret.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(query_string.as_bytes());
        
        let result = mac.finalize();
        hex::encode(result.into_bytes())
    }

    async fn make_signed_request<T>(&self, endpoint: &str, params: &HashMap<String, String>) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let timestamp = chrono::Utc::now().timestamp_millis();
        let mut query_params = params.clone();
        query_params.insert("timestamp".to_string(), timestamp.to_string());
        
        let query_string = serde_urlencoded::to_string(&query_params)?;
        let signature = self.create_signature(&query_string);
        
        let url = format!("{}{}?{}&signature={}", self.config.api_url, endpoint, query_string, signature);
        
        let response = self.client
            .get(&url)
            .header("X-MBX-APIKEY", &self.config.api_key)
            .send()
            .await?;
        
        if !response.status().is_success() {
            let error_text = response.text().await?;
            anyhow::bail!("Binance API error: {}", error_text);
        }
        
        let result = response.json::<T>().await?;
        Ok(result)
    }

    fn convert_symbol(&self, pair: &TradingPair) -> String {
        format!("{}{}", pair.base, pair.quote)
    }
}

#[async_trait]
impl Exchange for BinanceExchange {
    fn name(&self) -> &str {
        "binance"
    }

    async fn get_price(&self, pair: &TradingPair) -> Result<Price> {
        let symbol = self.convert_symbol(pair);
        let url = format!("{}/api/v3/ticker/bookTicker?symbol={}", self.config.api_url, symbol);
        
        let response = self.client.get(&url).send().await?;
        let ticker: BinanceTicker = response.json().await?;
        
        Ok(Price {
            exchange: self.name().to_string(),
            pair: pair.clone(),
            bid: Decimal::from_str(&ticker.bid_price)?,
            ask: Decimal::from_str(&ticker.ask_price)?,
            timestamp: Utc::now(),
            volume_24h: Some(Decimal::from_str(&ticker.volume)?),
        })
    }

    async fn get_order_book(&self, pair: &TradingPair, depth: usize) -> Result<OrderBook> {
        let symbol = self.convert_symbol(pair);
        let url = format!("{}/api/v3/depth?symbol={}&limit={}", self.config.api_url, symbol, depth);
        
        let response = self.client.get(&url).send().await?;
        let order_book: BinanceOrderBook = response.json().await?;
        
        let bids = order_book.bids.iter()
            .map(|level| OrderBookLevel {
                price: Decimal::from_str(&level[0]).unwrap_or_default(),
                quantity: Decimal::from_str(&level[1]).unwrap_or_default(),
            })
            .collect();
            
        let asks = order_book.asks.iter()
            .map(|level| OrderBookLevel {
                price: Decimal::from_str(&level[0]).unwrap_or_default(),
                quantity: Decimal::from_str(&level[1]).unwrap_or_default(),
            })
            .collect();
        
        Ok(OrderBook {
            exchange: self.name().to_string(),
            pair: pair.clone(),
            bids,
            asks,
            timestamp: Utc::now(),
        })
    }

    async fn get_balances(&self) -> Result<HashMap<String, Balance>> {
        let params = HashMap::new();
        let account_info: BinanceAccountInfo = self.make_signed_request("/api/v3/account", &params).await?;
        
        let mut balances = HashMap::new();
        
        for balance in account_info.balances {
            let free = Decimal::from_str(&balance.free).unwrap_or_default();
            let locked = Decimal::from_str(&balance.locked).unwrap_or_default();
            let total = free + locked;
            
            if total > Decimal::ZERO {
                balances.insert(balance.asset.clone(), Balance {
                    asset: balance.asset,
                    free,
                    locked,
                    total,
                    usd_value: Decimal::ZERO,
                });
            }
        }
        
        Ok(balances)
    }

    async fn place_buy_order(&self, pair: &TradingPair, amount: Decimal, price: Option<Decimal>) -> Result<Trade> {
        todo!("Implement buy order placement")
    }

    async fn place_sell_order(&self, pair: &TradingPair, amount: Decimal, price: Option<Decimal>) -> Result<Trade> {
        todo!("Implement sell order placement")
    }

    async fn get_order_status(&self, order_id: &str) -> Result<Trade> {
        todo!("Implement order status check")
    }

    async fn cancel_order(&self, order_id: &str) -> Result<()> {
        todo!("Implement order cancellation")
    }

    fn supports_pair(&self, pair: &TradingPair) -> bool {
        self.config.trading_pairs.contains(&pair.symbol)
    }

    async fn get_supported_pairs(&self) -> Result<Vec<TradingPair>> {
        let pairs = self.config.trading_pairs.iter()
            .filter_map(|symbol| {
                let parts: Vec<&str> = symbol.split('/').collect();
                if parts.len() == 2 {
                    Some(TradingPair::new(parts[0], parts[1]))
                } else {
                    None
                }
            })
            .collect();
        
        Ok(pairs)
    }

    async fn get_trading_fees(&self, _pair: &TradingPair) -> Result<TradingFees> {
        Ok(TradingFees {
            maker_fee: Decimal::from_str("0.001")?,
            taker_fee: Decimal::from_str("0.001")?,
        })
    }
}
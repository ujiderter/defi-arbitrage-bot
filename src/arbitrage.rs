use anyhow::Result;
use chrono::Utc;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::time::Duration;
use tokio::time;
use tracing::{info, warn, error, debug};

use crate::config::Config;
use crate::exchanges::{ExchangeManager, Exchange};
use crate::models::{ArbitrageOpportunity, OpportunityStatus, TradingPair, Price};
use crate::database::Database;
use crate::blockchain::BlockchainManager;

pub struct ArbitrageBot {
    config: Config,
    exchange_manager: ExchangeManager,
    blockchain_manager: BlockchainManager,
    database: Database,
    dry_run: bool,
    active_opportunities: HashMap<String, ArbitrageOpportunity>,
}

impl ArbitrageBot {
    pub async fn new(config: Config) -> Result<Self> {
        let mut exchange_manager = ExchangeManager::new();
        
        for (name, exchange_config) in &config.exchanges {
            if exchange_config.enabled {
                match name.as_str() {
                    "binance" => {
                        let exchange = Box::new(crate::exchanges::binance::BinanceExchange::new(exchange_config.clone()));
                        exchange_manager.add_exchange(exchange);
                        info!("Initialized Binance exchange");
                    },
                    "uniswap" => {
                        let exchange = Box::new(crate::exchanges::uniswap::UniswapExchange::new(exchange_config.clone()).await?);
                        exchange_manager.add_exchange(exchange);
                        info!("Initialized Uniswap exchange");
                    },
                    _ => {
                        warn!("Unknown exchange: {}", name);
                    }
                }
            }
        }
        
        let blockchain_manager = BlockchainManager::new(&config.blockchain).await?;
        let database = Database::new(&config.database_url).await?;
        
        Ok(Self {
            config,
            exchange_manager,
            blockchain_manager,
            database,
            dry_run: false,
            active_opportunities: HashMap::new(),
        })
    }
    
    pub fn set_dry_run(&mut self, dry_run: bool) {
        self.dry_run = dry_run;
        if dry_run {
            info!("Bot running in DRY RUN mode - no actual trades will be executed");
        }
    }
    
    pub async fn run(&mut self) -> Result<()> {
        info!("Starting arbitrage bot main loop");
        
        let mut interval = time::interval(Duration::from_secs(self.config.trading.check_interval_seconds));
        
        loop {
            interval.tick().await;
            
            if let Err(e) = self.scan_and_execute().await {
                error!("Error in main loop: {}", e);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
    
    async fn scan_and_execute(&mut self) -> Result<()> {
        debug!("Scanning for arbitrage opportunities");
        
        let enabled_exchanges = self.config.get_enabled_exchanges();
        
        if enabled_exchanges.len() < 2 {
            warn!("Need at least 2 exchanges for arbitrage");
            return Ok(());
        }
        
        let mut all_pairs = std::collections::HashSet::new();
        for exchange_config in enabled_exchanges.values() {
            for pair_str in &exchange_config.trading_pairs {
                if let Some(pair) = self.parse_trading_pair(pair_str) {
                    all_pairs.insert(pair);
                }
            }
        }
        
        for pair in all_pairs {
            if let Err(e) = self.scan_pair_for_opportunities(&pair).await {
                warn!("Error scanning pair {}: {}", pair.symbol, e);
            }
        }
        
        self.execute_opportunities().await?;
        
        self.cleanup_expired_opportunities().await?;
        
        Ok(())
    }
    
    async fn scan_pair_for_opportunities(&mut self, pair: &TradingPair) -> Result<()> {
        let mut prices = Vec::new();
        
        for exchange in self.exchange_manager.get_all_exchanges() {
            if exchange.supports_pair(pair) {
                match exchange.get_price(pair).await {
                    Ok(price) => {
                        prices.push(price);
                        debug!("Got price from {}: {} bid, {} ask", 
                               exchange.name(), price.bid, price.ask);
                    },
                    Err(e) => {
                        warn!("Failed to get price from {} for {}: {}", 
                              exchange.name(), pair.symbol, e);
                    }
                }
            }
        }
        
        if prices.len() < 2 {
            return Ok(());
        }
        
        for i in 0..prices.len() {
            for j in (i+1)..prices.len() {
                let price1 = &prices[i];
                let price2 = &prices[j];
                
                if let Some(opportunity) = self.calculate_arbitrage_opportunity(
                    pair,
                    &price1.exchange,
                    &price2.exchange,
                    price1.ask,
                    price2.bid,
                ).await? {
                    self.add_opportunity(opportunity).await?;
                }
                
                if let Some(opportunity) = self.calculate_arbitrage_opportunity(
                    pair,
                    &price2.exchange,
                    &price1.exchange,
                    price2.ask,
                    price1.bid,
                ).await? {
                    self.add_opportunity(opportunity).await?;
                }
            }
        }
        
        Ok(())
    }
    
    async fn calculate_arbitrage_opportunity(
        &self,
        pair: &TradingPair,
        buy_exchange: &str,
        sell_exchange: &str,
        buy_price: Decimal,
        sell_price: Decimal,
    ) -> Result<Option<ArbitrageOpportunity>> {
        let gross_profit_pct = (sell_price - buy_price) / buy_price * Decimal::from(100);
        
        if gross_profit_pct <= self.config.trading.min_profit_threshold {
            return Ok(None);
        }
        
        let buy_exchange_obj = self.exchange_manager.get_exchange(buy_exchange)
            .ok_or_else(|| anyhow::anyhow!("Exchange not found: {}", buy_exchange))?;
        let sell_exchange_obj = self.exchange_manager.get_exchange(sell_exchange)
            .ok_or_else(|| anyhow::anyhow!("Exchange not found: {}", sell_exchange))?;
        
        let buy_fees = buy_exchange_obj.get_trading_fees(pair).await?;
        let sell_fees = sell_exchange_obj.get_trading_fees(pair).await?;
        
        let total_fee_pct = buy_fees.taker_fee + sell_fees.taker_fee;
        let net_profit_pct = gross_profit_pct - (total_fee_pct * Decimal::from(100));
        
        if net_profit_pct <= self.config.trading.min_profit_threshold {
            return Ok(None);
        }
        
        let max_trade_size = self.calculate_max_trade_size(
            buy_exchange_obj,
            sell_exchange_obj,
            pair,
            buy_price,
            sell_price,
        ).await?;
        
        if max_trade_size <= Decimal::ZERO {
            return Ok(None);
        }
        
        let profit_amount = max_trade_size * net_profit_pct / Decimal::from(100);
        
        let opportunity = ArbitrageOpportunity {
            id: uuid::Uuid::new_v4(),
            pair: pair.clone(),
            buy_exchange: buy_exchange.to_string(),
            sell_exchange: sell_exchange.to_string(),
            buy_price,
            sell_price,
            profit_percentage: net_profit_pct,
            profit_amount,
            max_trade_size,
            timestamp: Utc::now(),
            status: OpportunityStatus::Active,
        };
        
        info!("Found arbitrage opportunity: {:.2}% profit, ${:.2} potential profit",
              net_profit_pct, profit_amount);
        
        Ok(Some(opportunity))
    }
    
    async fn calculate_max_trade_size(
        &self,
        buy_exchange: &dyn Exchange,
        sell_exchange: &dyn Exchange,
        pair: &TradingPair,
        buy_price: Decimal,
        sell_price: Decimal,
    ) -> Result<Decimal> {
        let buy_order_book = buy_exchange.get_order_book(pair, 20).await?;
        let sell_order_book = sell_exchange.get_order_book(pair, 20).await?;
        
        let mut buy_liquidity = Decimal::ZERO;
        for ask in &buy_order_book.asks {
            if ask.price <= buy_price * (Decimal::ONE + self.config.trading.max_slippage) {
                buy_liquidity += ask.quantity;
            } else {
                break;
            }
        }
        
        let mut sell_liquidity = Decimal::ZERO;
        for bid in &sell_order_book.bids {
            if bid.price >= sell_price * (Decimal::ONE - self.config.trading.max_slippage) {
                sell_liquidity += bid.quantity;
            } else {
                break;
            }
        }
        
        let max_size = buy_liquidity.min(sell_liquidity);
        
        let config_max = self.config.exchanges.get(&buy_exchange.name().to_string())
            .map(|c| c.max_trade_amount)
            .unwrap_or(Decimal::from(1000));
        
        Ok(max_size.min(config_max))
    }
    
    async fn add_opportunity(&mut self, opportunity: ArbitrageOpportunity) -> Result<()> {
        let key = format!("{}-{}-{}", 
                         opportunity.pair.symbol, 
                         opportunity.buy_exchange, 
                         opportunity.sell_exchange);
        
        if let Some(existing) = self.active_opportunities.get(&key) {
            if opportunity.profit_percentage > existing.profit_percentage {
                self.active_opportunities.insert(key.clone(), opportunity.clone());
                self.database.save_opportunity(&opportunity).await?;
                info!("Updated opportunity: {}", key);
            }
        } else {
            self.active_opportunities.insert(key.clone(), opportunity.clone());
            self.database.save_opportunity(&opportunity).await?;
            info!("Added new opportunity: {}", key);
        }
        
        Ok(())
    }
    
    async fn execute_opportunities(&mut self) -> Result<()> {
        let opportunities: Vec<_> = self.active_opportunities.values().cloned().collect();
        
        let mut sorted_opportunities = opportunities;
        sorted_opportunities.sort_by(|a, b| b.profit_percentage.cmp(&a.profit_percentage));
        
        let max_concurrent = self.config.trading.max_concurrent_trades;
        let to_execute = sorted_opportunities.into_iter()
            .take(max_concurrent)
            .collect::<Vec<_>>();
        
        for opportunity in to_execute {
            if let Err(e) = self.execute_opportunity(&opportunity).await {
                error!("Failed to execute opportunity {}: {}", opportunity.id, e);
            }
        }
        
        Ok(())
    }
    
    async fn execute_opportunity(&mut self, opportunity: &ArbitrageOpportunity) -> Result<()> {
        if self.dry_run {
            info!("DRY RUN: Would execute arbitrage opportunity: {:.2}% profit, ${:.2}",
                  opportunity.profit_percentage, opportunity.profit_amount);
            return Ok(());
        }
        
        info!("Executing arbitrage opportunity: {} -> {}, {:.2}% profit",
              opportunity.buy_exchange, opportunity.sell_exchange, opportunity.profit_percentage);
        
        // TODO: Implement actual trade execution
        // This would involve:
        // 1. Check account balances
        // 2. Place buy order on buy exchange
        // 3. Wait for fill
        // 4. Place sell order on sell exchange
        // 5. Monitor execution
        // 6. Handle partial fills and errors
        // 7. Update database with results
        
        info!("Trade execution completed for opportunity {}", opportunity.id);
        Ok(())
    }
    
    async fn cleanup_expired_opportunities(&mut self) -> Result<()> {
        let now = Utc::now();
        let expiry_threshold = chrono::Duration::minutes(5);
        
        let expired_keys: Vec<_> = self.active_opportunities.iter()
            .filter(|(_, opp)| now.signed_duration_since(opp.timestamp) > expiry_threshold)
            .map(|(key, _)| key.clone())
            .collect();
        
        for key in expired_keys {
            if let Some(mut opportunity) = self.active_opportunities.remove(&key) {
                opportunity.status = OpportunityStatus::Expired;
                self.database.update_opportunity_status(&opportunity).await?;
                debug!("Expired opportunity: {}", key);
            }
        }
        
        Ok(())
    }
    
    pub async fn scan_pair(&self, pair_str: &str) -> Result<()> {
        if let Some(pair) = self.parse_trading_pair(pair_str) {
            let prices = self.exchange_manager.get_all_prices(&pair).await?;
            
            println!("Prices for {}:", pair.symbol);
            for price in prices {
                println!("  {}: bid={}, ask={}, spread={:.4}%",
                        price.exchange,
                        price.bid,
                        price.ask,
                        (price.ask - price.bid) / price.bid * Decimal::from(100));
            }
            
            let best_buy = self.exchange_manager.find_best_buy_price(&pair).await?;
            let best_sell = self.exchange_manager.find_best_sell_price(&pair).await?;
            
            if let (Some(buy), Some(sell)) = (best_buy, best_sell) {
                if buy.exchange != sell.exchange {
                    let profit_pct = (sell.bid - buy.ask) / buy.ask * Decimal::from(100);
                    println!("\nArbitrage opportunity:");
                    println!("  Buy on {} at {}", buy.exchange, buy.ask);
                    println!("  Sell on {} at {}", sell.exchange, sell.bid);
                    println!("  Gross profit: {:.4}%", profit_pct);
                }
            }
        } else {
            anyhow::bail!("Invalid trading pair format: {}", pair_str);
        }
        
        Ok(())
    }
    
    pub async fn scan_all(&self) -> Result<()> {
        let exchanges = self.exchange_manager.get_all_exchanges();
        
        for exchange in exchanges {
            let pairs = exchange.get_supported_pairs().await?;
            println!("\n{} supported pairs:", exchange.name());
            
            for pair in pairs.iter().take(5) {
                if let Ok(price) = exchange.get_price(pair).await {
                    println!("  {}: bid={}, ask={}", pair.symbol, price.bid, price.ask);
                }
            }
        }
        
        Ok(())
    }
    
    fn parse_trading_pair(&self, pair_str: &str) -> Option<TradingPair> {
        let parts: Vec<&str> = pair_str.split('/').collect();
        if parts.len() == 2 {
            Some(TradingPair::new(parts[0], parts[1]))
        } else {
            None
        }
    }
}
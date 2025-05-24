use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use ethers::prelude::*;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use crate::config::ExchangeConfig;
use crate::exchanges::{Exchange, TradingFees};
use crate::models::{Balance, OrderBook, OrderBookLevel, Price, Trade, TradingPair, TradeSide, TradeStatus};

pub struct UniswapExchange {
    config: ExchangeConfig,
    provider: Arc<Provider<Http>>,
    wallet: Option<LocalWallet>,
}

abigen!(
    UniswapV2Router,
    r#"[
        function getAmountsOut(uint amountIn, address[] calldata path) external view returns (uint[] memory amounts)
        function getAmountsIn(uint amountOut, address[] calldata path) external view returns (uint[] memory amounts)
        function swapExactTokensForTokens(uint amountIn, uint amountOutMin, address[] calldata path, address to, uint deadline) external returns (uint[] memory amounts)
        function swapTokensForExactTokens(uint amountOut, uint amountInMax, address[] calldata path, address to, uint deadline) external returns (uint[] memory amounts)
    ]"#
);

// ERC20 Token ABI
abigen!(
    ERC20,
    r#"[
        function balanceOf(address owner) external view returns (uint256)
        function decimals() external view returns (uint8)
        function symbol() external view returns (string)
        function approve(address spender, uint256 amount) external returns (bool)
    ]"#
);

impl UniswapExchange {
    pub async fn new(config: ExchangeConfig) -> Result<Self> {
        let provider = Provider::<Http>::try_from(&config.api_url)?;
        let provider = Arc::new(provider);
        
        // Initialize wallet if private key is provided
        let wallet = if !config.api_secret.is_empty() {
            Some(config.api_secret.parse::<LocalWallet>()?)
        } else {
            None
        };
        
        Ok(Self {
            config,
            provider,
            wallet,
        })
    }
    
    fn get_token_address(&self, symbol: &str) -> Option<Address> {
        match symbol.to_uppercase().as_str() {
            "USDC" => Some("0xA0b86a33E6441e5C46EE5F395f4c0C2D45C41B1A".parse().ok()?),
            "USDT" => Some("0xdAC17F958D2ee523a2206206994597C13D831ec7".parse().ok()?),
            "DAI" => Some("0x6B175474E89094C44Da98b954EedeAC495271d0F".parse().ok()?),
            "WETH" => Some("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".parse().ok()?),
            "WBTC" => Some("0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599".parse().ok()?),
            _ => None,
        }
    }
    
    async fn get_token_decimals(&self, token_address: Address) -> Result<u8> {
        let token = ERC20::new(token_address, self.provider.clone());
        let decimals = token.decimals().call().await?;
        Ok(decimals)
    }
    
    async fn get_amounts_out(&self, amount_in: U256, path: Vec<Address>) -> Result<Vec<U256>> {
        let router_address: Address = "0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D".parse()?;
        let router = UniswapV2Router::new(router_address, self.provider.clone());
        
        let amounts = router.get_amounts_out(amount_in, path).call().await?;
        Ok(amounts)
    }
    
    async fn get_amounts_in(&self, amount_out: U256, path: Vec<Address>) -> Result<Vec<U256>> {
        let router_address: Address = "0x7a250d5630B4cF539739dF2C5dAcb4c659F2488D".parse()?;
        let router = UniswapV2Router::new(router_address, self.provider.clone());
        
        let amounts = router.get_amounts_in(amount_out, path).call().await?;
        Ok(amounts)
    }
}

#[async_trait]
impl Exchange for UniswapExchange {
    fn name(&self) -> &str {
        "uniswap"
    }

    async fn get_price(&self, pair: &TradingPair) -> Result<Price> {
        let base_address = self.get_token_address(&pair.base)
            .ok_or_else(|| anyhow::anyhow!("Token not supported: {}", pair.base))?;
        let quote_address = self.get_token_address(&pair.quote)
            .ok_or_else(|| anyhow::anyhow!("Token not supported: {}", pair.quote))?;
        
        let base_decimals = self.get_token_decimals(base_address).await?;
        let quote_decimals = self.get_token_decimals(quote_address).await?;
        
        let one_unit = U256::from(10_u64.pow(base_decimals as u32));
        
        let path = vec![base_address, quote_address];
        let amounts_out = self.get_amounts_out(one_unit, path.clone()).await?;
        
        if amounts_out.len() < 2 {
            anyhow::bail!("Invalid amounts returned from Uniswap");
        }
        
        let quote_amount = amounts_out[1];
        let ask_price = Decimal::from_str(&quote_amount.to_string())?
            / Decimal::from(10_u64.pow(quote_decimals as u32));
        
        let bid_price = ask_price * Decimal::from_str("0.997")?;
        
        Ok(Price {
            exchange: self.name().to_string(),
            pair: pair.clone(),
            bid: bid_price,
            ask: ask_price,
            timestamp: Utc::now(),
            volume_24h: None,
        })
    }

    async fn get_order_book(&self, pair: &TradingPair, depth: usize) -> Result<OrderBook> {
        let base_address = self.get_token_address(&pair.base)
            .ok_or_else(|| anyhow::anyhow!("Token not supported: {}", pair.base))?;
        let quote_address = self.get_token_address(&pair.quote)
            .ok_or_else(|| anyhow::anyhow!("Token not supported: {}", pair.quote))?;
        
        let base_decimals = self.get_token_decimals(base_address).await?;
        let quote_decimals = self.get_token_decimals(quote_address).await?;
        let path = vec![base_address, quote_address];
        
        let mut asks = Vec::new();
        let mut bids = Vec::new();
        
        for i in 1..=depth {
            let quantity = Decimal::from(i) * Decimal::from(100);
            let quantity_wei = U256::from_dec_str(&(quantity * Decimal::from(10_u64.pow(base_decimals as u32))).to_string())?;
            
            if let Ok(amounts_out) = self.get_amounts_out(quantity_wei, path.clone()).await {
                if amounts_out.len() >= 2 {
                    let quote_amount = Decimal::from_str(&amounts_out[1].to_string())?
                        / Decimal::from(10_u64.pow(quote_decimals as u32));
                    let price = quote_amount / quantity;
                    
                    asks.push(OrderBookLevel {
                        price,
                        quantity,
                    });
                    
                    bids.push(OrderBookLevel {
                        price: price * Decimal::from_str("0.997")?,
                        quantity,
                    });
                }
            }
        }
        
        Ok(OrderBook {
            exchange: self.name().to_string(),
            pair: pair.clone(),
            bids,
            asks,
            timestamp: Utc::now(),
        })
    }

    async fn get_balances(&self) -> Result<HashMap<String, Balance>> {
        let mut balances = HashMap::new();
        
        if let Some(wallet) = &self.wallet {
            let eth_balance = self.provider.get_balance(wallet.address(), None).await?;
            let eth_balance_decimal = Decimal::from_str(&eth_balance.to_string())?
                / Decimal::from(10_u64.pow(18)); // ETH has 18 decimals
            
            if eth_balance_decimal > Decimal::ZERO {
                balances.insert("ETH".to_string(), Balance {
                    asset: "ETH".to_string(),
                    free: eth_balance_decimal,
                    locked: Decimal::ZERO,
                    total: eth_balance_decimal,
                    usd_value: Decimal::ZERO,
                });
            }
            
            for pair_str in &self.config.trading_pairs {
                if let Some(pair) = self.parse_trading_pair(pair_str) {
                    for symbol in [&pair.base, &pair.quote] {
                        if let Some(token_address) = self.get_token_address(symbol) {
                            let token = ERC20::new(token_address, self.provider.clone());
                            if let Ok(balance) = token.balance_of(wallet.address()).call().await {
                                let decimals = self.get_token_decimals(token_address).await?;
                                let balance_decimal = Decimal::from_str(&balance.to_string())?
                                    / Decimal::from(10_u64.pow(decimals as u32));
                                
                                if balance_decimal > Decimal::ZERO {
                                    balances.insert(symbol.clone(), Balance {
                                        asset: symbol.clone(),
                                        free: balance_decimal,
                                        locked: Decimal::ZERO,
                                        total: balance_decimal,
                                        usd_value: Decimal::ZERO,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Ok(balances)
    }

    async fn place_buy_order(&self, pair: &TradingPair, amount: Decimal, price: Option<Decimal>) -> Result<Trade> {
        todo!("Implement Uniswap buy order (swap)")
    }

    async fn place_sell_order(&self, pair: &TradingPair, amount: Decimal, price: Option<Decimal>) -> Result<Trade> {
        todo!("Implement Uniswap sell order (swap)")
    }

    async fn get_order_status(&self, order_id: &str) -> Result<Trade> {
        todo!("Implement transaction status check")
    }

    async fn cancel_order(&self, order_id: &str) -> Result<()> {
        anyhow::bail!("Uniswap transactions cannot be cancelled")
    }

    fn supports_pair(&self, pair: &TradingPair) -> bool {
        self.get_token_address(&pair.base).is_some() && 
        self.get_token_address(&pair.quote).is_some()
    }

    async fn get_supported_pairs(&self) -> Result<Vec<TradingPair>> {
        let pairs = self.config.trading_pairs.iter()
            .filter_map(|symbol| self.parse_trading_pair(symbol))
            .filter(|pair| self.supports_pair(pair))
            .collect();
        
        Ok(pairs)
    }

    async fn get_trading_fees(&self, _pair: &TradingPair) -> Result<TradingFees> {
        Ok(TradingFees {
            maker_fee: Decimal::from_str("0.003")?,
            taker_fee: Decimal::from_str("0.003")?,
        })
    }
}

impl UniswapExchange {
    fn parse_trading_pair(&self, pair_str: &str) -> Option<TradingPair> {
        let parts: Vec<&str> = pair_str.split('/').collect();
        if parts.len() == 2 {
            Some(TradingPair::new(parts[0], parts[1]))
        } else {
            None
        }
    }
}
//! Data sources for fetching on-chain and off-chain token information.
//!
//! This module handles all external data fetching including RPC calls,
//! API requests, and metadata retrieval with retry logic and caching.

use crate::oracle::types::{OracleConfig}; // Use new OracleConfig from types.rs
// Import token data types from types_old.rs where they're actually defined
use crate::oracle::types_old::{
    TokenData, Metadata, HolderData, LiquidityPool, VolumeData, CreatorHoldings,
    SocialActivity, PoolType, Attribute,
};
use crate::types::{PremintCandidate, Pubkey};
use anyhow::{anyhow, Context, Result};
use chrono::Timelike; // For .hour() method
use reqwest::Client;
// Temporarily commented out due to Solana dependency issues
// TODO: Restore when Solana dependencies are properly configured
// use solana_client::nonblocking::rpc_client::RpcClient;
// use solana_sdk::pubkey::Pubkey;

// Placeholder RpcClient for now - will be replaced with actual Solana RPC client
#[derive(Debug)]
pub struct RpcClient;

impl RpcClient {
    pub fn new(_endpoint: &str) -> Self {
        Self
    }
}
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio_retry::{strategy::ExponentialBackoff, Retry};
use tracing::{debug, warn, error, instrument};

/// Data source manager for fetching token information.
pub struct OracleDataSources {
    rpc_clients: Vec<Arc<RpcClient>>,
    http_client: Client,
    config: OracleConfig,
}

impl OracleDataSources {
    /// Create a new data sources manager.
    pub fn new(
        rpc_clients: Vec<Arc<RpcClient>>,
        http_client: Client,
        config: OracleConfig,
    ) -> Self {
        Self {
            rpc_clients,
            http_client,
            config,
        }
    }

    /// Fetch complete token data with retries.
    #[instrument(skip(self), fields(mint = %candidate.mint))]
    pub async fn fetch_token_data_with_retries(
        &self,
        candidate: &PremintCandidate,
    ) -> Result<TokenData> {
        let retry_strategy = ExponentialBackoff::from_millis(100)
            .max_delay(Duration::from_secs(5))
            .take(self.config.rpc_retry_attempts);

        Retry::spawn(retry_strategy, || self.fetch_token_data(candidate)).await
    }

    /// Fetch complete token data from multiple sources.
    #[instrument(skip(self), fields(mint = %candidate.mint))]
    async fn fetch_token_data(&self, candidate: &PremintCandidate) -> Result<TokenData> {
        // Use first available RPC client
        let rpc = self.rpc_clients
            .first()
            .ok_or_else(|| anyhow!("No RPC clients available"))?;

        // Fetch basic token information
        let token_supply = self.fetch_token_supply(candidate, rpc).await?;
        let (supply, decimals) = token_supply;

        // Fetch metadata URI and content
        let metadata_uri = self.resolve_metadata_uri(&candidate.mint, rpc).await
            .unwrap_or_else(|_| "".to_string());
        
        let metadata = if !metadata_uri.is_empty() {
            self.fetch_metadata_from_uri(&metadata_uri).await.ok()
        } else {
            None
        };

        // Fetch holder distribution
        let holder_distribution = self.fetch_holder_distribution(candidate, rpc).await
            .unwrap_or_default();

        // Fetch liquidity information
        let liquidity_pool = self.fetch_liquidity_data(candidate, rpc).await
            .unwrap_or(None);

        // Fetch volume and transaction data
        let volume_data = self.fetch_volume_data(candidate, rpc).await
            .unwrap_or_default();

        // Fetch creator holdings and sell activity
        let creator_holdings = self.fetch_creator_holdings(candidate, &()).await
            .unwrap_or_default();

        // Fetch social activity (if API keys available)
        let social_activity = self.fetch_social_activity(candidate).await
            .unwrap_or_default();

        // Create holder and price history with current data
        let mut holder_history = VecDeque::new();
        holder_history.push_back(holder_distribution.len());

        let mut price_history = VecDeque::new();
        if let Some(pool) = &liquidity_pool {
            let price = pool.sol_amount / (pool.token_amount / 10f64.powi(decimals as i32));
            price_history.push_back(price);
        }

        let token_data = TokenData {
            supply,
            decimals,
            metadata_uri,
            metadata,
            holder_distribution,
            liquidity_pool,
            volume_data,
            creator_holdings,
            holder_history,
            price_history,
            social_activity,
        };

        debug!("Fetched complete token data for {}", candidate.mint);
        Ok(token_data)
    }

    /// Fetch token supply and decimals.
    #[instrument(skip(self), fields(mint = %candidate.mint))]
    async fn fetch_token_supply(
        &self,
        candidate: &PremintCandidate,
        __rpc: &RpcClient,
    ) -> Result<(u64, u8)> {
        debug!("Fetching token supply and decimals");
        
        // Placeholder implementation - would use actual RPC calls
        // let mint_account = rpc.get_account(&candidate.mint).await?;
        
        Ok((1_000_000_000, 9)) // 1B supply, 9 decimals
    }

    /// Resolve metadata URI from mint account.
    #[instrument(skip(self), fields(mint = %mint_address))]
    async fn resolve_metadata_uri(
        &self,
        mint_address: &Pubkey,
        __rpc: &RpcClient,
    ) -> Result<String> {
        debug!("Resolving metadata URI for {}", mint_address);
        
        // Placeholder implementation
        Ok(format!("https://example.com/metadata/{}.json", mint_address))
    }

    /// Fetch metadata from URI.
    #[instrument(skip(self), fields(uri = %uri))]
    async fn fetch_metadata_from_uri(&self, uri: &str) -> Result<Metadata> {
        let response = self
            .http_client
            .get(uri)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .context("Failed to fetch metadata")?;

        if !response.status().is_success() {
            return Err(anyhow!("Failed to fetch metadata: {}", response.status()));
        }

        let metadata: Metadata = response
            .json()
            .await
            .context("Failed to parse metadata")?;

        debug!("Fetched metadata: {}", metadata.name);
        Ok(metadata)
    }

    /// Fetch holder distribution data.
    #[instrument(skip(self), fields(mint = %candidate.mint))]
    async fn fetch_holder_distribution(
        &self,
        candidate: &PremintCandidate,
        __rpc: &RpcClient,
    ) -> Result<Vec<HolderData>> {
        debug!("Fetching token holder distribution");
        
        // Placeholder implementation - would use actual RPC calls
        let holders = vec![
            HolderData {
                address: "Creator".to_string(),
                percentage: 15.0,
                is_whale: true,
            },
            HolderData {
                address: "LargeHolder".to_string(),
                percentage: 8.0,
                is_whale: true,
            },
            HolderData {
                address: "MediumHolder1".to_string(),
                percentage: 3.5,
                is_whale: false,
            },
        ];

        debug!("Fetched {} holders", holders.len());
        Ok(holders)
    }

    /// Fetch liquidity pool data.
    #[instrument(skip(self), fields(mint = %candidate.mint))]
    async fn fetch_liquidity_data(
        &self,
        candidate: &PremintCandidate,
        _rpc: &RpcClient,
    ) -> Result<Option<LiquidityPool>> {
        // Try to find Raydium pools first
        if let Ok(raydium_pools) = self.find_raydium_pools(candidate, _rpc).await {
            if let Some(pool) = raydium_pools.first() {
                return Ok(Some(pool.clone()));
            }
        }

        // Try Pump.fun pools
        if let Ok(pump_pool) = self.find_pump_fun_pool(candidate, _rpc).await {
            if pump_pool.is_some() {
                return Ok(pump_pool);
            }
        }

        // Try Orca pools
        if let Ok(orca_pool) = self.find_orca_pools(candidate, _rpc).await {
            if orca_pool.is_some() {
                return Ok(orca_pool);
            }
        }

        debug!("No liquidity pools found");
        Ok(None)
    }

    /// Find Raydium liquidity pools.
    #[instrument(skip(self), fields(mint = %candidate.mint))]
    async fn find_raydium_pools(
        &self,
        candidate: &PremintCandidate,
        _rpc: &RpcClient,
    ) -> Result<Vec<LiquidityPool>> {
        // Simplified implementation - would need proper Raydium pool scanning
        debug!("Searching for Raydium pools");
        
        // For now, return empty - real implementation would scan for AMM pools
        Ok(vec![])
    }

    /// Find Pump.fun pool.
    #[instrument(skip(self), fields(mint = %candidate.mint))]
    async fn find_pump_fun_pool(
        &self,
        candidate: &PremintCandidate,
        _rpc: &RpcClient,
    ) -> Result<Option<LiquidityPool>> {
        debug!("Searching for Pump.fun pool");
        
        // Simplified implementation
        // Would use Pump.fun API if key is available
        if self.config.pump_fun_api_key.is_some() {
            // Mock pool for demonstration
            Ok(Some(LiquidityPool {
                sol_amount: 25.0,
                token_amount: 1000000.0,
                pool_address: "MockPumpFunPool".to_string(),
                pool_type: PoolType::PumpFun,
            }))
        } else {
            Ok(None)
        }
    }

    /// Find Orca pools.
    #[instrument(skip(self), fields(mint = %candidate.mint))]
    async fn find_orca_pools(
        &self,
        candidate: &PremintCandidate,
        _rpc: &RpcClient,
    ) -> Result<Option<LiquidityPool>> {
        debug!("Searching for Orca pools");
        
        // Simplified implementation
        Ok(None)
    }

    /// Fetch volume and transaction data.
    #[instrument(skip(self), fields(mint = %candidate.mint))]
    async fn fetch_volume_data(
        &self,
        candidate: &PremintCandidate,
        _rpc: &RpcClient,
    ) -> Result<VolumeData> {
        debug!("Fetching volume data");
        
        // Placeholder implementation - would analyze recent transactions
        // let signatures = _rpc.get_signatures_for_address(&candidate.mint).await?;
        
        let transaction_count = 150_u32; // Mock data

        // Analyze transactions for volume calculation
        let mut total_volume = 0.0;
        let mut buy_volume = 0.0;
        let mut sell_volume = 0.0;

        // Simplified volume calculation from transaction count
        let estimated_volume = transaction_count as f64 * 10.0; // Rough estimate
        let initial_volume = estimated_volume * 0.3;
        let volume_growth_rate = if initial_volume > 0.0 {
            estimated_volume / initial_volume
        } else {
            1.0
        };

        let buy_sell_ratio = if sell_volume > 0.0 {
            buy_volume / sell_volume
        } else {
            1.0
        };

        debug!("Volume data: {} transactions, growth rate: {:.2}x", 
               transaction_count, volume_growth_rate);

        Ok(VolumeData {
            initial_volume,
            current_volume: estimated_volume,
            volume_growth_rate,
            transaction_count,
            buy_sell_ratio,
        })
    }

    /// Fetch creator holdings and sell activity.
    #[instrument(skip(self), fields(mint = %candidate.mint, creator = %candidate.creator))]
    async fn fetch_creator_holdings(
        &self,
        candidate: &PremintCandidate,
        _rpc: &(), // TODO: Replace with RpcClient when Solana deps are available
    ) -> Result<CreatorHoldings> {
        // TODO: Implement when Solana dependencies are available
        // For now, return stub data
        Ok(CreatorHoldings {
            initial_balance: 150_000_000, // 150M tokens initially
            current_balance: 135_000_000, // 135M tokens now
            first_sell_timestamp: Some(candidate.timestamp - 3600), // Sold 1 hour after creation
            sell_transactions: 3,
        })
    }

    /// Fetch social activity data.
    #[instrument(skip(self), fields(mint = %candidate.mint))]
    async fn fetch_social_activity(&self, candidate: &PremintCandidate) -> Result<SocialActivity> {
        debug!("Fetching social activity data");
        
        // Simplified implementation - would use social media APIs
        // For now, return mock data
        Ok(SocialActivity {
            twitter_mentions: 5,
            telegram_members: 150,
            discord_members: 75,
            social_score: 0.3,
        })
    }

    // --- Pillar III: Macro-economic Data Sources for MarketRegimeDetector ---

    /// Fetch current SOL/USD price from external API (CoinGecko).
    #[instrument(skip(self))]
    pub async fn fetch_sol_price_usd(&self) -> Result<f64> {
        let url = "https://api.coingecko.com/api/v3/simple/price?ids=solana&vs_currencies=usd";
        
        let retry_strategy = ExponentialBackoff::from_millis(500)
            .max_delay(Duration::from_secs(3))
            .take(3);

        Retry::spawn(retry_strategy, || async {
            let response = self.http_client
                .get(url)
                .send()
                .await?
                .json::<serde_json::Value>()
                .await?;
            
            let price = response["solana"]["usd"]
                .as_f64()
                .context("Failed to parse SOL price from CoinGecko")?;
            
            debug!("Fetched SOL price: ${:.2}", price);
            Ok(price)
        }).await
    }

    /// Calculate simple volatility indicator based on price history.
    #[instrument(skip(self, price_history))]
    pub async fn calculate_sol_volatility(&self, price_history: &[f64]) -> Result<f64> {
        if price_history.len() < 2 {
            return Ok(0.0);
        }

        let mean = price_history.iter().sum::<f64>() / price_history.len() as f64;
        let variance = price_history.iter()
            .map(|&p| (p - mean).powi(2))
            .sum::<f64>() / price_history.len() as f64;
        
        let std_dev = variance.sqrt();
        let volatility_percentage = (std_dev / mean) * 100.0; // Volatility as % of price
        
        debug!("Calculated volatility: {:.2}%", volatility_percentage);
        Ok(volatility_percentage)
    }

    /// Fetch network TPS (Transactions Per Second) as a measure of network load.
    /// This is a simplified implementation that would use RPC performance samples.
    #[instrument(skip(self))]
    pub async fn fetch_network_tps(&self) -> Result<f64> {
        // TODO: When Solana RPC client is available, implement:
        // let samples = rpc.get_recent_performance_samples(Some(5)).await?;
        // For now, return simulated TPS based on time of day
        
        let current_hour = chrono::Utc::now().hour();
        let simulated_tps = match current_hour {
            // Peak hours (US market hours): higher TPS
            14..=22 => 2000.0 + (current_hour as f64 * 50.0),
            // Off-peak hours: lower TPS
            _ => 1000.0 + (current_hour as f64 * 20.0),
        };
        
        debug!("Network TPS estimate: {:.1}", simulated_tps);
        Ok(simulated_tps)
    }

    /// Fetch global DEX volume data (simplified implementation).
    /// In a real implementation, this would aggregate data from Raydium, Orca, etc.
    #[instrument(skip(self))]
    pub async fn fetch_global_dex_volume(&self) -> Result<f64> {
        // Simplified implementation - would query actual DEX APIs
        // For now, simulate varying volume based on time patterns
        
        let current_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs();
        
        // Simulate daily volume patterns
        let hour_factor = ((current_timestamp % 86400) as f64 / 86400.0 * 2.0 * std::f64::consts::PI).sin().abs();
        let base_volume = 50_000_000.0; // 50M USD base
        let daily_volume = base_volume + (base_volume * 0.5 * hour_factor);
        
        debug!("Global DEX volume estimate: ${:.0}", daily_volume);
        Ok(daily_volume)
    }
}

impl Default for VolumeData {
    fn default() -> Self {
        Self {
            initial_volume: 0.0,
            current_volume: 0.0,
            volume_growth_rate: 1.0,
            transaction_count: 0,
            buy_sell_ratio: 1.0,
        }
    }
}

impl Default for CreatorHoldings {
    fn default() -> Self {
        Self {
            initial_balance: 0,
            current_balance: 0,
            first_sell_timestamp: None,
            sell_transactions: 0,
        }
    }
}

impl Default for SocialActivity {
    fn default() -> Self {
        Self {
            twitter_mentions: 0,
            telegram_members: 0,
            discord_members: 0,
            social_score: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::types::OracleConfig;
    use crate::types::{PremintCandidate, Pubkey};
    use reqwest::Client;

    fn create_test_config() -> OracleConfig {
        OracleConfig::default()
    }

    fn create_test_candidate() -> PremintCandidate {
        PremintCandidate {
            mint: "TestMintAddress".to_string(),
            creator: "TestCreatorAddress".to_string(),
            program: "test".to_string(),
            slot: 12345,
            timestamp: 1640995200,
            instruction_summary: None,
            is_jito_bundle: Some(true),
        }
    }

    #[test]
    fn test_volume_data_default() {
        let volume_data = VolumeData::default();
        
        assert_eq!(volume_data.initial_volume, 0.0);
        assert_eq!(volume_data.current_volume, 0.0);
        assert_eq!(volume_data.volume_growth_rate, 1.0);
        assert_eq!(volume_data.transaction_count, 0);
        assert_eq!(volume_data.buy_sell_ratio, 1.0);
    }

    #[test]
    fn test_creator_holdings_default() {
        let creator_holdings = CreatorHoldings::default();
        
        assert_eq!(creator_holdings.initial_balance, 0);
        assert_eq!(creator_holdings.current_balance, 0);
        assert_eq!(creator_holdings.first_sell_timestamp, None);
        assert_eq!(creator_holdings.sell_transactions, 0);
    }

    #[test]
    fn test_social_activity_default() {
        let social_activity = SocialActivity::default();
        
        assert_eq!(social_activity.twitter_mentions, 0);
        assert_eq!(social_activity.telegram_members, 0);
        assert_eq!(social_activity.discord_members, 0);
        assert_eq!(social_activity.social_score, 0.0);
    }

    #[tokio::test]
    async fn test_data_sources_creation() {
        let config = create_test_config();
        let http_client = Client::new();
        let rpc_clients = vec![];
        
        let data_sources = OracleDataSources::new(rpc_clients, http_client, config);
        
        // Just verify construction works
        assert_eq!(data_sources.rpc_clients.len(), 0);
    }
}
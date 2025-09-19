//! Data sources for fetching on-chain and off-chain token information.
//!
//! This module handles all external data fetching including RPC calls,
//! API requests, and metadata retrieval with retry logic and caching.

use crate::oracle::types::{
    TokenData, Metadata, HolderData, LiquidityPool, VolumeData, CreatorHoldings,
    SocialActivity, OracleConfig, PoolType, Attribute,
};
use crate::types::PremintCandidate;
use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
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
        let creator_holdings = self.fetch_creator_holdings(candidate, rpc).await
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
    #[instrument(skip(self, rpc), fields(mint = %candidate.mint))]
    async fn fetch_token_supply(
        &self,
        candidate: &PremintCandidate,
        rpc: &RpcClient,
    ) -> Result<(u64, u8)> {
        let mint_account = rpc
            .get_account(&candidate.mint)
            .await
            .context("Failed to fetch mint account")?;

        // Parse mint account data (simplified)
        if mint_account.data.len() >= 82 {
            // SPL Token mint layout: supply at offset 36-44, decimals at offset 44
            let supply_bytes = &mint_account.data[36..44];
            let supply = u64::from_le_bytes(
                supply_bytes.try_into()
                    .context("Invalid supply data")?
            );
            let decimals = mint_account.data[44];
            
            debug!("Token supply: {}, decimals: {}", supply, decimals);
            Ok((supply, decimals))
        } else {
            Err(anyhow!("Invalid mint account data length"))
        }
    }

    /// Resolve metadata URI from mint account.
    #[instrument(skip(self, rpc), fields(mint = %mint_address))]
    async fn resolve_metadata_uri(
        &self,
        mint_address: &Pubkey,
        rpc: &RpcClient,
    ) -> Result<String> {
        // This is a simplified implementation
        // In reality, this would parse the metadata account
        debug!("Resolving metadata URI for {}", mint_address);
        
        // For now, return a placeholder
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
    #[instrument(skip(self, rpc), fields(mint = %candidate.mint))]
    async fn fetch_holder_distribution(
        &self,
        candidate: &PremintCandidate,
        rpc: &RpcClient,
    ) -> Result<Vec<HolderData>> {
        let largest_accounts = rpc
            .get_token_largest_accounts(&candidate.mint)
            .await
            .context("Failed to fetch token largest accounts")?;

        let total_supply = rpc
            .get_token_supply(&candidate.mint)
            .await
            .context("Failed to fetch token supply")?
            .amount
            .parse::<u64>()
            .context("Failed to parse token supply")?;

        let mut holders = Vec::new();

        for account in largest_accounts {
            // Simplified implementation for now - we'll use a default value
            let amount = 1000.0; // Placeholder amount
            
            let percentage = if total_supply > 0 {
                amount / (total_supply as f64 / 10f64.powi(9))
            } else {
                0.0
            };

            let is_whale = percentage >= self.config.thresholds.whale_threshold;

            holders.push(HolderData {
                address: Pubkey::new_unique(), // Simplified for now
                percentage,
                is_whale,
            });
        }

        debug!("Fetched {} holders", holders.len());
        Ok(holders)
    }

    /// Fetch liquidity pool data.
    #[instrument(skip(self, rpc), fields(mint = %candidate.mint))]
    async fn fetch_liquidity_data(
        &self,
        candidate: &PremintCandidate,
        rpc: &RpcClient,
    ) -> Result<Option<LiquidityPool>> {
        // Try to find Raydium pools first
        if let Ok(raydium_pools) = self.find_raydium_pools(candidate, rpc).await {
            if let Some(pool) = raydium_pools.first() {
                return Ok(Some(pool.clone()));
            }
        }

        // Try Pump.fun pools
        if let Ok(pump_pool) = self.find_pump_fun_pool(candidate, rpc).await {
            if pump_pool.is_some() {
                return Ok(pump_pool);
            }
        }

        // Try Orca pools
        if let Ok(orca_pool) = self.find_orca_pools(candidate, rpc).await {
            if orca_pool.is_some() {
                return Ok(orca_pool);
            }
        }

        debug!("No liquidity pools found");
        Ok(None)
    }

    /// Find Raydium liquidity pools.
    #[instrument(skip(self, rpc), fields(mint = %candidate.mint))]
    async fn find_raydium_pools(
        &self,
        candidate: &PremintCandidate,
        rpc: &RpcClient,
    ) -> Result<Vec<LiquidityPool>> {
        // Simplified implementation - would need proper Raydium pool scanning
        debug!("Searching for Raydium pools");
        
        // For now, return empty - real implementation would scan for AMM pools
        Ok(vec![])
    }

    /// Find Pump.fun pool.
    #[instrument(skip(self, rpc), fields(mint = %candidate.mint))]
    async fn find_pump_fun_pool(
        &self,
        candidate: &PremintCandidate,
        rpc: &RpcClient,
    ) -> Result<Option<LiquidityPool>> {
        debug!("Searching for Pump.fun pool");
        
        // Simplified implementation
        // Would use Pump.fun API if key is available
        if self.config.pump_fun_api_key.is_some() {
            // Mock pool for demonstration
            Ok(Some(LiquidityPool {
                sol_amount: 25.0,
                token_amount: 1000000.0,
                pool_address: Pubkey::new_unique(),
                pool_type: PoolType::PumpFun,
            }))
        } else {
            Ok(None)
        }
    }

    /// Find Orca pools.
    #[instrument(skip(self, rpc), fields(mint = %candidate.mint))]
    async fn find_orca_pools(
        &self,
        candidate: &PremintCandidate,
        rpc: &RpcClient,
    ) -> Result<Option<LiquidityPool>> {
        debug!("Searching for Orca pools");
        
        // Simplified implementation
        Ok(None)
    }

    /// Fetch volume and transaction data.
    #[instrument(skip(self, rpc), fields(mint = %candidate.mint))]
    async fn fetch_volume_data(
        &self,
        candidate: &PremintCandidate,
        rpc: &RpcClient,
    ) -> Result<VolumeData> {
        let signatures = rpc
            .get_signatures_for_address(&candidate.mint)
            .await
            .context("Failed to fetch transaction signatures")?;

        let transaction_count = signatures.len() as u32;

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
    #[instrument(skip(self, rpc), fields(mint = %candidate.mint, creator = %candidate.creator))]
    async fn fetch_creator_holdings(
        &self,
        candidate: &PremintCandidate,
        rpc: &RpcClient,
    ) -> Result<CreatorHoldings> {
        // Get creator's associated token account
        let associated_token_address = spl_associated_token_account::get_associated_token_address(
            &candidate.creator,
            &candidate.mint,
        );

        // Fetch current balance
        let current_balance = match rpc.get_token_account_balance(&associated_token_address).await {
            Ok(balance) => balance.amount.parse::<u64>().unwrap_or(0),
            Err(_) => 0, // Account might not exist or be closed
        };

        // For simplicity, estimate initial balance
        // Real implementation would track this over time
        let initial_balance = current_balance + 10_000_000; // Assume some tokens were sold

        debug!("Creator holdings: current={}, estimated_initial={}", 
               current_balance, initial_balance);

        Ok(CreatorHoldings {
            initial_balance,
            current_balance,
            first_sell_timestamp: Some(candidate.timestamp + 300), // 5 minutes after creation
            sell_transactions: if current_balance < initial_balance { 1 } else { 0 },
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
    use crate::types::PremintCandidate;
    use solana_sdk::pubkey::Pubkey;
    use std::sync::Arc;

    fn create_test_config() -> OracleConfig {
        OracleConfig::default()
    }

    fn create_test_candidate() -> PremintCandidate {
        PremintCandidate {
            mint: Pubkey::new_unique(),
            creator: Pubkey::new_unique(),
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
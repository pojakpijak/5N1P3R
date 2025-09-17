/*!
MarketMaker Module - Second module [2/2] of Market Simulator environment

This module generates realistic on-chain activities for existing tokens based on their profiles.
It creates simulated trading activities to test the SNIPER bot's behavior in different market conditions.
This advanced version introduces a dynamic activity model with distinct market phases.
*/

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use fastrand;
use solana_sdk::pubkey::Pubkey;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

// These would be imported from the bot crate in a real workspace setup
use crate::token_generator::{TokenProfile, GeneratedToken};
use crate::wallet::WalletManager;
use crate::tx_builder::TransactionBuilder;

/// Defines the current market phase for a token, driving the simulation's behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketPhase {
    /// Initial high-volume, frequent buying activity to simulate a launch.
    Hype,
    /// A period of calmer, mixed buy/sell activity.
    Consolidation,
    /// A final phase of increased selling pressure.
    SellOff,
}

/// Holds the dynamic state for a token being managed by the market maker.
#[derive(Debug, Clone)]
pub struct TokenState {
    pub mint: Pubkey,
    pub profile: TokenProfile,
    pub created_at: Instant,
    pub activity_count: u32,
    pub is_active: bool,
    // New fields for dynamic activity model
    pub current_phase: MarketPhase,
    pub phase_start_time: Instant,
}


/// Configuration for MarketMaker
#[derive(Debug, Clone)]
pub struct MarketMakerConfig {
    pub loop_interval_ms: u64,
    pub trader_wallet_count: usize,
    // --- Dynamic Activity Model Parameters ---
    pub hype_phase_duration_secs: (u64, u64),
    pub consolidation_phase_duration_secs: (u64, u64),
    pub selloff_phase_duration_secs: (u64, u64),
    pub hype_phase_tx_interval_ms: (u64, u64),
    // --- Rug Pull Parameters ---
    pub rug_min_sleep_mins: u64,
    pub rug_max_sleep_mins: u64,
    // --- Trash Token Parameters ---
    pub trash_transaction_count: u32,
}

impl Default for MarketMakerConfig {
    fn default() -> Self {
        Self {
            loop_interval_ms: 1000,
            trader_wallet_count: 10,
            hype_phase_duration_secs: (10, 30),
            consolidation_phase_duration_secs: (30, 90),
            selloff_phase_duration_secs: (10, 20),
            hype_phase_tx_interval_ms: (50, 200),
            rug_min_sleep_mins: 1,
            rug_max_sleep_mins: 3,
            trash_transaction_count: 3,
        }
    }
}

/// MarketMaker manages simulated trading activities for tokens
pub struct MarketMaker {
    config: MarketMakerConfig,
    live_tokens: Arc<tokio::sync::RwLock<HashMap<Pubkey, TokenState>>>,
    trader_wallets: Vec<Arc<WalletManager>>,
    creator_rug_wallet: Arc<WalletManager>,
    tx_builder: Option<Arc<TransactionBuilder>>,
    is_running: Arc<tokio::sync::RwLock<bool>>,
}

impl MarketMaker {
    /// Create a new MarketMaker instance
    pub fn new(config: MarketMakerConfig) -> Result<Self> {
        info!("🏭 Creating MarketMaker with {} trader wallets", config.trader_wallet_count);
        let trader_wallets = (0..config.trader_wallet_count)
            .map(|_| Arc::new(WalletManager::new_random()))
            .collect();
        let creator_rug_wallet = Arc::new(WalletManager::new_random());
        info!("Generated creator rug wallet: {}", creator_rug_wallet.pubkey());

        Ok(Self {
            config,
            live_tokens: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            trader_wallets,
            creator_rug_wallet,
            tx_builder: None,
            is_running: Arc::new(tokio::sync::RwLock::new(false)),
        })
    }

    pub fn set_transaction_builder(&mut self, tx_builder: Arc<TransactionBuilder>) {
        self.tx_builder = Some(tx_builder);
        info!("✅ Transaction builder configured for MarketMaker");
    }

    /// Add a new token to be managed by the MarketMaker
    pub async fn add_token(&self, token: &GeneratedToken) {
        let token_state = TokenState {
            mint: token.mint,
            profile: token.profile,
            created_at: Instant::now(),
            activity_count: 0,
            is_active: true,
            current_phase: MarketPhase::Hype,
            phase_start_time: Instant::now(),
        };
        self.live_tokens.write().await.insert(token.mint, token_state);
        info!("📈 Added token {} with profile {:?} to MarketMaker, starting in Hype phase.", token.mint, token.profile);
    }

    /// Start the MarketMaker main loop
    pub async fn start(&self) -> Result<()> {
        *self.is_running.write().await = true;
        info!("🚀 Starting MarketMaker main loop");
        
        let mut ticker = interval(Duration::from_millis(self.config.loop_interval_ms));
        
        loop {
            if !*self.is_running.read().await {
                info!("🛑 MarketMaker main loop stopped");
                break;
            }
            ticker.tick().await;
            if let Err(e) = self.process_tokens().await {
                error!("Error processing tokens: {}", e);
            }
        }
        Ok(())
    }

    pub async fn stop(&self) {
        *self.is_running.write().await = false;
        info!("🛑 MarketMaker stop requested");
    }

    /// Process all active tokens according to their profiles and phases
    async fn process_tokens(&self) -> Result<()> {
        let tokens_snapshot = self.live_tokens.read().await.clone();
        for (mint, token_state) in tokens_snapshot {
            if !token_state.is_active { continue; }

            // Spawn a task for each token to handle its logic concurrently
            let self_clone = self.clone_for_task();
            tokio::spawn(async move {
                if let Err(e) = self_clone.process_single_token(mint, token_state).await {
                    error!("Error processing token {}: {}", mint, e);
                }
            });
        }
        Ok(())
    }

    /// Process a single token based on its profile
    async fn process_single_token(&self, mint: Pubkey, mut token_state: TokenState) -> Result<()> {
        match token_state.profile {
            TokenProfile::Gem => self.handle_gem_token(&mut token_state).await,
            TokenProfile::Rug => self.handle_rug_token(&mut token_state).await,
            TokenProfile::Trash => self.handle_trash_token(&mut token_state).await,
        }
    }

    /// Handle Gem token logic with dynamic market phases.
    async fn handle_gem_token(&self, token_state: &mut TokenState) -> Result<()> {
        let phase_elapsed = token_state.phase_start_time.elapsed();
        let mut next_phase = None;
        let mut activity_this_tick = false;

        match token_state.current_phase {
            MarketPhase::Hype => {
                let (min, max) = self.config.hype_phase_duration_secs;
                if phase_elapsed.as_secs() > fastrand::u64(min..=max) {
                    next_phase = Some(MarketPhase::Consolidation);
                } else {
                    activity_this_tick = true; // High frequency activity
                }
            }
            MarketPhase::Consolidation => {
                let (min, max) = self.config.consolidation_phase_duration_secs;
                if phase_elapsed.as_secs() > fastrand::u64(min..=max) {
                    next_phase = Some(MarketPhase::SellOff);
                } else if fastrand::bool() { // Lower frequency activity
                    activity_this_tick = true;
                }
            }
            MarketPhase::SellOff => {
                let (min, max) = self.config.selloff_phase_duration_secs;
                if phase_elapsed.as_secs() > fastrand::u64(min..=max) {
                    token_state.is_active = false; // End of life for this token
                    info!("💎 Gem token {} activity completed.", token_state.mint);
                } else if fastrand::u8(0..3) == 0 { // Infrequent, larger sells
                    activity_this_tick = true;
                }
            }
        }
        
        if activity_this_tick && token_state.is_active {
            self.simulate_trader_activity(token_state).await;
        }

        if let Some(phase) = next_phase {
            info!("💎 Token {} transitioning to {:?} phase.", token_state.mint, phase);
            token_state.current_phase = phase;
            token_state.phase_start_time = Instant::now();
        }
        
        // Update the state in the shared map
        self.live_tokens.write().await.insert(token_state.mint, token_state.clone());
        Ok(())
    }

    async fn simulate_trader_activity(&self, token_state: &mut TokenState) {
        let trader = &self.trader_wallets[fastrand::usize(..self.trader_wallets.len())];
        debug!("💎 Simulating trader activity for {} from wallet {}", token_state.mint, trader.pubkey());
        token_state.activity_count += 1;
        // In a full implementation, this would call tx_builder to create a buy/sell tx.
    }
    
    // Simplified handlers for Rug and Trash
    async fn handle_rug_token(&self, token_state: &mut TokenState) -> Result<()> {
         let (min, max) = (self.config.rug_min_sleep_mins, self.config.rug_max_sleep_mins);
         let sleep_duration = Duration::from_mins(fastrand::u64(min..=max));
         if token_state.created_at.elapsed() >= sleep_duration {
             warn!("💀 Executing RUG PULL for token {}!", token_state.mint);
             token_state.is_active = false;
             // Here, you would build and send a transaction to remove liquidity.
             self.live_tokens.write().await.remove(&token_state.mint);
         }
        Ok(())
    }
    
    async fn handle_trash_token(&self, token_state: &mut TokenState) -> Result<()> {
        if token_state.activity_count < self.config.trash_transaction_count {
            self.simulate_trader_activity(token_state).await;
            self.live_tokens.write().await.insert(token_state.mint, token_state.clone());
        } else {
            info!("🗑️ Trash token {} removed after {} transactions.", token_state.mint, token_state.activity_count);
            token_state.is_active = false;
            self.live_tokens.write().await.remove(&token_state.mint);
        }
        Ok(())
    }

    // Helper to clone self for spawning tasks
    fn clone_for_task(&self) -> Self {
        Self {
            config: self.config.clone(),
            live_tokens: self.live_tokens.clone(),
            trader_wallets: self.trader_wallets.clone(),
            creator_rug_wallet: self.creator_rug_wallet.clone(),
            tx_builder: self.tx_builder.clone(),
            is_running: self.is_running.clone(),
        }
    }
}

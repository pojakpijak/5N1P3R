//! Types for DecisionLedger and PerformanceMonitor systems
//!
//! This contains types needed for the DecisionLedger system and Pillar II components.

use crate::types::{PremintCandidate, Pubkey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Scored candidate with simplified structure for demo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredCandidate {
    /// Original candidate information
    pub base: PremintCandidate,
    /// The mint address
    pub mint: Pubkey,
    /// Predicted score (0-100)
    pub predicted_score: u8,
    /// Feature scores breakdown
    pub feature_scores: HashMap<String, f64>,
    /// Explanation of the score
    pub reason: String,
    /// Calculation time in microseconds
    pub calculation_time: u128,
    /// Whether anomaly was detected
    pub anomaly_detected: bool,
    /// Timestamp when scored
    pub timestamp: u64,
}

/// Represents the final financial outcome of a transaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Outcome {
    /// Profit in SOL
    Profit(f64),
    /// Loss in SOL
    Loss(f64),
    /// No change (e.g., failed transaction, no confirmation)
    Neutral,
    /// Transaction sent but still waiting for result
    PendingConfirmation,
    /// Transaction failed during execution (e.g., timeout, reverted)
    FailedExecution,
    /// Candidate scored but transaction never sent
    NotExecuted,
}

impl Default for Outcome {
    fn default() -> Self {
        Outcome::NotExecuted
    }
}

/// Complete record of a PredictiveOracle decision and its transactional outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRecord {
    /// Database record ID (set by DB)
    pub id: Option<i64>,
    /// The scored candidate that triggered this decision
    pub scored_candidate: ScoredCandidate,
    
    /// Transaction data (if sent)
    pub transaction_signature: Option<String>,
    /// Purchase price in SOL per token
    pub buy_price_sol: Option<f64>,
    /// Sale price in SOL per token
    pub sell_price_sol: Option<f64>,
    /// Amount of tokens bought
    pub amount_bought_tokens: Option<f64>,
    /// Amount of tokens sold
    pub amount_sold_tokens: Option<f64>,
    /// Total SOL spent on purchase
    pub initial_sol_spent: Option<f64>,
    /// Total SOL received from sale (net)
    pub final_sol_received: Option<f64>,
    
    /// When the decision was made (timestamp from PremintCandidate)
    pub timestamp_decision_made: u64,
    /// When the purchase transaction was sent
    pub timestamp_transaction_sent: Option<u64>,
    /// When the final outcome was evaluated
    pub timestamp_outcome_evaluated: Option<u64>,
    
    /// The final transaction outcome
    pub actual_outcome: Outcome,
    
    /// Market context snapshot at decision time (for later analysis)
    /// This will be populated by the MarketRegimeDetector in the future
    pub market_context_snapshot: HashMap<String, f64>,
}

// --- Communication Channels for DecisionLedger ---

/// Channel for sending new decisions to DecisionLedger
pub type DecisionRecordSender = tokio::sync::mpsc::Sender<TransactionRecord>;
pub type DecisionRecordReceiver = tokio::sync::mpsc::Receiver<TransactionRecord>;

/// Channel for sending outcome updates to DecisionLedger
/// (signature, outcome, buy_price, sell_price, sol_spent, sol_received, timestamp_evaluated)
pub type OutcomeUpdateSender = tokio::sync::mpsc::Sender<(String, Outcome, Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<u64>)>;
pub type OutcomeUpdateReceiver = tokio::sync::mpsc::Receiver<(String, Outcome, Option<f64>, Option<f64>, Option<f64>, Option<f64>, Option<u64>)>;

// --- Pillar II: Performance Monitor and Strategy Optimizer Types ---

/// Feature weights for scoring algorithm (imported from types_old.rs structure)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureWeights {
    pub liquidity: f64,
    pub holder_distribution: f64,
    pub volume_growth: f64,
    pub holder_growth: f64,
    pub price_change: f64,
    pub jito_bundle_presence: f64,
    pub creator_sell_speed: f64,
    pub metadata_quality: f64,
    pub social_activity: f64,
}

impl Default for FeatureWeights {
    fn default() -> Self {
        Self {
            liquidity: 0.20,
            holder_distribution: 0.15,
            volume_growth: 0.15,
            holder_growth: 0.10,
            price_change: 0.10,
            jito_bundle_presence: 0.05,
            creator_sell_speed: 0.10,
            metadata_quality: 0.10,
            social_activity: 0.05,
        }
    }
}

/// Score thresholds for various features
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreThresholds {
    pub min_liquidity_sol: f64,
    pub whale_threshold: f64,
    pub volume_growth_threshold: f64,
    pub holder_growth_threshold: f64,
    pub min_metadata_quality: f64,
    pub creator_sell_penalty_threshold: u64,
    pub social_activity_threshold: f64,
}

impl Default for ScoreThresholds {
    fn default() -> Self {
        Self {
            min_liquidity_sol: 10.0,
            whale_threshold: 0.15,
            volume_growth_threshold: 2.0,
            holder_growth_threshold: 1.5,
            min_metadata_quality: 0.7,
            creator_sell_penalty_threshold: 300,
            social_activity_threshold: 100.0,
        }
    }
}

/// Complete performance report for strategy evaluation
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PerformanceReport {
    pub timestamp: u64,
    pub time_window_hours: f64,
    pub total_trades_evaluated: usize,
    
    // Key Performance Indicators (KPIs)
    pub win_rate_percent: f64, // (Profitable trades / All closed trades) * 100
    pub profit_factor: f64,    // (Sum of profits / Sum of losses)
    pub average_profit_sol: f64,
    pub average_loss_sol: f64,
    pub net_profit_sol: f64,
    pub max_drawdown_percent: f64, // Maximum capital drawdown
}

/// Set of optimized parameters for Oracle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimizedParameters {
    pub new_weights: FeatureWeights,
    pub new_thresholds: ScoreThresholds,
    pub reason: String, // Justification for the change
}

// --- Communication Channels for Pillar II ---

// From PerformanceMonitor to StrategyOptimizer
pub type PerformanceReportSender = tokio::sync::mpsc::Sender<PerformanceReport>;
pub type PerformanceReportReceiver = tokio::sync::mpsc::Receiver<PerformanceReport>;

// From StrategyOptimizer to main loop
pub type OptimizedParametersSender = tokio::sync::mpsc::Sender<OptimizedParameters>;
pub type OptimizedParametersReceiver = tokio::sync::mpsc::Receiver<OptimizedParameters>;

// --- Pillar III: Contextual Adaptations (MarketRegimeDetector) Types ---

/// Represents the identified market state/regime.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum MarketRegime {
    /// Bullish market: high activity, rising prices, low risk aversion
    Bullish,
    /// Bearish market: low activity, falling prices, high risk aversion
    Bearish,
    /// Choppy market: sideways movement, high volatility, no clear trend
    Choppy,
    /// High network congestion: elevated fees, high transaction failure risk
    HighCongestion,
    /// Low activity market: very low volume and activity
    LowActivity,
}

impl Default for MarketRegime {
    fn default() -> Self {
        MarketRegime::LowActivity
    }
}

/// Set of scoring parameters specific to a market regime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeSpecificParameters {
    pub weights: FeatureWeights,
    pub thresholds: ScoreThresholds,
    // Can add other parameters like buy_score_threshold in the future
}

impl Default for RegimeSpecificParameters {
    fn default() -> Self {
        Self {
            weights: FeatureWeights::default(),
            thresholds: ScoreThresholds::default(),
        }
    }
}

/// Extended Oracle configuration with regime-specific parameters for Pillar III.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleConfig {
    /// RPC endpoints
    pub rpc_endpoints: Vec<String>,
    /// Pump.fun API key
    pub pump_fun_api_key: Option<String>,
    /// Bitquery API key
    pub bitquery_api_key: Option<String>,
    /// RPC retry attempts
    pub rpc_retry_attempts: usize,
    /// RPC timeout in seconds
    pub rpc_timeout_seconds: u64,
    /// Cache TTL in seconds
    pub cache_ttl_seconds: u64,
    /// Maximum parallel requests
    pub max_parallel_requests: usize,
    /// Rate limit requests per second
    pub rate_limit_requests_per_second: u32,
    /// GUI notification threshold
    pub notify_threshold: u8,

    /// Regime-specific parameters mapping for Pillar III
    /// Each market regime has its own set of weights and thresholds
    pub regime_parameters: std::collections::HashMap<MarketRegime, RegimeSpecificParameters>,

    // Additional modular architecture fields
    /// Adaptive weights recalculation interval
    pub adaptive_recalc_interval: u64,
    /// Circuit breaker failure threshold
    pub circuit_breaker_failure_threshold: u32,
    /// Circuit breaker cooldown in seconds
    pub circuit_breaker_cooldown_seconds: u64,
    /// Maximum cache entries
    pub max_cache_entries: usize,
}

impl Default for OracleConfig {
    fn default() -> Self {
        let mut regime_parameters = std::collections::HashMap::new();
        
        // Default parameters for each regime - these would be optimized based on historical data
        let low_activity = RegimeSpecificParameters {
            weights: FeatureWeights {
                liquidity: 0.25,      // Higher weight on liquidity in low activity
                holder_distribution: 0.20,
                volume_growth: 0.10,  // Lower weight as volume is low
                holder_growth: 0.15,
                price_change: 0.05,   // Less important in low activity
                jito_bundle_presence: 0.05,
                creator_sell_speed: 0.10,
                metadata_quality: 0.10,
                social_activity: 0.00, // Almost irrelevant in low activity
            },
            thresholds: ScoreThresholds::default(),
        };

        let bullish = RegimeSpecificParameters {
            weights: FeatureWeights {
                liquidity: 0.15,
                holder_distribution: 0.10,
                volume_growth: 0.25,  // High weight on volume in bull market
                holder_growth: 0.20,  // High weight on holder growth
                price_change: 0.15,   // Price momentum important
                jito_bundle_presence: 0.05,
                creator_sell_speed: 0.05, // Less concern about creator selling in bull market
                metadata_quality: 0.05,
                social_activity: 0.00,
            },
            thresholds: ScoreThresholds {
                min_liquidity_sol: 5.0, // Lower requirement in bull market
                whale_threshold: 0.20,   // Higher tolerance for whales
                volume_growth_threshold: 1.5, // Lower bar for volume growth
                holder_growth_threshold: 1.2,
                min_metadata_quality: 0.6,
                creator_sell_penalty_threshold: 500, // Higher tolerance
                social_activity_threshold: 50.0,
            },
        };

        let bearish = RegimeSpecificParameters {
            weights: FeatureWeights {
                liquidity: 0.30,      // Very high weight on liquidity in bear market
                holder_distribution: 0.25, // Important to avoid whale dumps
                volume_growth: 0.05,  // Volume growth rare in bear market
                holder_growth: 0.05,
                price_change: 0.00,   // Price changes often negative
                jito_bundle_presence: 0.05,
                creator_sell_speed: 0.20, // Very important - avoid fast selling creators
                metadata_quality: 0.10,
                social_activity: 0.00,
            },
            thresholds: ScoreThresholds {
                min_liquidity_sol: 20.0, // Higher requirement in bear market
                whale_threshold: 0.10,   // Lower tolerance for whales
                volume_growth_threshold: 3.0, // Higher bar for volume growth
                holder_growth_threshold: 2.0,
                min_metadata_quality: 0.8,
                creator_sell_penalty_threshold: 150, // Lower tolerance
                social_activity_threshold: 200.0,
            },
        };

        let choppy = RegimeSpecificParameters {
            weights: FeatureWeights {
                liquidity: 0.20,
                holder_distribution: 0.15,
                volume_growth: 0.15,
                holder_growth: 0.15,
                price_change: 0.00,   // Price changes unreliable in choppy market
                jito_bundle_presence: 0.10, // More important for execution timing
                creator_sell_speed: 0.15,
                metadata_quality: 0.10,
                social_activity: 0.00,
            },
            thresholds: ScoreThresholds::default(),
        };

        let high_congestion = RegimeSpecificParameters {
            weights: FeatureWeights {
                liquidity: 0.15,
                holder_distribution: 0.10,
                volume_growth: 0.10,
                holder_growth: 0.10,
                price_change: 0.10,
                jito_bundle_presence: 0.35, // Very high weight on Jito bundles for execution
                creator_sell_speed: 0.05,
                metadata_quality: 0.05,
                social_activity: 0.00,
            },
            thresholds: ScoreThresholds {
                min_liquidity_sol: 15.0,
                whale_threshold: 0.15,
                volume_growth_threshold: 2.0,
                holder_growth_threshold: 1.5,
                min_metadata_quality: 0.5, // Lower bar due to execution urgency
                creator_sell_penalty_threshold: 300,
                social_activity_threshold: 100.0,
            },
        };

        regime_parameters.insert(MarketRegime::LowActivity, low_activity);
        regime_parameters.insert(MarketRegime::Bullish, bullish);
        regime_parameters.insert(MarketRegime::Bearish, bearish);
        regime_parameters.insert(MarketRegime::Choppy, choppy);
        regime_parameters.insert(MarketRegime::HighCongestion, high_congestion);

        Self {
            rpc_endpoints: vec!["https://api.mainnet-beta.solana.com".to_string()],
            pump_fun_api_key: None,
            bitquery_api_key: None,
            rpc_retry_attempts: 3,
            rpc_timeout_seconds: 10,
            cache_ttl_seconds: 300,
            max_parallel_requests: 10,
            rate_limit_requests_per_second: 20,
            notify_threshold: 75,
            regime_parameters,
            adaptive_recalc_interval: 100,
            circuit_breaker_failure_threshold: 5,
            circuit_breaker_cooldown_seconds: 60,
            max_cache_entries: 1000,
        }
    }
}
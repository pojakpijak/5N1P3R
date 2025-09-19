//! Core types and data structures for the Oracle system.

use crate::types::{PremintCandidate, QuantumCandidateGui};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

/// Feature types supported by the Oracle scoring system.
/// Each feature represents a different aspect of token evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Feature {
    /// Token liquidity in SOL
    Liquidity,
    /// Distribution of token holders
    HolderDistribution,
    /// Volume growth rate over time
    VolumeGrowth,
    /// Growth in number of holders
    HolderGrowth,
    /// Price change percentage
    PriceChange,
    /// Presence in Jito bundles
    JitoBundlePresence,
    /// Speed of creator selling
    CreatorSellSpeed,
    /// Quality of token metadata
    MetadataQuality,
    /// Social media activity
    SocialActivity,
}

impl Feature {
    /// Returns the string representation of the feature for serialization.
    pub fn as_str(&self) -> &'static str {
        match self {
            Feature::Liquidity => "liquidity",
            Feature::HolderDistribution => "holder_distribution",
            Feature::VolumeGrowth => "volume_growth",
            Feature::HolderGrowth => "holder_growth",
            Feature::PriceChange => "price_change",
            Feature::JitoBundlePresence => "jito_bundle_presence",
            Feature::CreatorSellSpeed => "creator_sell_speed",
            Feature::MetadataQuality => "metadata_quality",
            Feature::SocialActivity => "social_activity",
        }
    }

    /// Returns all available features.
    pub fn all() -> Vec<Feature> {
        vec![
            Feature::Liquidity,
            Feature::HolderDistribution,
            Feature::VolumeGrowth,
            Feature::HolderGrowth,
            Feature::PriceChange,
            Feature::JitoBundlePresence,
            Feature::CreatorSellSpeed,
            Feature::MetadataQuality,
            Feature::SocialActivity,
        ]
    }
}

/// Scored candidate with all required fields from the problem statement.
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

/// Extended Oracle configuration with all new fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleConfig {
    /// Feature weights for scoring
    pub weights: FeatureWeights,
    /// RPC endpoints
    pub rpc_endpoints: Vec<String>,
    /// Pump.fun API key
    pub pump_fun_api_key: Option<String>,
    /// Bitquery API key
    pub bitquery_api_key: Option<String>,
    /// Score thresholds
    pub thresholds: ScoreThresholds,
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

    // New fields for modular architecture
    /// Adaptive weights recalculation interval
    pub adaptive_recalc_interval: u64,
    /// Circuit breaker failure threshold
    pub circuit_breaker_failure_threshold: u32,
    /// Circuit breaker cooldown in seconds
    pub circuit_breaker_cooldown_seconds: u64,
    /// Endpoint success sample size
    pub endpoint_success_sample_size: usize,
    /// Adaptive error rate window size
    pub adaptive_error_rate_window: usize,
    /// Maximum cache entries
    pub max_cache_entries: usize,
    /// Metrics HTTP server listen address
    pub metrics_http_listen: Option<String>,
}

/// Feature weights for scoring algorithm.
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

/// Score thresholds for various features.
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

/// Oracle metrics for monitoring and observability.
#[derive(Debug, Default, Clone)]
pub struct OracleMetrics {
    pub total_scored: u64,
    pub avg_scoring_time: f64,
    pub high_score_count: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub rpc_errors: u64,
    pub api_errors: u64,
}

/// Token data structure containing all analyzed information.
#[derive(Debug, Clone)]
pub struct TokenData {
    pub supply: u64,
    pub decimals: u8,
    pub metadata_uri: String,
    pub metadata: Option<Metadata>,
    pub holder_distribution: Vec<HolderData>,
    pub liquidity_pool: Option<LiquidityPool>,
    pub volume_data: VolumeData,
    pub creator_holdings: CreatorHoldings,
    pub holder_history: VecDeque<usize>,
    pub price_history: VecDeque<f64>,
    pub social_activity: SocialActivity,
}

/// Token metadata structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metadata {
    pub name: String,
    pub symbol: String,
    pub description: String,
    pub image: String,
    pub attributes: Vec<Attribute>,
}

/// Metadata attribute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attribute {
    pub trait_type: String,
    pub value: String,
}

/// Holder data for distribution analysis.
#[derive(Debug, Clone)]
pub struct HolderData {
    pub address: Pubkey,
    pub percentage: f64,
    pub is_whale: bool,
}

/// Liquidity pool information.
#[derive(Debug, Clone)]
pub struct LiquidityPool {
    pub sol_amount: f64,
    pub token_amount: f64,
    pub pool_address: Pubkey,
    pub pool_type: PoolType,
}

/// Pool type enumeration.
#[derive(Debug, Clone)]
pub enum PoolType {
    Raydium,
    Orca,
    PumpFun,
    Unknown,
}

/// Volume data for transaction analysis.
#[derive(Debug, Clone)]
pub struct VolumeData {
    pub initial_volume: f64,
    pub current_volume: f64,
    pub volume_growth_rate: f64,
    pub transaction_count: u32,
    pub buy_sell_ratio: f64,
}

/// Creator holdings and sell activity.
#[derive(Debug, Clone)]
pub struct CreatorHoldings {
    pub initial_balance: u64,
    pub current_balance: u64,
    pub first_sell_timestamp: Option<u64>,
    pub sell_transactions: u32,
}

/// Social activity metrics.
#[derive(Debug, Clone)]
pub struct SocialActivity {
    pub twitter_mentions: u32,
    pub telegram_members: u32,
    pub discord_members: u32,
    pub social_score: f64,
}

/// Feature scores container using the Feature enum internally.
#[derive(Debug, Clone)]
pub struct FeatureScores {
    scores: [f64; 9], // Fixed array for performance
}

impl FeatureScores {
    /// Create new empty feature scores.
    pub fn new() -> Self {
        Self { scores: [0.0; 9] }
    }

    /// Set score for a feature.
    pub fn set(&mut self, feature: Feature, score: f64) {
        self.scores[feature as usize] = score;
    }

    /// Get score for a feature.
    pub fn get(&self, feature: Feature) -> f64 {
        self.scores[feature as usize]
    }

    /// Convert to HashMap for external API compatibility.
    pub fn to_hashmap(&self) -> HashMap<String, f64> {
        let mut map = HashMap::new();
        for feature in Feature::all() {
            map.insert(feature.as_str().to_string(), self.get(feature));
        }
        map
    }

    /// Create from HashMap (for backward compatibility).
    pub fn from_hashmap(map: &HashMap<String, f64>) -> Self {
        let mut scores = Self::new();
        for feature in Feature::all() {
            if let Some(&score) = map.get(feature.as_str()) {
                scores.set(feature, score);
            }
        }
        scores
    }
}

impl Default for FeatureScores {
    fn default() -> Self {
        Self::new()
    }
}

// Default implementations
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

impl Default for OracleConfig {
    fn default() -> Self {
        Self {
            weights: FeatureWeights::default(),
            rpc_endpoints: vec!["https://api.mainnet-beta.solana.com".to_string()],
            pump_fun_api_key: None,
            bitquery_api_key: None,
            thresholds: ScoreThresholds::default(),
            rpc_retry_attempts: 3,
            rpc_timeout_seconds: 10,
            cache_ttl_seconds: 300,
            max_parallel_requests: 10,
            rate_limit_requests_per_second: 20,
            notify_threshold: 75,
            // New default values
            adaptive_recalc_interval: 100,
            circuit_breaker_failure_threshold: 5,
            circuit_breaker_cooldown_seconds: 60,
            endpoint_success_sample_size: 50,
            adaptive_error_rate_window: 100,
            max_cache_entries: 1000,
            metrics_http_listen: None,
        }
    }
}
//! Oracle module - Universe-Class modular predictive oracle system.
//!
//! This module provides a modular, testable, and extensible oracle architecture
//! for token scoring and anomaly detection. The design follows clean architecture
//! principles with clear separation of concerns.

pub mod types;
pub mod features;
pub mod data_sources;
pub mod scorer;
pub mod anomaly;
pub mod weights;
pub mod metrics;
pub mod circuit_breaker;
pub mod rate_limit;
pub mod quantum_oracle;

// Re-export main public types and the primary oracle
pub use quantum_oracle::PredictiveOracle;
pub use types::{
    Feature, ScoredCandidate, OracleConfig, FeatureWeights, ScoreThresholds,
    OracleMetrics, TokenData, FeatureScores,
};

// Re-export other key components for advanced usage
pub use features::OracleFeatureComputer;
pub use scorer::OracleScorer;
pub use anomaly::AnomalyDetector;
pub use weights::AdaptiveWeights;
pub use circuit_breaker::{EndpointHealth, EndpointState};
pub use rate_limit::AdaptiveRateLimiter;

/// Oracle builder for convenient construction with sensible defaults.
pub struct OracleBuilder {
    config: OracleConfig,
}

impl OracleBuilder {
    /// Create a new builder with default configuration.
    pub fn new() -> Self {
        Self {
            config: OracleConfig::default(),
        }
    }

    /// Set the RPC endpoints.
    pub fn with_rpc_endpoints(mut self, endpoints: Vec<String>) -> Self {
        self.config.rpc_endpoints = endpoints;
        self
    }

    /// Set feature weights.
    pub fn with_weights(mut self, weights: FeatureWeights) -> Self {
        self.config.weights = weights;
        self
    }

    /// Set score thresholds.
    pub fn with_thresholds(mut self, thresholds: ScoreThresholds) -> Self {
        self.config.thresholds = thresholds;
        self
    }

    /// Set GUI notification threshold.
    pub fn with_notify_threshold(mut self, threshold: u8) -> Self {
        self.config.notify_threshold = threshold;
        self
    }

    /// Set cache TTL in seconds.
    pub fn with_cache_ttl(mut self, ttl_seconds: u64) -> Self {
        self.config.cache_ttl_seconds = ttl_seconds;
        self
    }

    /// Set max cache entries.
    pub fn with_max_cache_entries(mut self, max_entries: usize) -> Self {
        self.config.max_cache_entries = max_entries;
        self
    }

    /// Set rate limiting.
    pub fn with_rate_limit(mut self, requests_per_second: u32) -> Self {
        self.config.rate_limit_requests_per_second = requests_per_second;
        self
    }

    /// Set max parallel requests.
    pub fn with_max_parallel_requests(mut self, max_requests: usize) -> Self {
        self.config.max_parallel_requests = max_requests;
        self
    }

    /// Set circuit breaker configuration.
    pub fn with_circuit_breaker(
        mut self,
        failure_threshold: u32,
        cooldown_seconds: u64,
    ) -> Self {
        self.config.circuit_breaker_failure_threshold = failure_threshold;
        self.config.circuit_breaker_cooldown_seconds = cooldown_seconds;
        self
    }

    /// Set adaptive weights recalculation interval.
    pub fn with_adaptive_interval(mut self, interval: u64) -> Self {
        self.config.adaptive_recalc_interval = interval;
        self
    }

    /// Enable metrics HTTP server.
    pub fn with_metrics_server(mut self, listen_addr: String) -> Self {
        self.config.metrics_http_listen = Some(listen_addr);
        self
    }

    /// Set API keys.
    pub fn with_api_keys(
        mut self,
        pump_fun_key: Option<String>,
        bitquery_key: Option<String>,
    ) -> Self {
        self.config.pump_fun_api_key = pump_fun_key;
        self.config.bitquery_api_key = bitquery_key;
        self
    }

    /// Build the oracle configuration.
    pub fn build_config(self) -> OracleConfig {
        self.config
    }

    /// Build the oracle instance (requires channels).
    pub fn build(
        self,
        candidate_receiver: tokio::sync::mpsc::Receiver<crate::types::PremintCandidate>,
        scored_sender: tokio::sync::mpsc::Sender<ScoredCandidate>,
    ) -> anyhow::Result<PredictiveOracle> {
        PredictiveOracle::new(candidate_receiver, scored_sender, self.config)
    }
}

impl Default for OracleBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_oracle_builder() {
        let config = OracleBuilder::new()
            .with_notify_threshold(80)
            .with_cache_ttl(600)
            .with_rate_limit(50)
            .build_config();

        assert_eq!(config.notify_threshold, 80);
        assert_eq!(config.cache_ttl_seconds, 600);
        assert_eq!(config.rate_limit_requests_per_second, 50);
    }

    #[test]
    fn test_oracle_builder_defaults() {
        let config = OracleBuilder::new().build_config();
        
        assert_eq!(config.notify_threshold, 75);
        assert_eq!(config.cache_ttl_seconds, 300);
        assert_eq!(config.rate_limit_requests_per_second, 20);
        assert_eq!(config.adaptive_recalc_interval, 100);
        assert_eq!(config.circuit_breaker_failure_threshold, 5);
    }
}
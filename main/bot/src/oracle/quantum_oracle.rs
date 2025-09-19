//! Main Oracle orchestrator - Universe-Class predictive oracle implementation.
//!
//! This module contains the main PredictiveOracle that orchestrates all the 
//! modular components to provide comprehensive token scoring.

use crate::oracle::types::{ScoredCandidate, OracleConfig, OracleMetrics};
use crate::oracle::scorer::OracleScorer;
use crate::oracle::metrics::OracleMetricsCollector;
use crate::oracle::circuit_breaker::CircuitBreaker;
use crate::oracle::rate_limit::AdaptiveRateLimiter;
use crate::types::{PremintCandidate, QuantumCandidateGui};
use anyhow::{anyhow, Result};
use moka::future::Cache;
use reqwest::Client;
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, Semaphore};
use tracing::{debug, error, info, warn, instrument};

/// Main Universe-Class Predictive Oracle.
pub struct PredictiveOracle {
    pub candidate_receiver: mpsc::Receiver<PremintCandidate>,
    pub scored_sender: mpsc::Sender<ScoredCandidate>,
    pub gui_suggestions: Arc<Mutex<Option<mpsc::Sender<QuantumCandidateGui>>>>,
    pub config: OracleConfig,
    
    // Core components
    scorer: OracleScorer,
    metrics_collector: Arc<OracleMetricsCollector>,
    circuit_breaker: Arc<Mutex<CircuitBreaker>>,
    rate_limiter: Arc<Mutex<AdaptiveRateLimiter>>,
    
    // Infrastructure
    token_cache: Cache<String, (Instant, String)>, // Simplified cache for now
    request_semaphore: Arc<Semaphore>,
    
    // State tracking
    scored_history: Arc<Mutex<Vec<ScoredCandidate>>>,
    adaptive_recalc_counter: Arc<Mutex<u64>>,
}

impl PredictiveOracle {
    /// Create a new Universe-Class Predictive Oracle.
    pub fn new(
        candidate_receiver: mpsc::Receiver<PremintCandidate>,
        scored_sender: mpsc::Sender<ScoredCandidate>,
        config: OracleConfig,
    ) -> Result<Self> {
        // Validate configuration
        if config.rpc_endpoints.is_empty() {
            return Err(anyhow!("At least one RPC endpoint is required"));
        }

        // Create RPC clients
        let rpc_clients: Vec<Arc<RpcClient>> = config
            .rpc_endpoints
            .iter()
            .map(|endpoint| {
                Arc::new(RpcClient::new_with_timeout(
                    endpoint.clone(),
                    Duration::from_secs(config.rpc_timeout_seconds),
                ))
            })
            .collect();

        // Create HTTP client
        let http_client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;

        // Create GUI suggestions channel
        let gui_suggestions = Arc::new(Mutex::new(None));

        // Create scorer
        let scorer = OracleScorer::new(
            scored_sender.clone(),
            gui_suggestions.clone(),
            rpc_clients,
            http_client,
            config.clone(),
        );

        // Create supporting components
        let metrics_collector = Arc::new(OracleMetricsCollector::new());
        
        let circuit_breaker = Arc::new(Mutex::new(CircuitBreaker::new(
            config.circuit_breaker_failure_threshold,
            config.circuit_breaker_cooldown_seconds,
            config.endpoint_success_sample_size,
        )));

        let rate_limiter = Arc::new(Mutex::new(AdaptiveRateLimiter::new(
            config.rate_limit_requests_per_second,
            config.adaptive_error_rate_window,
            0.2, // 20% error threshold
        )));

        // Create cache
        let token_cache = Cache::builder()
            .max_capacity(config.max_cache_entries as u64)
            .time_to_live(Duration::from_secs(config.cache_ttl_seconds))
            .build();

        let request_semaphore = Arc::new(Semaphore::new(config.max_parallel_requests));

        info!("Created Universe-Class Predictive Oracle with {} RPC endpoints", config.rpc_endpoints.len());

        Ok(Self {
            candidate_receiver,
            scored_sender,
            gui_suggestions,
            config,
            scorer,
            metrics_collector,
            circuit_breaker,
            rate_limiter,
            token_cache,
            request_semaphore,
            scored_history: Arc::new(Mutex::new(Vec::new())),
            adaptive_recalc_counter: Arc::new(Mutex::new(0)),
        })
    }

    /// Set GUI sender for notifications.
    pub fn set_gui_sender(&self, sender: mpsc::Sender<QuantumCandidateGui>) {
        tokio::spawn({
            let gui_suggestions = self.gui_suggestions.clone();
            async move {
                let mut gui_lock = gui_suggestions.lock().await;
                *gui_lock = Some(sender);
            }
        });
    }

    /// Main execution loop.
    #[instrument(skip(self))]
    pub async fn run(&mut self) {
        info!("Starting Universe-Class Predictive Oracle main loop");

        // Start metrics server if configured
        if let Some(metrics_addr) = &self.config.metrics_http_listen {
            let metrics_collector = self.metrics_collector.clone();
            let addr = metrics_addr.clone();
            tokio::spawn(async move {
                if let Err(e) = metrics_collector.start_metrics_server(&addr).await {
                    warn!("Failed to start metrics server: {}", e);
                }
            });
        }

        while let Some(candidate) = self.candidate_receiver.recv().await {
            // Acquire rate limit and semaphore
            let permit = self.request_semaphore.clone().acquire_owned().await;
            
            let rate_check = {
                let mut limiter = self.rate_limiter.lock().await;
                limiter.check_and_record_request().await
            };

            if rate_check.is_err() {
                debug!("Rate limit exceeded, skipping candidate {}", candidate.mint);
                continue;
            }

            // Process candidate asynchronously
            let scorer = self.scorer.clone();
            let metrics_collector = self.metrics_collector.clone();
            let circuit_breaker = self.circuit_breaker.clone();
            let rate_limiter = self.rate_limiter.clone();
            let scored_history = self.scored_history.clone();
            let adaptive_counter = self.adaptive_recalc_counter.clone();
            let adaptive_interval = self.config.adaptive_recalc_interval;

            tokio::spawn(async move {
                let start_time = Instant::now();

                match Self::process_candidate(
                    &scorer,
                    &candidate,
                    &metrics_collector,
                    &circuit_breaker,
                    &rate_limiter,
                ).await {
                    Ok(scored) => {
                        // Record metrics
                        let scoring_duration = start_time.elapsed();
                        metrics_collector.record_scoring_time(scoring_duration).await;
                        metrics_collector.increment_counter("oracle_scored_total").await;

                        if scored.predicted_score >= 80 {
                            metrics_collector.increment_counter("oracle_high_score_total").await;
                        }

                        // Send GUI notification
                        scorer.send_gui_notification(&scored).await;

                        // Add to history for adaptive weights
                        {
                            let mut history = scored_history.lock().await;
                            history.push(scored.clone());
                            
                            // Keep history bounded
                            if history.len() > 1000 {
                                history.drain(0..500); // Remove older half
                            }
                        }

                        // Check if we should recalculate adaptive weights
                        {
                            let mut counter = adaptive_counter.lock().await;
                            *counter += 1;
                            
                            if *counter >= adaptive_interval {
                                *counter = 0;
                                drop(counter);
                                
                                // Trigger weight recalculation
                                let history = scored_history.lock().await;
                                if !history.is_empty() {
                                    scorer.update_adaptive_weights(&history).await;
                                    info!("Updated adaptive weights after {} scored candidates", adaptive_interval);
                                }
                            }
                        }

                        info!("Successfully scored candidate {} with score {} in {}μs",
                              candidate.mint, scored.predicted_score, scoring_duration.as_micros());
                    }
                    Err(e) => {
                        warn!("Failed to score candidate {}: {}", candidate.mint, e);
                        
                        // Record failure for rate limiting
                        let mut limiter = rate_limiter.lock().await;
                        limiter.record_failure();
                    }
                }

                drop(permit);
            });
        }

        info!("Oracle main loop ended");
    }

    /// Process a single candidate.
    async fn process_candidate(
        scorer: &OracleScorer,
        candidate: &PremintCandidate,
        metrics_collector: &OracleMetricsCollector,
        circuit_breaker: &Arc<Mutex<CircuitBreaker>>,
        rate_limiter: &Arc<Mutex<AdaptiveRateLimiter>>,
    ) -> Result<ScoredCandidate> {
        debug!("Processing candidate: {}", candidate.mint);

        // Score the candidate
        let scored = scorer.score_candidate(candidate).await?;

        // Record success for circuit breaker and rate limiter
        {
            let mut cb = circuit_breaker.lock().await;
            for endpoint in &scorer.config.rpc_endpoints {
                cb.record_success(endpoint);
            }
        }

        {
            let mut limiter = rate_limiter.lock().await;
            limiter.record_success();
        }

        Ok(scored)
    }

    /// Get current Oracle metrics.
    #[instrument(skip(self))]
    pub async fn get_metrics(&self) -> OracleMetrics {
        let snapshot = self.metrics_collector.get_metrics_snapshot().await;
        
        OracleMetrics {
            total_scored: snapshot.counters.get("oracle_scored_total").unwrap_or(&0).clone(),
            avg_scoring_time: snapshot.gauges.get("oracle_avg_scoring_time_seconds").unwrap_or(&0.0).clone(),
            high_score_count: snapshot.counters.get("oracle_high_score_total").unwrap_or(&0).clone(),
            cache_hits: snapshot.counters.get("oracle_cache_hits_total").unwrap_or(&0).clone(),
            cache_misses: snapshot.counters.get("oracle_cache_misses_total").unwrap_or(&0).clone(),
            rpc_errors: snapshot.counters.get("oracle_rpc_errors_total").unwrap_or(&0).clone(),
            api_errors: snapshot.counters.get("oracle_api_errors_total").unwrap_or(&0).clone(),
        }
    }

    /// Clear the token cache.
    pub async fn clear_cache(&self) {
        self.token_cache.invalidate_all().await;
        info!("Cleared Oracle token cache");
    }

    /// Get current cache size.
    pub async fn get_cache_size(&self) -> usize {
        self.token_cache.entry_count() as usize
    }

    /// Graceful shutdown.
    #[instrument(skip(self))]
    pub async fn shutdown(&self) {
        info!("Initiating Oracle graceful shutdown");

        // Flush metrics snapshot
        let snapshot = self.metrics_collector.get_metrics_snapshot().await;
        info!("Final metrics - Scored: {}, High scores: {}, Cache hits: {}", 
              snapshot.counters.get("oracle_scored_total").unwrap_or(&0),
              snapshot.counters.get("oracle_high_score_total").unwrap_or(&0),
              snapshot.counters.get("oracle_cache_hits_total").unwrap_or(&0));

        // Clear cache
        self.token_cache.invalidate_all().await;

        info!("Oracle shutdown complete");
    }
}

// Make sure we don't accidentally implement Clone
// (PredictiveOracle should not be cloneable due to the receiver)
// The comment from original code explains this is intentional

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::types::OracleConfig;
    use crate::types::PremintCandidate;
    use solana_sdk::pubkey::Pubkey;
    use tokio::sync::mpsc;

    fn create_test_config() -> OracleConfig {
        OracleConfig {
            rpc_endpoints: vec!["https://api.mainnet-beta.solana.com".to_string()],
            ..OracleConfig::default()
        }
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

    #[tokio::test]
    async fn test_oracle_creation() {
        let (candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let config = create_test_config();

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config);
        assert!(oracle.is_ok());
    }

    #[tokio::test]
    async fn test_oracle_creation_empty_rpc_endpoints() {
        let (candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let mut config = create_test_config();
        config.rpc_endpoints.clear();

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config);
        assert!(oracle.is_err());
    }

    #[tokio::test]
    async fn test_get_metrics() {
        let (candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let config = create_test_config();

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config).unwrap();
        let metrics = oracle.get_metrics().await;

        // Should have default values
        assert_eq!(metrics.total_scored, 0);
        assert_eq!(metrics.high_score_count, 0);
    }

    #[tokio::test]
    async fn test_cache_operations() {
        let (candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let config = create_test_config();

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config).unwrap();
        
        // Initially empty
        assert_eq!(oracle.get_cache_size().await, 0);
        
        // Clear should not panic
        oracle.clear_cache().await;
        assert_eq!(oracle.get_cache_size().await, 0);
    }

    #[tokio::test]
    async fn test_set_gui_sender() {
        let (candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let (gui_tx, _gui_rx) = mpsc::channel(10);
        let config = create_test_config();

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config).unwrap();
        
        // Should not panic
        oracle.set_gui_sender(gui_tx);
        
        // Give a moment for the spawn to complete
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    #[tokio::test]
    async fn test_shutdown() {
        let (candidate_tx, candidate_rx) = mpsc::channel(10);
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let config = create_test_config();

        let oracle = PredictiveOracle::new(candidate_rx, scored_tx, config).unwrap();
        
        // Should not panic
        oracle.shutdown().await;
    }
}
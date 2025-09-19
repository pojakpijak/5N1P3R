//! Oracle scorer - combines feature scores into final predictions.
//!
//! This module orchestrates the scoring process by combining feature computation,
//! anomaly detection, and weighting to produce final candidate scores.

use crate::oracle::types::{
    ScoredCandidate, OracleConfig, TokenData, FeatureScores, Feature, FeatureWeights,
};
use crate::oracle::features::OracleFeatureComputer;
use crate::oracle::data_sources::OracleDataSources;
use crate::oracle::anomaly::AnomalyDetector;
use crate::oracle::weights::AdaptiveWeights;
use crate::types::{PremintCandidate, QuantumCandidateGui};
use anyhow::Result;
use reqwest::Client;
use solana_client::nonblocking::rpc_client::RpcClient;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, info, warn, instrument};

/// Oracle scorer that combines all scoring components.
#[derive(Clone)]
pub struct OracleScorer {
    pub scored_sender: mpsc::Sender<ScoredCandidate>,
    pub gui_suggestions: Arc<Mutex<Option<mpsc::Sender<QuantumCandidateGui>>>>,
    pub config: OracleConfig,
    pub data_sources: Arc<OracleDataSources>,
    pub feature_computer: Arc<OracleFeatureComputer>,
    pub anomaly_detector: Arc<AnomalyDetector>,
    pub adaptive_weights: Arc<Mutex<AdaptiveWeights>>,
}

impl OracleScorer {
    /// Create a new oracle scorer.
    pub fn new(
        scored_sender: mpsc::Sender<ScoredCandidate>,
        gui_suggestions: Arc<Mutex<Option<mpsc::Sender<QuantumCandidateGui>>>>,
        rpc_clients: Vec<Arc<RpcClient>>,
        http_client: Client,
        config: OracleConfig,
    ) -> Self {
        let data_sources = Arc::new(OracleDataSources::new(
            rpc_clients,
            http_client,
            config.clone(),
        ));
        
        let feature_computer = Arc::new(OracleFeatureComputer::new(config.clone()));
        let anomaly_detector = Arc::new(AnomalyDetector::new(config.clone()));
        let adaptive_weights = Arc::new(Mutex::new(AdaptiveWeights::new(config.weights.clone())));

        Self {
            scored_sender,
            gui_suggestions,
            config,
            data_sources,
            feature_computer,
            anomaly_detector,
            adaptive_weights,
        }
    }

    /// Score a candidate token.
    #[instrument(skip(self), fields(mint = %candidate.mint))]
    pub async fn score_candidate(&self, candidate: &PremintCandidate) -> Result<ScoredCandidate> {
        let start_time = Instant::now();
        
        debug!("Starting to score candidate: {}", candidate.mint);

        // Fetch token data from multiple sources
        let token_data = self.data_sources
            .fetch_token_data_with_retries(candidate)
            .await?;

        // Compute feature scores
        let feature_scores = self.feature_computer
            .compute_all_features(candidate, &token_data)
            .await?;

        // Detect anomalies
        let anomaly_detected = self.anomaly_detector
            .detect_anomalies(&token_data)
            .await;

        // Calculate weighted final score
        let predicted_score = self.calculate_predicted_score(&feature_scores).await?;

        // Apply anomaly penalty if detected
        let final_score = if anomaly_detected {
            debug!("Anomaly detected, applying penalty");
            (predicted_score as f64 * 0.5) as u8 // 50% penalty for anomalies
        } else {
            predicted_score
        };

        // Generate explanation
        let reason = self.generate_reason(&feature_scores, final_score, anomaly_detected);

        // Create scored candidate
        let scored = ScoredCandidate {
            base: candidate.clone(),
            mint: candidate.mint,
            predicted_score: final_score,
            feature_scores: feature_scores.to_hashmap(),
            reason,
            calculation_time: start_time.elapsed().as_micros(),
            anomaly_detected,
            timestamp: candidate.timestamp,
        };

        info!("Scored candidate {} with score {} in {}μs", 
              candidate.mint, final_score, scored.calculation_time);

        Ok(scored)
    }

    /// Calculate the predicted score using adaptive weights.
    #[instrument(skip(self, feature_scores))]
    async fn calculate_predicted_score(&self, feature_scores: &FeatureScores) -> Result<u8> {
        let weights = self.adaptive_weights.lock().await;
        let effective_weights = weights.get_effective_weights();

        let mut weighted_sum = 0.0;
        let mut total_weight = 0.0;

        // Calculate weighted sum of all features
        for feature in Feature::all() {
            let score = feature_scores.get(feature);
            let weight = self.get_feature_weight(&effective_weights, feature);
            
            weighted_sum += score * weight;
            total_weight += weight;
        }

        // Normalize to 0-100 scale
        let normalized_score = if total_weight > 0.0 {
            (weighted_sum / total_weight * 100.0).round() as u8
        } else {
            50 // Default score if no weights
        };

        debug!("Calculated weighted score: {}/100 (sum={:.3}, weight={:.3})", 
               normalized_score, weighted_sum, total_weight);

        Ok(normalized_score.min(100))
    }

    /// Get weight for a specific feature.
    fn get_feature_weight(&self, weights: &FeatureWeights, feature: Feature) -> f64 {
        match feature {
            Feature::Liquidity => weights.liquidity,
            Feature::HolderDistribution => weights.holder_distribution,
            Feature::VolumeGrowth => weights.volume_growth,
            Feature::HolderGrowth => weights.holder_growth,
            Feature::PriceChange => weights.price_change,
            Feature::JitoBundlePresence => weights.jito_bundle_presence,
            Feature::CreatorSellSpeed => weights.creator_sell_speed,
            Feature::MetadataQuality => weights.metadata_quality,
            Feature::SocialActivity => weights.social_activity,
        }
    }

    /// Generate human-readable reason for the score.
    #[instrument(skip(self, feature_scores))]
    fn generate_reason(
        &self,
        feature_scores: &FeatureScores,
        score: u8,
        anomaly_detected: bool,
    ) -> String {
        let mut reasons = Vec::new();

        // Analyze strongest features
        let mut feature_impacts: Vec<(Feature, f64, f64)> = Feature::all()
            .into_iter()
            .map(|f| {
                let score = feature_scores.get(f);
                let weight = self.get_feature_weight(&self.config.weights, f);
                (f, score, score * weight)
            })
            .collect();

        // Sort by impact (score * weight)
        feature_impacts.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

        // Add top positive factors
        for (feature, score, _impact) in feature_impacts.iter().take(3) {
            if *score > 0.6 {
                let feature_name = format!("{:?}", feature).to_lowercase().replace("_", " ");
                reasons.push(format!("strong {}", feature_name));
            }
        }

        // Add top negative factors
        for (feature, score, _impact) in feature_impacts.iter().rev().take(2) {
            if *score < 0.4 {
                let feature_name = format!("{:?}", feature).to_lowercase().replace("_", " ");
                reasons.push(format!("weak {}", feature_name));
            }
        }

        // Add anomaly warning
        if anomaly_detected {
            reasons.push("anomaly detected".to_string());
        }

        // Create final reason string
        let reason = if reasons.is_empty() {
            format!("Moderate score of {}/100 based on balanced factors", score)
        } else {
            format!("Score {}/100: {}", score, reasons.join(", "))
        };

        debug!("Generated reason: {}", reason);
        reason
    }

    /// Update adaptive weights with historical data.
    #[instrument(skip(self, historical_scores))]
    pub async fn update_adaptive_weights(&self, historical_scores: &[ScoredCandidate]) {
        if historical_scores.is_empty() {
            return;
        }

        let mut weights = self.adaptive_weights.lock().await;
        weights.recalculate(historical_scores);
        
        debug!("Updated adaptive weights with {} historical scores", historical_scores.len());
    }

    /// Send GUI notification if score meets threshold.
    #[instrument(skip(self, scored))]
    pub async fn send_gui_notification(&self, scored: &ScoredCandidate) {
        if scored.predicted_score >= self.config.notify_threshold {
            let gui_suggestion = QuantumCandidateGui {
                mint: scored.mint,
                score: scored.predicted_score,
                reason: scored.reason.clone(),
                feature_scores: scored.feature_scores.clone(),
                timestamp: scored.timestamp,
            };

            if let Some(sender) = self.gui_suggestions.lock().await.as_ref() {
                if let Err(e) = sender.send(gui_suggestion).await {
                    warn!("Failed to send GUI suggestion: {}", e);
                } else {
                    debug!("Sent GUI notification for score {}", scored.predicted_score);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::types::*;
    use crate::types::PremintCandidate;
    use solana_sdk::pubkey::Pubkey;
    use std::collections::VecDeque;
    use tokio::sync::mpsc;

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

    #[tokio::test]
    async fn test_oracle_scorer_creation() {
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let gui_suggestions = Arc::new(Mutex::new(None));
        let config = create_test_config();
        let http_client = Client::new();
        let rpc_clients = vec![];

        let scorer = OracleScorer::new(
            scored_tx,
            gui_suggestions,
            rpc_clients,
            http_client,
            config.clone(),
        );

        assert_eq!(scorer.config.notify_threshold, config.notify_threshold);
    }

    #[tokio::test]
    async fn test_calculate_predicted_score() {
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let gui_suggestions = Arc::new(Mutex::new(None));
        let config = create_test_config();
        let http_client = Client::new();
        let rpc_clients = vec![];

        let scorer = OracleScorer::new(
            scored_tx,
            gui_suggestions,
            rpc_clients,
            http_client,
            config,
        );

        let mut feature_scores = FeatureScores::new();
        feature_scores.set(Feature::Liquidity, 0.8);
        feature_scores.set(Feature::VolumeGrowth, 0.7);
        feature_scores.set(Feature::MetadataQuality, 0.9);

        let score = scorer.calculate_predicted_score(&feature_scores).await.unwrap();
        
        // Score should be between 0 and 100
        assert!(score <= 100);
        // With decent feature scores, should be above 50
        assert!(score > 50);
    }

    #[test]
    fn test_generate_reason() {
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let gui_suggestions = Arc::new(Mutex::new(None));
        let config = create_test_config();
        let http_client = Client::new();
        let rpc_clients = vec![];

        let scorer = OracleScorer::new(
            scored_tx,
            gui_suggestions,
            rpc_clients,
            http_client,
            config,
        );

        let mut feature_scores = FeatureScores::new();
        feature_scores.set(Feature::Liquidity, 0.9);
        feature_scores.set(Feature::VolumeGrowth, 0.8);
        feature_scores.set(Feature::HolderDistribution, 0.2);

        let reason = scorer.generate_reason(&feature_scores, 75, false);
        
        assert!(reason.contains("75"));
        assert!(reason.len() > 10); // Should be a meaningful explanation
    }

    #[test]
    fn test_generate_reason_with_anomaly() {
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let gui_suggestions = Arc::new(Mutex::new(None));
        let config = create_test_config();
        let http_client = Client::new();
        let rpc_clients = vec![];

        let scorer = OracleScorer::new(
            scored_tx,
            gui_suggestions,
            rpc_clients,
            http_client,
            config,
        );

        let feature_scores = FeatureScores::new();
        let reason = scorer.generate_reason(&feature_scores, 60, true);
        
        assert!(reason.contains("anomaly detected"));
    }

    #[test]
    fn test_get_feature_weight() {
        let (scored_tx, _scored_rx) = mpsc::channel(10);
        let gui_suggestions = Arc::new(Mutex::new(None));
        let config = create_test_config();
        let http_client = Client::new();
        let rpc_clients = vec![];

        let scorer = OracleScorer::new(
            scored_tx,
            gui_suggestions,
            rpc_clients,
            http_client,
            config.clone(),
        );

        let weight = scorer.get_feature_weight(&config.weights, Feature::Liquidity);
        assert_eq!(weight, config.weights.liquidity);

        let weight = scorer.get_feature_weight(&config.weights, Feature::VolumeGrowth);
        assert_eq!(weight, config.weights.volume_growth);
    }
}
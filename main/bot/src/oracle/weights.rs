//! Adaptive weights system for dynamic feature importance adjustment.
//!
//! This module implements an adaptive weighting system that can adjust feature
//! weights based on historical performance and market conditions.

use crate::oracle::types::{FeatureWeights, ScoredCandidate, Feature};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, info, instrument};

/// Adaptive weights manager that wraps static weights with dynamic adjustments.
pub struct AdaptiveWeights {
    /// Base feature weights from configuration
    base_weights: FeatureWeights,
    /// Dynamic weight adjustments per feature
    weight_adjustments: HashMap<Feature, f64>,
    /// Historical performance tracking per feature
    feature_performance: HashMap<Feature, FeaturePerformance>,
    /// Last recalculation timestamp
    last_recalculation: u64,
    /// Number of recalculations performed
    recalculation_count: u64,
    /// Adaptation rate (how quickly to adjust weights)
    adaptation_rate: f64,
}

/// Performance tracking for individual features.
#[derive(Debug, Clone)]
struct FeaturePerformance {
    /// Rolling window of feature scores for successful predictions (score >= 80)
    successful_scores: Vec<f64>,
    /// Rolling window of feature scores for failed predictions (score < 50)
    failed_scores: Vec<f64>,
    /// Variance of feature scores
    score_variance: f64,
    /// Correlation with final scores
    correlation_with_outcome: f64,
    /// Recent effectiveness score (0.0-1.0)
    effectiveness: f64,
}

impl AdaptiveWeights {
    /// Create new adaptive weights with base configuration.
    pub fn new(base_weights: FeatureWeights) -> Self {
        let mut feature_performance = HashMap::new();
        
        // Initialize performance tracking for all features
        for feature in Feature::all() {
            feature_performance.insert(feature, FeaturePerformance::default());
        }

        Self {
            base_weights,
            weight_adjustments: HashMap::new(),
            feature_performance,
            last_recalculation: current_timestamp(),
            recalculation_count: 0,
            adaptation_rate: 0.1, // 10% adaptation rate
        }
    }

    /// Get effective weights (base + adjustments).
    #[instrument(skip(self))]
    pub fn get_effective_weights(&self) -> FeatureWeights {
        let mut effective = self.base_weights.clone();

        // Apply dynamic adjustments
        effective.liquidity = self.apply_adjustment(effective.liquidity, Feature::Liquidity);
        effective.holder_distribution = self.apply_adjustment(effective.holder_distribution, Feature::HolderDistribution);
        effective.volume_growth = self.apply_adjustment(effective.volume_growth, Feature::VolumeGrowth);
        effective.holder_growth = self.apply_adjustment(effective.holder_growth, Feature::HolderGrowth);
        effective.price_change = self.apply_adjustment(effective.price_change, Feature::PriceChange);
        effective.jito_bundle_presence = self.apply_adjustment(effective.jito_bundle_presence, Feature::JitoBundlePresence);
        effective.creator_sell_speed = self.apply_adjustment(effective.creator_sell_speed, Feature::CreatorSellSpeed);
        effective.metadata_quality = self.apply_adjustment(effective.metadata_quality, Feature::MetadataQuality);
        effective.social_activity = self.apply_adjustment(effective.social_activity, Feature::SocialActivity);

        debug!("Applied adaptive weight adjustments");
        effective
    }

    /// Apply weight adjustment to a base weight.
    fn apply_adjustment(&self, base_weight: f64, feature: Feature) -> f64 {
        let adjustment = self.weight_adjustments.get(&feature).unwrap_or(&0.0);
        (base_weight * (1.0 + adjustment)).max(0.01).min(1.0) // Keep within reasonable bounds
    }

    /// Recalculate weights based on historical performance.
    #[instrument(skip(self, historical_scores))]
    pub fn recalculate(&mut self, historical_scores: &[ScoredCandidate]) {
        if historical_scores.is_empty() {
            debug!("No historical scores provided for recalculation");
            return;
        }

        info!("Recalculating adaptive weights with {} historical scores", historical_scores.len());

        // Update feature performance tracking
        self.update_feature_performance(historical_scores);

        // Calculate new weight adjustments
        self.calculate_weight_adjustments();

        // Update tracking
        self.last_recalculation = current_timestamp();
        self.recalculation_count += 1;

        info!("Completed weight recalculation #{}", self.recalculation_count);
    }

    /// Update performance tracking for all features.
    #[instrument(skip(self, historical_scores))]
    fn update_feature_performance(&mut self, historical_scores: &[ScoredCandidate]) {
        // Separate successful and failed predictions
        let successful: Vec<_> = historical_scores
            .iter()
            .filter(|s| s.predicted_score >= 80 && !s.anomaly_detected)
            .collect();

        let failed: Vec<_> = historical_scores
            .iter()
            .filter(|s| s.predicted_score < 50 || s.anomaly_detected)
            .collect();

        // Update performance for each feature
        for feature in Feature::all() {
            let correlation = self.calculate_correlation(feature, historical_scores);
            
            let performance = self.feature_performance.get_mut(&feature).unwrap();
            
            // Update successful scores
            performance.successful_scores.clear();
            for candidate in &successful {
                if let Some(&score) = candidate.feature_scores.get(feature.as_str()) {
                    performance.successful_scores.push(score);
                }
            }

            // Update failed scores
            performance.failed_scores.clear();
            for candidate in &failed {
                if let Some(&score) = candidate.feature_scores.get(feature.as_str()) {
                    performance.failed_scores.push(score);
                }
            }

            // Calculate variance and effectiveness
            let successful_scores = &performance.successful_scores;
            let variance = Self::calculate_variance_static(successful_scores);
            let effectiveness = Self::calculate_effectiveness_static(performance);
            
            performance.score_variance = variance;
            performance.effectiveness = effectiveness;
            performance.correlation_with_outcome = correlation;

            debug!("Updated performance for {:?}: effectiveness={:.3}, variance={:.3}, correlation={:.3}",
                   feature, effectiveness, variance, correlation);
        }
    }

    /// Calculate weight adjustments based on feature performance.
    #[instrument(skip(self))]
    fn calculate_weight_adjustments(&mut self) {
        let mut new_adjustments = HashMap::new();
        
        for feature in Feature::all() {
            let performance = self.feature_performance.get(&feature).unwrap();
            
            // Calculate adjustment based on multiple factors
            let effectiveness_factor = (performance.effectiveness - 0.5) * 2.0; // -1.0 to 1.0
            let correlation_factor = performance.correlation_with_outcome; // Already -1.0 to 1.0
            let variance_factor = (0.5 - performance.score_variance).max(-0.5); // Prefer lower variance
            
            // Combined adjustment factor
            let adjustment = self.adaptation_rate * (
                effectiveness_factor * 0.4 +
                correlation_factor * 0.4 +
                variance_factor * 0.2
            );

            // Apply adjustment with dampening for stability
            let current_adjustment = self.weight_adjustments.get(&feature).unwrap_or(&0.0);
            let new_adjustment = current_adjustment * 0.8 + adjustment * 0.2; // Smooth adjustment
            
            new_adjustments.insert(feature, new_adjustment.clamp(-0.5, 0.5));

            debug!("Weight adjustment for {:?}: {:.3} -> {:.3} (factors: eff={:.3}, corr={:.3}, var={:.3})",
                   feature, current_adjustment, new_adjustment, effectiveness_factor, correlation_factor, variance_factor);
        }
        
        // Update all adjustments at once
        self.weight_adjustments = new_adjustments;
    }

    /// Calculate variance of a score vector.
    fn calculate_variance(&self, scores: &[f64]) -> f64 {
        Self::calculate_variance_static(scores)
    }

    /// Calculate variance of a score vector (static version).
    fn calculate_variance_static(scores: &[f64]) -> f64 {
        if scores.len() < 2 {
            return 0.0;
        }

        let mean = scores.iter().sum::<f64>() / scores.len() as f64;
        let variance = scores
            .iter()
            .map(|&score| (score - mean).powi(2))
            .sum::<f64>() / scores.len() as f64;

        variance.sqrt() // Return standard deviation
    }

    /// Calculate effectiveness of a feature.
    fn calculate_effectiveness(&self, performance: &FeaturePerformance) -> f64 {
        Self::calculate_effectiveness_static(performance)
    }

    /// Calculate effectiveness of a feature (static version).
    fn calculate_effectiveness_static(performance: &FeaturePerformance) -> f64 {
        let successful_mean = if performance.successful_scores.is_empty() {
            0.5
        } else {
            performance.successful_scores.iter().sum::<f64>() / performance.successful_scores.len() as f64
        };

        let failed_mean = if performance.failed_scores.is_empty() {
            0.5
        } else {
            performance.failed_scores.iter().sum::<f64>() / performance.failed_scores.len() as f64
        };

        // Effectiveness is how well the feature discriminates between success and failure
        let discrimination = successful_mean - failed_mean;
        (0.5 + discrimination).clamp(0.0, 1.0)
    }

    /// Calculate correlation between feature scores and final outcomes.
    fn calculate_correlation(&self, feature: Feature, historical_scores: &[ScoredCandidate]) -> f64 {
        if historical_scores.len() < 2 {
            return 0.0;
        }

        let mut feature_scores = Vec::new();
        let mut final_scores = Vec::new();

        for candidate in historical_scores {
            if let Some(&score) = candidate.feature_scores.get(feature.as_str()) {
                feature_scores.push(score);
                final_scores.push(candidate.predicted_score as f64 / 100.0);
            }
        }

        if feature_scores.len() < 2 {
            return 0.0;
        }

        // Calculate Pearson correlation coefficient
        let n = feature_scores.len() as f64;
        let sum_x = feature_scores.iter().sum::<f64>();
        let sum_y = final_scores.iter().sum::<f64>();
        let sum_xy = feature_scores.iter().zip(&final_scores).map(|(x, y)| x * y).sum::<f64>();
        let sum_x2 = feature_scores.iter().map(|x| x * x).sum::<f64>();
        let sum_y2 = final_scores.iter().map(|y| y * y).sum::<f64>();

        let numerator = n * sum_xy - sum_x * sum_y;
        let denominator = ((n * sum_x2 - sum_x * sum_x) * (n * sum_y2 - sum_y * sum_y)).sqrt();

        if denominator.abs() < 1e-10 {
            0.0
        } else {
            (numerator / denominator).clamp(-1.0, 1.0)
        }
    }

    /// Get current adaptation statistics.
    pub fn get_adaptation_stats(&self) -> AdaptationStats {
        AdaptationStats {
            recalculation_count: self.recalculation_count,
            last_recalculation: self.last_recalculation,
            current_adjustments: self.weight_adjustments.clone(),
            feature_effectiveness: self.feature_performance
                .iter()
                .map(|(f, p)| (*f, p.effectiveness))
                .collect(),
        }
    }

    /// Reset weights to base configuration.
    #[instrument(skip(self))]
    pub fn reset_to_base(&mut self) {
        self.weight_adjustments.clear();
        for performance in self.feature_performance.values_mut() {
            *performance = FeaturePerformance::default();
        }
        self.recalculation_count = 0;
        
        info!("Reset adaptive weights to base configuration");
    }

    /// Set adaptation rate (0.0 = no adaptation, 1.0 = full adaptation).
    pub fn set_adaptation_rate(&mut self, rate: f64) {
        self.adaptation_rate = rate.clamp(0.0, 1.0);
        debug!("Set adaptation rate to {:.2}", self.adaptation_rate);
    }
}

/// Statistics about weight adaptation.
#[derive(Debug, Clone)]
pub struct AdaptationStats {
    pub recalculation_count: u64,
    pub last_recalculation: u64,
    pub current_adjustments: HashMap<Feature, f64>,
    pub feature_effectiveness: HashMap<Feature, f64>,
}

impl Default for FeaturePerformance {
    fn default() -> Self {
        Self {
            successful_scores: Vec::new(),
            failed_scores: Vec::new(),
            score_variance: 0.0,
            correlation_with_outcome: 0.0,
            effectiveness: 0.5, // Neutral effectiveness
        }
    }
}

/// Get current timestamp in seconds.
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::types::*;
    use crate::types::PremintCandidate;
    use solana_sdk::pubkey::Pubkey;
    use std::collections::HashMap;

    fn create_test_weights() -> FeatureWeights {
        FeatureWeights::default()
    }

    fn create_test_candidate(score: u8, feature_scores: HashMap<String, f64>) -> ScoredCandidate {
        ScoredCandidate {
            base: PremintCandidate {
                mint: Pubkey::new_unique(),
                creator: Pubkey::new_unique(),
                program: "test".to_string(),
                slot: 12345,
                timestamp: 1640995200,
                instruction_summary: None,
                is_jito_bundle: Some(true),
            },
            mint: Pubkey::new_unique(),
            predicted_score: score,
            feature_scores,
            reason: "test".to_string(),
            calculation_time: 1000,
            anomaly_detected: false,
            timestamp: 1640995200,
        }
    }

    #[test]
    fn test_adaptive_weights_creation() {
        let base_weights = create_test_weights();
        let adaptive = AdaptiveWeights::new(base_weights.clone());
        
        let effective_weights = adaptive.get_effective_weights();
        
        // Initially, effective weights should equal base weights
        assert_eq!(effective_weights.liquidity, base_weights.liquidity);
        assert_eq!(effective_weights.volume_growth, base_weights.volume_growth);
    }

    #[test]
    fn test_recalculate_with_empty_history() {
        let base_weights = create_test_weights();
        let mut adaptive = AdaptiveWeights::new(base_weights);
        
        adaptive.recalculate(&[]);
        
        // Should not panic and should not change recalculation count
        assert_eq!(adaptive.recalculation_count, 0);
    }

    #[test]
    fn test_recalculate_with_historical_data() {
        let base_weights = create_test_weights();
        let mut adaptive = AdaptiveWeights::new(base_weights);
        
        // Create some test historical data
        let mut successful_scores = HashMap::new();
        successful_scores.insert("liquidity".to_string(), 0.9);
        successful_scores.insert("volume_growth".to_string(), 0.8);
        
        let mut failed_scores = HashMap::new();
        failed_scores.insert("liquidity".to_string(), 0.2);
        failed_scores.insert("volume_growth".to_string(), 0.3);
        
        let historical = vec![
            create_test_candidate(85, successful_scores.clone()),
            create_test_candidate(90, successful_scores),
            create_test_candidate(45, failed_scores.clone()),
            create_test_candidate(30, failed_scores),
        ];
        
        adaptive.recalculate(&historical);
        
        assert_eq!(adaptive.recalculation_count, 1);
        assert!(adaptive.last_recalculation > 0);
    }

    #[test]
    fn test_calculate_variance() {
        let base_weights = create_test_weights();
        let adaptive = AdaptiveWeights::new(base_weights);
        
        let scores = vec![0.8, 0.9, 0.7, 0.85, 0.75];
        let variance = AdaptiveWeights::calculate_variance_static(&scores);
        
        assert!(variance > 0.0);
        assert!(variance < 1.0);
    }

    #[test]
    fn test_calculate_variance_empty() {
        let base_weights = create_test_weights();
        let adaptive = AdaptiveWeights::new(base_weights);
        
        let variance = AdaptiveWeights::calculate_variance_static(&[]);
        assert_eq!(variance, 0.0);
    }

    #[test]
    fn test_calculate_effectiveness() {
        let base_weights = create_test_weights();
        let adaptive = AdaptiveWeights::new(base_weights);
        
        let mut performance = FeaturePerformance::default();
        performance.successful_scores = vec![0.8, 0.9, 0.85];
        performance.failed_scores = vec![0.3, 0.2, 0.4];
        
        let effectiveness = AdaptiveWeights::calculate_effectiveness_static(&performance);
        
        // Should be high since successful scores are much higher than failed
        assert!(effectiveness > 0.7);
    }

    #[test]
    fn test_weight_adjustment_bounds() {
        let base_weights = create_test_weights();
        let mut adaptive = AdaptiveWeights::new(base_weights);
        
        // Manually set extreme adjustment
        adaptive.weight_adjustments.insert(Feature::Liquidity, 2.0); // Very high
        
        let effective_weights = adaptive.get_effective_weights();
        
        // Should be clamped to reasonable bounds
        assert!(effective_weights.liquidity <= 1.0);
        assert!(effective_weights.liquidity >= 0.01);
    }

    #[test]
    fn test_reset_to_base() {
        let base_weights = create_test_weights();
        let mut adaptive = AdaptiveWeights::new(base_weights.clone());
        
        // Make some changes
        adaptive.weight_adjustments.insert(Feature::Liquidity, 0.3);
        adaptive.recalculation_count = 5;
        
        adaptive.reset_to_base();
        
        assert_eq!(adaptive.recalculation_count, 0);
        assert!(adaptive.weight_adjustments.is_empty());
        
        let effective_weights = adaptive.get_effective_weights();
        assert_eq!(effective_weights.liquidity, base_weights.liquidity);
    }

    #[test]
    fn test_adaptation_rate() {
        let base_weights = create_test_weights();
        let mut adaptive = AdaptiveWeights::new(base_weights);
        
        adaptive.set_adaptation_rate(0.5);
        assert_eq!(adaptive.adaptation_rate, 0.5);
        
        // Test bounds
        adaptive.set_adaptation_rate(-0.1);
        assert_eq!(adaptive.adaptation_rate, 0.0);
        
        adaptive.set_adaptation_rate(1.5);
        assert_eq!(adaptive.adaptation_rate, 1.0);
    }

    #[test]
    fn test_get_adaptation_stats() {
        let base_weights = create_test_weights();
        let mut adaptive = AdaptiveWeights::new(base_weights);
        
        adaptive.weight_adjustments.insert(Feature::Liquidity, 0.1);
        adaptive.recalculation_count = 3;
        
        let stats = adaptive.get_adaptation_stats();
        
        assert_eq!(stats.recalculation_count, 3);
        assert!(stats.current_adjustments.contains_key(&Feature::Liquidity));
        assert_eq!(stats.feature_effectiveness.len(), Feature::all().len());
    }

    #[test]
    fn test_correlation_calculation() {
        let base_weights = create_test_weights();
        let adaptive = AdaptiveWeights::new(base_weights);
        
        // Create test data with clear correlation
        let mut high_feature_high_score = HashMap::new();
        high_feature_high_score.insert("liquidity".to_string(), 0.9);
        
        let mut low_feature_low_score = HashMap::new();
        low_feature_low_score.insert("liquidity".to_string(), 0.1);
        
        let historical = vec![
            create_test_candidate(90, high_feature_high_score.clone()),
            create_test_candidate(85, high_feature_high_score),
            create_test_candidate(20, low_feature_low_score.clone()),
            create_test_candidate(15, low_feature_low_score),
        ];
        
        let correlation = adaptive.calculate_correlation(Feature::Liquidity, &historical);
        
        // Should show positive correlation
        assert!(correlation > 0.5);
    }
}
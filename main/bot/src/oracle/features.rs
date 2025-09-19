//! Feature computation and scoring logic.
//!
//! This module contains all the feature computation functions that analyze
//! different aspects of tokens to produce normalized scores (0.0-1.0).

use crate::oracle::types::{
    Feature, FeatureScores, TokenData, OracleConfig, VolumeData, LiquidityPool,
    HolderData, CreatorHoldings, SocialActivity, Metadata,
};
use crate::types::PremintCandidate;
use anyhow::Result;
use std::collections::HashMap;
use tracing::{debug, warn, instrument};

/// Feature computer responsible for calculating all feature scores.
pub struct OracleFeatureComputer {
    config: OracleConfig,
}

impl OracleFeatureComputer {
    /// Create a new feature computer with the given configuration.
    pub fn new(config: OracleConfig) -> Self {
        Self { config }
    }

    /// Compute all features for a given token.
    #[instrument(skip(self, token_data), fields(mint = %candidate.mint))]
    pub async fn compute_all_features(
        &self,
        candidate: &PremintCandidate,
        token_data: &TokenData,
    ) -> Result<FeatureScores> {
        let mut scores = FeatureScores::new();

        // Compute each feature score
        scores.set(Feature::Liquidity, self.compute_liquidity_score(token_data)?);
        scores.set(
            Feature::HolderDistribution,
            self.compute_holder_distribution_score(token_data)?,
        );
        scores.set(Feature::VolumeGrowth, self.compute_volume_growth_score(token_data)?);
        scores.set(Feature::HolderGrowth, self.compute_holder_growth_score(token_data)?);
        scores.set(Feature::PriceChange, self.compute_price_change_score(token_data)?);
        scores.set(
            Feature::JitoBundlePresence,
            self.compute_jito_bundle_score(candidate)?,
        );
        scores.set(
            Feature::CreatorSellSpeed,
            self.compute_creator_sell_score(token_data)?,
        );
        scores.set(
            Feature::MetadataQuality,
            self.compute_metadata_quality_score(token_data)?,
        );
        scores.set(Feature::SocialActivity, self.compute_social_activity_score(token_data)?);

        debug!("Computed feature scores: {:?}", scores.to_hashmap());
        Ok(scores)
    }

    /// Compute liquidity score based on SOL amount in pools.
    #[instrument(skip(self, token_data))]
    fn compute_liquidity_score(&self, token_data: &TokenData) -> Result<f64> {
        let liquidity_sol = match &token_data.liquidity_pool {
            Some(pool) => pool.sol_amount,
            None => {
                debug!("No liquidity pool found");
                return Ok(0.0);
            }
        };

        // Normalize liquidity score: 0.0 at min_liquidity_sol, 1.0 at 10x min_liquidity_sol
        let min_liquidity = self.config.thresholds.min_liquidity_sol;
        let max_liquidity = min_liquidity * 10.0;

        let score = if liquidity_sol < min_liquidity {
            0.0
        } else if liquidity_sol >= max_liquidity {
            1.0
        } else {
            (liquidity_sol - min_liquidity) / (max_liquidity - min_liquidity)
        };

        debug!("Liquidity score: {} SOL -> {}", liquidity_sol, score);
        Ok(score.clamp(0.0, 1.0))
    }

    /// Compute holder distribution score (higher score for more distributed holdings).
    #[instrument(skip(self, token_data))]
    fn compute_holder_distribution_score(&self, token_data: &TokenData) -> Result<f64> {
        if token_data.holder_distribution.is_empty() {
            return Ok(0.0);
        }

        // Calculate concentration of top holders
        let top_10_concentration: f64 = token_data
            .holder_distribution
            .iter()
            .take(10)
            .map(|h| h.percentage)
            .sum();

        // Better distribution = lower concentration = higher score
        // Score inversely related to top 10 concentration
        let score = match top_10_concentration {
            x if x >= 0.9 => 0.0, // Highly concentrated
            x if x >= 0.7 => 0.2,
            x if x >= 0.5 => 0.5,
            x if x >= 0.3 => 0.8,
            _ => 1.0, // Well distributed
        };

        debug!("Holder distribution: top 10 = {:.2}% -> score {}", top_10_concentration * 100.0, score);
        Ok(score)
    }

    /// Compute volume growth score.
    #[instrument(skip(self, token_data))]
    fn compute_volume_growth_score(&self, token_data: &TokenData) -> Result<f64> {
        let volume_data = &token_data.volume_data;
        let growth_rate = volume_data.volume_growth_rate;

        // Normalize growth rate to 0-1 range
        let threshold = self.config.thresholds.volume_growth_threshold;
        let score = if growth_rate <= 1.0 {
            0.0 // No growth or decline
        } else if growth_rate >= threshold * 5.0 {
            1.0 // Excellent growth
        } else {
            (growth_rate - 1.0) / (threshold * 5.0 - 1.0)
        };

        debug!("Volume growth: {}x -> score {}", growth_rate, score);
        Ok(score.clamp(0.0, 1.0))
    }

    /// Compute holder growth score.
    #[instrument(skip(self, token_data))]
    fn compute_holder_growth_score(&self, token_data: &TokenData) -> Result<f64> {
        if token_data.holder_history.len() < 2 {
            return Ok(0.5); // Default for insufficient data
        }

        let current_holders = token_data.holder_history.back().unwrap_or(&0);
        let initial_holders = token_data.holder_history.front().unwrap_or(&1);

        let growth_rate = *current_holders as f64 / (*initial_holders).max(1) as f64;
        let threshold = self.config.thresholds.holder_growth_threshold;

        let score = if growth_rate <= 1.0 {
            0.0
        } else if growth_rate >= threshold * 3.0 {
            1.0
        } else {
            (growth_rate - 1.0) / (threshold * 3.0 - 1.0)
        };

        debug!("Holder growth: {}x ({} -> {}) -> score {}", 
               growth_rate, initial_holders, current_holders, score);
        Ok(score.clamp(0.0, 1.0))
    }

    /// Compute price change score.
    #[instrument(skip(self, token_data))]
    fn compute_price_change_score(&self, token_data: &TokenData) -> Result<f64> {
        if token_data.price_history.len() < 2 {
            return Ok(0.5); // Default for insufficient data
        }

        let current_price = token_data.price_history.back().unwrap_or(&0.0);
        let initial_price = token_data.price_history.front().unwrap_or(&1.0);

        let price_change = (current_price - initial_price) / initial_price.max(0.0001);

        // Positive price change is good, but extremely high changes might be suspicious
        let score = match price_change {
            x if x <= -0.5 => 0.0,  // Major decline
            x if x <= 0.0 => 0.2,   // Any decline
            x if x <= 0.5 => 0.5 + x, // Moderate gain
            x if x <= 2.0 => 0.8,   // Good gain
            x if x <= 10.0 => 0.9,  // Excellent but not suspicious
            _ => 0.7, // Very high gains might be suspicious
        };

        debug!("Price change: {:.2}% -> score {}", price_change * 100.0, score);
        Ok(score.clamp(0.0, 1.0))
    }

    /// Compute Jito bundle presence score.
    #[instrument(skip(self, candidate))]
    fn compute_jito_bundle_score(&self, candidate: &PremintCandidate) -> Result<f64> {
        let score = match candidate.is_jito_bundle {
            Some(true) => 0.8,  // Jito bundles are generally positive
            Some(false) => 0.3, // Non-bundle is neutral/slightly negative
            None => 0.5,        // Unknown
        };

        debug!("Jito bundle presence: {:?} -> score {}", candidate.is_jito_bundle, score);
        Ok(score)
    }

    /// Compute creator sell speed score (lower score for fast selling).
    #[instrument(skip(self, token_data))]
    fn compute_creator_sell_score(&self, token_data: &TokenData) -> Result<f64> {
        let creator = &token_data.creator_holdings;
        
        if creator.sell_transactions == 0 {
            return Ok(1.0); // No selling is good
        }

        // Calculate sell percentage
        let sell_percentage = if creator.initial_balance > 0 {
            (creator.initial_balance - creator.current_balance) as f64 / creator.initial_balance as f64
        } else {
            0.0
        };

        // Check time since creation
        let time_penalty = if let Some(first_sell) = creator.first_sell_timestamp {
            let time_diff = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .saturating_sub(first_sell);
            
            if time_diff < self.config.thresholds.creator_sell_penalty_threshold {
                0.5 // Penalty for quick selling
            } else {
                1.0 // No penalty for later selling
            }
        } else {
            1.0
        };

        // Combined score: lower sell percentage and appropriate timing = higher score
        let sell_score = 1.0 - sell_percentage;
        let score = sell_score * time_penalty;

        debug!("Creator sell: {:.1}% sold, {} tx, time_penalty {} -> score {}", 
               sell_percentage * 100.0, creator.sell_transactions, time_penalty, score);
        Ok(score.clamp(0.0, 1.0))
    }

    /// Compute metadata quality score.
    #[instrument(skip(self, token_data))]
    fn compute_metadata_quality_score(&self, token_data: &TokenData) -> Result<f64> {
        let metadata = match &token_data.metadata {
            Some(meta) => meta,
            None => {
                debug!("No metadata available");
                return Ok(0.0);
            }
        };

        let mut quality_score = 0.0;
        let mut factors = 0;

        // Check name quality
        if !metadata.name.is_empty() && metadata.name.len() > 2 {
            quality_score += 0.2;
            factors += 1;
        }

        // Check symbol quality
        if !metadata.symbol.is_empty() && metadata.symbol.len() <= 10 {
            quality_score += 0.2;
            factors += 1;
        }

        // Check description quality
        if !metadata.description.is_empty() && metadata.description.len() > 20 {
            quality_score += 0.2;
            factors += 1;
        }

        // Check image presence
        if !metadata.image.is_empty() && metadata.image.starts_with("http") {
            quality_score += 0.2;
            factors += 1;
        }

        // Check attributes
        if !metadata.attributes.is_empty() {
            quality_score += 0.2;
            factors += 1;
        }

        let final_score = if factors > 0 { quality_score } else { 0.0_f64 };

        debug!("Metadata quality: {}/{} factors -> score {}", factors, 5, final_score);
        Ok(final_score.clamp(0.0, 1.0))
    }

    /// Compute social activity score.
    #[instrument(skip(self, token_data))]
    fn compute_social_activity_score(&self, token_data: &TokenData) -> Result<f64> {
        let social = &token_data.social_activity;
        let threshold = self.config.thresholds.social_activity_threshold;

        // Combine different social metrics
        let total_activity = social.twitter_mentions as f64 * 0.4
            + social.telegram_members as f64 * 0.3
            + social.discord_members as f64 * 0.3;

        let score = if total_activity >= threshold * 5.0 {
            1.0
        } else if total_activity >= threshold {
            total_activity / (threshold * 5.0)
        } else {
            total_activity / threshold * 0.5 // Partial credit for low activity
        };

        debug!("Social activity: twitter={}, telegram={}, discord={}, total={:.1} -> score {}",
               social.twitter_mentions, social.telegram_members, social.discord_members, 
               total_activity, score);
        Ok(score.clamp(0.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::types::*;
    use crate::types::PremintCandidate;
    use solana_sdk::pubkey::Pubkey;
    use std::collections::VecDeque;

    fn create_test_config() -> OracleConfig {
        OracleConfig::default()
    }

    fn create_test_candidate() -> PremintCandidate {
        PremintCandidate {
            mint: Pubkey::new_unique(),
            creator: Pubkey::new_unique(),
            program: "test".to_string(),
            slot: 12345,
            timestamp: 1640995200, // 2022-01-01
            instruction_summary: None,
            is_jito_bundle: Some(true),
        }
    }

    fn create_test_token_data() -> TokenData {
        TokenData {
            supply: 1_000_000_000,
            decimals: 9,
            metadata_uri: "https://example.com/metadata.json".to_string(),
            metadata: Some(Metadata {
                name: "Test Token".to_string(),
                symbol: "TEST".to_string(),
                description: "A test token for unit testing purposes".to_string(),
                image: "https://example.com/image.png".to_string(),
                attributes: vec![],
            }),
            holder_distribution: vec![
                HolderData {
                    address: Pubkey::new_unique(),
                    percentage: 0.1,
                    is_whale: false,
                },
            ],
            liquidity_pool: Some(LiquidityPool {
                sol_amount: 50.0,
                token_amount: 1000.0,
                pool_address: Pubkey::new_unique(),
                pool_type: PoolType::PumpFun,
            }),
            volume_data: VolumeData {
                initial_volume: 100.0,
                current_volume: 300.0,
                volume_growth_rate: 3.0,
                transaction_count: 50,
                buy_sell_ratio: 1.5,
            },
            creator_holdings: CreatorHoldings {
                initial_balance: 100_000_000,
                current_balance: 90_000_000,
                first_sell_timestamp: Some(1640995500),
                sell_transactions: 2,
            },
            holder_history: {
                let mut hist = VecDeque::new();
                hist.push_back(10);
                hist.push_back(25);
                hist
            },
            price_history: {
                let mut hist = VecDeque::new();
                hist.push_back(0.001);
                hist.push_back(0.0015);
                hist
            },
            social_activity: SocialActivity {
                twitter_mentions: 50,
                telegram_members: 200,
                discord_members: 100,
                social_score: 0.7,
            },
        }
    }

    #[test]
    fn test_liquidity_score_computation() {
        let computer = OracleFeatureComputer::new(create_test_config());
        let token_data = create_test_token_data();
        
        let score = computer.compute_liquidity_score(&token_data).unwrap();
        
        // With 50 SOL and min threshold of 10 SOL, max at 100 SOL
        // Score should be (50-10)/(100-10) = 40/90 ≈ 0.44
        assert!(score > 0.0 && score < 1.0);
    }

    #[test]
    fn test_holder_distribution_score() {
        let computer = OracleFeatureComputer::new(create_test_config());
        let token_data = create_test_token_data();
        
        let score = computer.compute_holder_distribution_score(&token_data).unwrap();
        
        // With 10% concentration, should get high score
        assert!(score > 0.8);
    }

    #[test]
    fn test_volume_growth_score() {
        let computer = OracleFeatureComputer::new(create_test_config());
        let token_data = create_test_token_data();
        
        let score = computer.compute_volume_growth_score(&token_data).unwrap();
        
        // 3x growth should give a good score
        assert!(score > 0.0);
    }

    #[test]
    fn test_jito_bundle_score() {
        let computer = OracleFeatureComputer::new(create_test_config());
        let candidate = create_test_candidate();
        
        let score = computer.compute_jito_bundle_score(&candidate).unwrap();
        
        // Jito bundle should give positive score
        assert_eq!(score, 0.8);
    }

    #[test]
    fn test_metadata_quality_score() {
        let computer = OracleFeatureComputer::new(create_test_config());
        let token_data = create_test_token_data();
        
        let score = computer.compute_metadata_quality_score(&token_data).unwrap();
        
        // Good metadata should give high score
        assert!(score >= 0.8); // All 4 factors present
    }

    #[test]
    fn test_feature_scores_operations() {
        let mut scores = FeatureScores::new();
        
        scores.set(Feature::Liquidity, 0.8);
        scores.set(Feature::VolumeGrowth, 0.6);
        
        assert_eq!(scores.get(Feature::Liquidity), 0.8);
        assert_eq!(scores.get(Feature::VolumeGrowth), 0.6);
        assert_eq!(scores.get(Feature::HolderDistribution), 0.0); // Default
        
        let hashmap = scores.to_hashmap();
        assert_eq!(hashmap.get("liquidity"), Some(&0.8));
        assert_eq!(hashmap.get("volume_growth"), Some(&0.6));
    }

    #[test]
    fn test_feature_scores_from_hashmap() {
        let mut map = HashMap::new();
        map.insert("liquidity".to_string(), 0.9);
        map.insert("volume_growth".to_string(), 0.7);
        
        let scores = FeatureScores::from_hashmap(&map);
        
        assert_eq!(scores.get(Feature::Liquidity), 0.9);
        assert_eq!(scores.get(Feature::VolumeGrowth), 0.7);
        assert_eq!(scores.get(Feature::HolderDistribution), 0.0);
    }

    #[tokio::test]
    async fn test_compute_all_features() {
        let computer = OracleFeatureComputer::new(create_test_config());
        let candidate = create_test_candidate();
        let token_data = create_test_token_data();
        
        let scores = computer.compute_all_features(&candidate, &token_data).await.unwrap();
        
        // Verify all features have been computed
        for feature in Feature::all() {
            let score = scores.get(feature);
            assert!(score >= 0.0 && score <= 1.0, "Feature {:?} score {} is out of range", feature, score);
        }
    }
}
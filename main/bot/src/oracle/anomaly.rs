//! Anomaly detection for token behavior analysis.
//!
//! This module identifies suspicious patterns in token data that might indicate
//! manipulated or problematic tokens that should be scored lower or avoided.

use crate::oracle::types::{TokenData, OracleConfig, VolumeData, HolderData, CreatorHoldings};
use std::collections::VecDeque;
use tracing::{debug, warn, instrument};

/// Anomaly detector for identifying suspicious token behavior.
pub struct AnomalyDetector {
    config: OracleConfig,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AnomalyType {
    /// Extremely high volume growth that seems artificial
    SuspiciousVolumeGrowth,
    /// Abnormally high transaction count
    HighTransactionCount,
    /// Very concentrated holder distribution
    HighHolderConcentration,
    /// Creator selling too quickly after launch
    CreatorQuickSell,
    /// Price pump followed by immediate dump
    PumpAndDump,
    /// Abnormal holder growth pattern
    AbnormalHolderGrowth,
    /// Liquidity manipulation patterns
    LiquidityManipulation,
}

impl AnomalyDetector {
    /// Create a new anomaly detector.
    pub fn new(config: OracleConfig) -> Self {
        Self { config }
    }

    /// Detect anomalies in token data.
    #[instrument(skip(self, token_data))]
    pub async fn detect_anomalies(&self, token_data: &TokenData) -> bool {
        let anomalies = self.identify_all_anomalies(token_data).await;
        
        if !anomalies.is_empty() {
            warn!("Detected {} anomalies: {:?}", anomalies.len(), anomalies);
            true
        } else {
            debug!("No anomalies detected");
            false
        }
    }

    /// Identify all types of anomalies present.
    #[instrument(skip(self, token_data))]
    pub async fn identify_all_anomalies(&self, token_data: &TokenData) -> Vec<AnomalyType> {
        let mut anomalies = Vec::new();

        // Check volume anomalies
        if let Some(anomaly) = self.check_volume_anomalies(&token_data.volume_data) {
            anomalies.push(anomaly);
        }

        // Check holder distribution anomalies
        if let Some(anomaly) = self.check_holder_distribution_anomalies(&token_data.holder_distribution) {
            anomalies.push(anomaly);
        }

        // Check creator behavior anomalies
        if let Some(anomaly) = self.check_creator_behavior_anomalies(&token_data.creator_holdings) {
            anomalies.push(anomaly);
        }

        // Check price pattern anomalies
        if let Some(anomaly) = self.check_price_pattern_anomalies(&token_data.price_history) {
            anomalies.push(anomaly);
        }

        // Check holder growth anomalies
        if let Some(anomaly) = self.check_holder_growth_anomalies(&token_data.holder_history) {
            anomalies.push(anomaly);
        }

        // Check liquidity anomalies
        if let Some(anomaly) = self.check_liquidity_anomalies(token_data) {
            anomalies.push(anomaly);
        }

        debug!("Identified {} anomalies", anomalies.len());
        anomalies
    }

    /// Check for volume-related anomalies.
    #[instrument(skip(self, volume_data))]
    fn check_volume_anomalies(&self, volume_data: &VolumeData) -> Option<AnomalyType> {
        // Check for extremely high volume growth (potential manipulation)
        if volume_data.volume_growth_rate > 10.0 {
            warn!("Suspicious volume growth: {}x", volume_data.volume_growth_rate);
            return Some(AnomalyType::SuspiciousVolumeGrowth);
        }

        // Check for abnormally high transaction count
        if volume_data.transaction_count > 1000 {
            warn!("High transaction count: {}", volume_data.transaction_count);
            return Some(AnomalyType::HighTransactionCount);
        }

        None
    }

    /// Check for holder distribution anomalies.
    #[instrument(skip(self, holders))]
    fn check_holder_distribution_anomalies(&self, holders: &[HolderData]) -> Option<AnomalyType> {
        if holders.is_empty() {
            return None;
        }

        // Check if top holder has too much concentration
        if let Some(top_holder) = holders.first() {
            if top_holder.percentage > 0.5 {
                warn!(
                    "High top holder concentration: {:.1}%",
                    top_holder.percentage * 100.0
                );
                return Some(AnomalyType::HighHolderConcentration);
            }
        }

        // Check if top 3 holders control too much
        let top_3_concentration: f64 = holders
            .iter()
            .take(3)
            .map(|h| h.percentage)
            .sum();

        if top_3_concentration > 0.8 {
            warn!(
                "High top 3 holder concentration: {:.1}%",
                top_3_concentration * 100.0
            );
            return Some(AnomalyType::HighHolderConcentration);
        }

        None
    }

    /// Check for creator behavior anomalies.
    #[instrument(skip(self, creator_holdings))]
    fn check_creator_behavior_anomalies(&self, creator_holdings: &CreatorHoldings) -> Option<AnomalyType> {
        // Check if creator started selling too quickly
        if let Some(first_sell_time) = creator_holdings.first_sell_timestamp {
            let current_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let time_to_sell = current_time.saturating_sub(first_sell_time);
            
            if time_to_sell < self.config.thresholds.creator_sell_penalty_threshold {
                warn!(
                    "Creator quick sell: {}s after launch",
                    time_to_sell
                );
                return Some(AnomalyType::CreatorQuickSell);
            }
        }

        // Check if creator sold too much too fast
        if creator_holdings.initial_balance > 0 {
            let sell_percentage = (creator_holdings.initial_balance - creator_holdings.current_balance) as f64
                / creator_holdings.initial_balance as f64;

            if sell_percentage > 0.8 && creator_holdings.sell_transactions > 5 {
                warn!(
                    "Creator massive sell-off: {:.1}% in {} transactions",
                    sell_percentage * 100.0,
                    creator_holdings.sell_transactions
                );
                return Some(AnomalyType::CreatorQuickSell);
            }
        }

        None
    }

    /// Check for price pattern anomalies (pump and dump).
    #[instrument(skip(self, price_history))]
    fn check_price_pattern_anomalies(&self, price_history: &VecDeque<f64>) -> Option<AnomalyType> {
        if price_history.len() < 3 {
            return None;
        }

        let prices: Vec<f64> = price_history.iter().cloned().collect();
        
        // Look for rapid price increase followed by rapid decrease
        for window in prices.windows(3) {
            let [start, peak, end] = [window[0], window[1], window[2]];
            
            if start > 0.0 && peak > 0.0 && end > 0.0 {
                let pump_ratio = peak / start;
                let dump_ratio = end / peak;
                
                // Detect pump (5x increase) followed by dump (50% decrease)
                if pump_ratio > 5.0 && dump_ratio < 0.5 {
                    warn!(
                        "Pump and dump pattern: {}x pump, {}x dump",
                        pump_ratio, dump_ratio
                    );
                    return Some(AnomalyType::PumpAndDump);
                }
            }
        }

        None
    }

    /// Check for abnormal holder growth patterns.
    #[instrument(skip(self, holder_history))]
    fn check_holder_growth_anomalies(&self, holder_history: &VecDeque<usize>) -> Option<AnomalyType> {
        if holder_history.len() < 2 {
            return None;
        }

        let holders: Vec<usize> = holder_history.iter().cloned().collect();
        
        // Check for sudden massive holder increase (potential bot activity)
        for window in holders.windows(2) {
            let [prev, current] = [window[0], window[1]];
            
            if prev > 0 {
                let growth_ratio = current as f64 / prev as f64;
                
                // Suspicious if holders increase by more than 20x suddenly
                if growth_ratio > 20.0 {
                    warn!(
                        "Abnormal holder growth: {} to {} ({:.1}x)",
                        prev, current, growth_ratio
                    );
                    return Some(AnomalyType::AbnormalHolderGrowth);
                }
            }
        }

        None
    }

    /// Check for liquidity manipulation patterns.
    #[instrument(skip(self, token_data))]
    fn check_liquidity_anomalies(&self, token_data: &TokenData) -> Option<AnomalyType> {
        if let Some(pool) = &token_data.liquidity_pool {
            // Check for extremely low liquidity relative to volume
            let volume = token_data.volume_data.current_volume;
            let liquidity = pool.sol_amount;
            
            if liquidity > 0.0 {
                let volume_to_liquidity_ratio = volume / liquidity;
                
                // Suspicious if volume is 100x the liquidity
                if volume_to_liquidity_ratio > 100.0 {
                    warn!(
                        "Liquidity manipulation: volume/liquidity ratio = {:.1}",
                        volume_to_liquidity_ratio
                    );
                    return Some(AnomalyType::LiquidityManipulation);
                }
            }

            // Check for extremely imbalanced pool ratios
            if pool.token_amount > 0.0 {
                let price = pool.sol_amount / (pool.token_amount / 10f64.powi(9));
                
                // Very high price might indicate liquidity manipulation
                if price > 1.0 {
                    warn!("Suspicious token price: {} SOL", price);
                    return Some(AnomalyType::LiquidityManipulation);
                }
            }
        }

        None
    }

    /// Get severity score for an anomaly type (0.0 = minor, 1.0 = critical).
    pub fn get_anomaly_severity(&self, anomaly_type: &AnomalyType) -> f64 {
        match anomaly_type {
            AnomalyType::SuspiciousVolumeGrowth => 0.8,
            AnomalyType::HighTransactionCount => 0.6,
            AnomalyType::HighHolderConcentration => 0.7,
            AnomalyType::CreatorQuickSell => 0.9,
            AnomalyType::PumpAndDump => 1.0,
            AnomalyType::AbnormalHolderGrowth => 0.7,
            AnomalyType::LiquidityManipulation => 0.9,
        }
    }

    /// Calculate overall anomaly score (0.0 = no anomalies, 1.0 = severe anomalies).
    #[instrument(skip(self, anomalies))]
    pub fn calculate_anomaly_score(&self, anomalies: &[AnomalyType]) -> f64 {
        if anomalies.is_empty() {
            return 0.0;
        }

        let total_severity: f64 = anomalies
            .iter()
            .map(|a| self.get_anomaly_severity(a))
            .sum();

        // Cap at 1.0 and apply diminishing returns for multiple anomalies
        let raw_score = total_severity / anomalies.len() as f64;
        raw_score.min(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::types::*;
    use solana_sdk::pubkey::Pubkey;
    use std::collections::VecDeque;

    fn create_test_config() -> OracleConfig {
        OracleConfig::default()
    }

    fn create_normal_token_data() -> TokenData {
        TokenData {
            supply: 1_000_000_000,
            decimals: 9,
            metadata_uri: "https://example.com/metadata.json".to_string(),
            metadata: None,
            holder_distribution: vec![
                HolderData {
                    address: Pubkey::new_unique(),
                    percentage: 0.1, // 10% - normal
                    is_whale: false,
                },
                HolderData {
                    address: Pubkey::new_unique(),
                    percentage: 0.05, // 5% - normal
                    is_whale: false,
                },
            ],
            liquidity_pool: Some(LiquidityPool {
                sol_amount: 50.0,
                token_amount: 1000000.0,
                pool_address: Pubkey::new_unique(),
                pool_type: PoolType::PumpFun,
            }),
            volume_data: VolumeData {
                initial_volume: 100.0,
                current_volume: 200.0,
                volume_growth_rate: 2.0, // Normal 2x growth
                transaction_count: 50,   // Normal count
                buy_sell_ratio: 1.5,
            },
            creator_holdings: CreatorHoldings {
                initial_balance: 100_000_000,
                current_balance: 95_000_000, // Sold 5%
                first_sell_timestamp: Some(1640995500), // 5 minutes after creation
                sell_transactions: 1,
            },
            holder_history: {
                let mut hist = VecDeque::new();
                hist.push_back(10);
                hist.push_back(25); // Normal growth
                hist
            },
            price_history: {
                let mut hist = VecDeque::new();
                hist.push_back(0.001);
                hist.push_back(0.0015); // Normal price increase
                hist
            },
            social_activity: SocialActivity::default(),
        }
    }

    #[tokio::test]
    async fn test_no_anomalies_detected() {
        let detector = AnomalyDetector::new(create_test_config());
        let token_data = create_normal_token_data();
        
        let has_anomalies = detector.detect_anomalies(&token_data).await;
        assert!(!has_anomalies);
        
        let anomalies = detector.identify_all_anomalies(&token_data).await;
        assert!(anomalies.is_empty());
    }

    #[tokio::test]
    async fn test_suspicious_volume_growth() {
        let detector = AnomalyDetector::new(create_test_config());
        let mut token_data = create_normal_token_data();
        
        // Set extremely high volume growth
        token_data.volume_data.volume_growth_rate = 15.0;
        
        let anomalies = detector.identify_all_anomalies(&token_data).await;
        assert!(anomalies.contains(&AnomalyType::SuspiciousVolumeGrowth));
    }

    #[tokio::test]
    async fn test_high_transaction_count() {
        let detector = AnomalyDetector::new(create_test_config());
        let mut token_data = create_normal_token_data();
        
        // Set very high transaction count
        token_data.volume_data.transaction_count = 1500;
        
        let anomalies = detector.identify_all_anomalies(&token_data).await;
        assert!(anomalies.contains(&AnomalyType::HighTransactionCount));
    }

    #[tokio::test]
    async fn test_high_holder_concentration() {
        let detector = AnomalyDetector::new(create_test_config());
        let mut token_data = create_normal_token_data();
        
        // Set one holder with 60% of tokens
        token_data.holder_distribution[0].percentage = 0.6;
        
        let anomalies = detector.identify_all_anomalies(&token_data).await;
        assert!(anomalies.contains(&AnomalyType::HighHolderConcentration));
    }

    #[tokio::test]
    async fn test_creator_quick_sell() {
        let detector = AnomalyDetector::new(create_test_config());
        let mut token_data = create_normal_token_data();
        
        // Set creator selling very quickly (within penalty threshold)
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        token_data.creator_holdings.first_sell_timestamp = Some(current_time - 30); // 30 seconds ago
        
        let anomalies = detector.identify_all_anomalies(&token_data).await;
        assert!(anomalies.contains(&AnomalyType::CreatorQuickSell));
    }

    #[tokio::test]
    async fn test_pump_and_dump() {
        let detector = AnomalyDetector::new(create_test_config());
        let mut token_data = create_normal_token_data();
        
        // Create pump and dump pattern
        let mut price_history = VecDeque::new();
        price_history.push_back(0.001);  // Start
        price_history.push_back(0.008);  // 8x pump
        price_history.push_back(0.003);  // 62% dump
        token_data.price_history = price_history;
        
        let anomalies = detector.identify_all_anomalies(&token_data).await;
        assert!(anomalies.contains(&AnomalyType::PumpAndDump));
    }

    #[tokio::test]
    async fn test_abnormal_holder_growth() {
        let detector = AnomalyDetector::new(create_test_config());
        let mut token_data = create_normal_token_data();
        
        // Create abnormal holder growth
        let mut holder_history = VecDeque::new();
        holder_history.push_back(5);   // Start with 5 holders
        holder_history.push_back(150); // Jump to 150 holders (30x growth)
        token_data.holder_history = holder_history;
        
        let anomalies = detector.identify_all_anomalies(&token_data).await;
        assert!(anomalies.contains(&AnomalyType::AbnormalHolderGrowth));
    }

    #[test]
    fn test_anomaly_severity_scores() {
        let detector = AnomalyDetector::new(create_test_config());
        
        assert_eq!(detector.get_anomaly_severity(&AnomalyType::PumpAndDump), 1.0);
        assert_eq!(detector.get_anomaly_severity(&AnomalyType::CreatorQuickSell), 0.9);
        assert_eq!(detector.get_anomaly_severity(&AnomalyType::HighTransactionCount), 0.6);
    }

    #[test]
    fn test_calculate_anomaly_score() {
        let detector = AnomalyDetector::new(create_test_config());
        
        // No anomalies
        assert_eq!(detector.calculate_anomaly_score(&[]), 0.0);
        
        // Single high severity anomaly
        let anomalies = vec![AnomalyType::PumpAndDump];
        assert_eq!(detector.calculate_anomaly_score(&anomalies), 1.0);
        
        // Multiple anomalies
        let anomalies = vec![
            AnomalyType::HighTransactionCount,  // 0.6
            AnomalyType::CreatorQuickSell,      // 0.9
        ];
        let score = detector.calculate_anomaly_score(&anomalies);
        assert!(score > 0.6 && score <= 1.0);
    }
}
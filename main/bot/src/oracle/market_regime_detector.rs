//! Market Regime Detector - Pillar III: Contextual Adaptations
//!
//! This module implements the autonomous MarketRegimeDetector that continuously
//! analyzes macro-economic indicators to identify the current market state and
//! adapt Oracle behavior accordingly.

use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{info, debug, warn, instrument};

use crate::oracle::data_sources::OracleDataSources;
use crate::oracle::types::MarketRegime;

/// Autonomous market regime detector that runs in the background.
/// 
/// This component continuously monitors macro-economic indicators like SOL price
/// volatility, network TPS, and DEX volume to identify the current market regime.
/// It updates a shared state that other components can read to adapt their behavior.
pub struct MarketRegimeDetector {
    /// Data sources for fetching macro-economic data
    data_sources: Arc<OracleDataSources>,
    
    /// Shared state holding the current market regime
    current_regime: Arc<RwLock<MarketRegime>>,
    
    /// How often to analyze market conditions
    detection_interval: Duration,
    
    /// Price history for volatility calculations (circular buffer)
    sol_price_history: Vec<f64>,
    
    /// Maximum number of price points to keep in history
    max_price_history: usize,
}

impl MarketRegimeDetector {
    /// Create a new MarketRegimeDetector.
    pub fn new(
        data_sources: Arc<OracleDataSources>,
        current_regime: Arc<RwLock<MarketRegime>>,
        detection_interval_seconds: u64,
    ) -> Self {
        Self {
            data_sources,
            current_regime,
            detection_interval: Duration::from_secs(detection_interval_seconds),
            sol_price_history: Vec::with_capacity(60), // Hold up to 60 data points
            max_price_history: 60,
        }
    }

    /// Run the market regime detection loop.
    /// 
    /// This method runs indefinitely, periodically analyzing market conditions
    /// and updating the shared regime state when changes are detected.
    #[instrument(skip(self))]
    pub async fn run(mut self) {
        info!(
            "MarketRegimeDetector started. Analysis interval: {} seconds",
            self.detection_interval.as_secs()
        );

        let mut interval = tokio::time::interval(self.detection_interval);

        loop {
            interval.tick().await;
            
            if let Err(e) = self.analyze_market_regime().await {
                warn!("Failed to analyze market regime: {}", e);
                // Continue running despite errors
                continue;
            }
        }
    }

    /// Perform market regime analysis and update shared state if regime changed.
    #[instrument(skip(self))]
    async fn analyze_market_regime(&mut self) -> Result<()> {
        debug!("Performing market regime analysis...");

        // --- Phase 1: Gather Macro-economic Data ---
        let sol_price = self.data_sources.fetch_sol_price_usd().await
            .unwrap_or_else(|e| {
                warn!("Failed to fetch SOL price: {}", e);
                0.0 // Use 0.0 as fallback, but don't update history
            });

        // Only update price history if we got a valid price
        if sol_price > 0.0 {
            self.update_price_history(sol_price);
        }

        let volatility = self.data_sources
            .calculate_sol_volatility(&self.sol_price_history)
            .await
            .unwrap_or_else(|e| {
                warn!("Failed to calculate volatility: {}", e);
                0.0
            });

        let network_tps = self.data_sources.fetch_network_tps().await
            .unwrap_or_else(|e| {
                warn!("Failed to fetch network TPS: {}", e);
                1000.0 // Default to moderate TPS
            });

        let dex_volume = self.data_sources.fetch_global_dex_volume().await
            .unwrap_or_else(|e| {
                warn!("Failed to fetch DEX volume: {}", e);
                50_000_000.0 // Default volume
            });

        // --- Phase 2: Analyze and Determine Regime ---
        let new_regime = self.determine_regime(sol_price, volatility, network_tps, dex_volume);

        // --- Phase 3: Update Global State if Changed ---
        let mut current_regime_lock = self.current_regime.write().await;
        if *current_regime_lock != new_regime {
            info!(
                "Market Regime Shift Detected: {:?} -> {:?} (SOL: ${:.2}, Vol: {:.1}%, TPS: {:.0}, DEX Vol: ${:.0})",
                *current_regime_lock,
                new_regime,
                sol_price,
                volatility,
                network_tps,
                dex_volume
            );
            *current_regime_lock = new_regime;
        } else {
            debug!(
                "Market regime unchanged: {:?} (SOL: ${:.2}, Vol: {:.1}%, TPS: {:.0})",
                *current_regime_lock,
                sol_price,
                volatility,
                network_tps
            );
        }

        Ok(())
    }

    /// Update the price history with a new price point.
    fn update_price_history(&mut self, new_price: f64) {
        // Maintain circular buffer behavior
        if self.sol_price_history.len() >= self.max_price_history {
            self.sol_price_history.remove(0);
        }
        self.sol_price_history.push(new_price);
    }

    /// Determine market regime based on current indicators.
    /// 
    /// This is a rule-based decision model that can be extended with more
    /// sophisticated ML models in the future.
    #[instrument(skip(self))]
    fn determine_regime(
        &self,
        sol_price: f64,
        volatility_percent: f64,
        tps: f64,
        dex_volume: f64,
    ) -> MarketRegime {
        // Priority 1: Network congestion (highest priority for execution risk)
        if tps > 3000.0 {
            debug!("High TPS detected ({:.0}), classifying as HighCongestion", tps);
            return MarketRegime::HighCongestion;
        }

        // Priority 2: High volatility indicates choppy/uncertain market
        if volatility_percent > 5.0 {
            debug!("High volatility detected ({:.1}%), classifying as Choppy", volatility_percent);
            return MarketRegime::Choppy;
        }

        // Priority 3: Price trend analysis (requires sufficient history)
        if self.sol_price_history.len() > 10 && sol_price > 0.0 {
            let price_trend = self.calculate_price_trend();
            let volume_threshold = 40_000_000.0; // 40M USD
            
            if price_trend > 0.02 && dex_volume > volume_threshold && tps > 1500.0 {
                debug!(
                    "Bullish conditions: price trend {:.1}%, volume ${:.0}, TPS {:.0}",
                    price_trend * 100.0,
                    dex_volume,
                    tps
                );
                return MarketRegime::Bullish;
            }
            
            if price_trend < -0.02 {
                debug!(
                    "Bearish conditions: price trend {:.1}%",
                    price_trend * 100.0
                );
                return MarketRegime::Bearish;
            }
        }

        // Default: Low activity
        debug!(
            "Default to LowActivity: insufficient indicators for other regimes (price points: {})",
            self.sol_price_history.len()
        );
        MarketRegime::LowActivity
    }

    /// Calculate price trend over the available history.
    /// Returns the relative change from first to last price in history.
    fn calculate_price_trend(&self) -> f64 {
        if self.sol_price_history.len() < 2 {
            return 0.0;
        }

        let first_price = self.sol_price_history[0];
        let last_price = self.sol_price_history[self.sol_price_history.len() - 1];

        if first_price <= 0.0 {
            return 0.0;
        }

        (last_price - first_price) / first_price
    }

    /// Get the current market regime (for external access).
    pub async fn get_current_regime(&self) -> MarketRegime {
        *self.current_regime.read().await
    }

    /// Get current price history length (for monitoring/debugging).
    pub fn get_price_history_length(&self) -> usize {
        self.sol_price_history.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::types::OracleConfig;
    use reqwest::Client;

    fn create_test_detector() -> (MarketRegimeDetector, Arc<RwLock<MarketRegime>>) {
        let config = OracleConfig::default();
        let http_client = Client::new();
        let data_sources = Arc::new(OracleDataSources::new(vec![], http_client, config));
        let current_regime = Arc::new(RwLock::new(MarketRegime::LowActivity));
        
        let detector = MarketRegimeDetector::new(data_sources, current_regime.clone(), 60);
        (detector, current_regime)
    }

    #[test]
    fn test_price_history_management() {
        let (mut detector, _) = create_test_detector();
        
        // Add prices up to capacity
        for i in 1..=60 {
            detector.update_price_history(i as f64);
        }
        assert_eq!(detector.sol_price_history.len(), 60);
        assert_eq!(detector.sol_price_history[0], 1.0);
        assert_eq!(detector.sol_price_history[59], 60.0);
        
        // Add one more - should remove the first
        detector.update_price_history(61.0);
        assert_eq!(detector.sol_price_history.len(), 60);
        assert_eq!(detector.sol_price_history[0], 2.0);
        assert_eq!(detector.sol_price_history[59], 61.0);
    }

    #[test]
    fn test_price_trend_calculation() {
        let (mut detector, _) = create_test_detector();
        
        // Test upward trend
        detector.update_price_history(100.0);
        detector.update_price_history(105.0);
        let trend = detector.calculate_price_trend();
        assert!((trend - 0.05).abs() < 0.001); // 5% increase
        
        // Test downward trend
        detector.sol_price_history.clear();
        detector.update_price_history(100.0);
        detector.update_price_history(95.0);
        let trend = detector.calculate_price_trend();
        assert!((trend - (-0.05)).abs() < 0.001); // 5% decrease
    }

    #[test]
    fn test_regime_determination_high_congestion() {
        let (detector, _) = create_test_detector();
        
        let regime = detector.determine_regime(150.0, 2.0, 3500.0, 50_000_000.0);
        assert_eq!(regime, MarketRegime::HighCongestion);
    }

    #[test]
    fn test_regime_determination_choppy() {
        let (detector, _) = create_test_detector();
        
        let regime = detector.determine_regime(150.0, 6.0, 2000.0, 50_000_000.0);
        assert_eq!(regime, MarketRegime::Choppy);
    }

    #[test]
    fn test_regime_determination_low_activity() {
        let (detector, _) = create_test_detector();
        
        let regime = detector.determine_regime(150.0, 1.0, 1000.0, 30_000_000.0);
        assert_eq!(regime, MarketRegime::LowActivity);
    }
}
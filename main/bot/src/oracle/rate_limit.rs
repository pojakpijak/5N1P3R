//! Adaptive rate limiting wrapper around governor.
//!
//! This module provides adaptive rate limiting that can adjust quotas based on
//! error rates and system conditions.

use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use std::collections::VecDeque;
use std::num::NonZeroU32;
use std::time::{Duration, Instant};
use tracing::{debug, warn, instrument};

/// Adaptive rate limiter that adjusts quotas based on error rates.
pub struct AdaptiveRateLimiter {
    /// Underlying governor rate limiter
    limiter: DefaultDirectRateLimiter,
    /// Base requests per second (configured)
    base_quota: u32,
    /// Current effective quota
    current_quota: u32,
    /// Error rate tracking window
    error_window: VecDeque<RequestResult>,
    /// Window size for error rate calculation
    window_size: usize,
    /// Error rate threshold for quota reduction
    error_threshold: f64,
    /// Minimum quota (never go below this)
    min_quota: u32,
    /// Maximum quota (never go above this)
    max_quota: u32,
    /// Last quota adjustment time
    last_adjustment: Instant,
    /// Minimum time between adjustments
    adjustment_interval: Duration,
}

/// Result of a request (for error rate tracking).
#[derive(Debug, Clone)]
struct RequestResult {
    timestamp: Instant,
    success: bool,
}

impl AdaptiveRateLimiter {
    /// Create a new adaptive rate limiter.
    pub fn new(
        base_requests_per_second: u32,
        error_window_size: usize,
        error_threshold: f64,
    ) -> Self {
        let quota = Quota::per_second(
            NonZeroU32::new(base_requests_per_second).unwrap_or(NonZeroU32::new(10).unwrap())
        );
        let limiter = RateLimiter::direct(quota);

        Self {
            limiter,
            base_quota: base_requests_per_second,
            current_quota: base_requests_per_second,
            error_window: VecDeque::new(),
            window_size: error_window_size,
            error_threshold,
            min_quota: base_requests_per_second / 4, // 25% of base minimum
            max_quota: base_requests_per_second * 2, // 200% of base maximum
            last_adjustment: Instant::now(),
            adjustment_interval: Duration::from_secs(30), // Adjust at most every 30 seconds
        }
    }

    /// Check if a request is allowed (governor interface).
    #[instrument(skip(self))]
    pub async fn check(&self) -> Result<(), governor::NotUntil<Instant>> {
        self.limiter.check()
    }

    /// Check if a request is allowed and consume a permit.
    #[instrument(skip(self))]
    pub async fn check_and_record_request(&mut self) -> Result<(), governor::NotUntil<Instant>> {
        let result = self.limiter.check();
        
        if result.is_ok() {
            // Request was allowed, we'll record success/failure later
            debug!("Rate limit check passed");
        } else {
            debug!("Rate limit check failed");
        }
        
        result
    }

    /// Record the result of a request for adaptive adjustment.
    #[instrument(skip(self))]
    pub fn record_request_result(&mut self, success: bool) {
        let result = RequestResult {
            timestamp: Instant::now(),
            success,
        };

        self.error_window.push_back(result);

        // Keep window size limited
        while self.error_window.len() > self.window_size {
            self.error_window.pop_front();
        }

        // Consider adjusting quota
        if self.should_adjust_quota() {
            self.adjust_quota();
        }

        debug!("Recorded request result: success={}, window_size={}", success, self.error_window.len());
    }

    /// Record a successful request.
    pub fn record_success(&mut self) {
        self.record_request_result(true);
    }

    /// Record a failed request.
    pub fn record_failure(&mut self) {
        self.record_request_result(false);
    }

    /// Check if quota should be adjusted.
    fn should_adjust_quota(&self) -> bool {
        // Need enough samples
        if self.error_window.len() < self.window_size / 2 {
            return false;
        }

        // Don't adjust too frequently
        if self.last_adjustment.elapsed() < self.adjustment_interval {
            return false;
        }

        true
    }

    /// Adjust quota based on current error rate.
    #[instrument(skip(self))]
    fn adjust_quota(&mut self) {
        let error_rate = self.calculate_current_error_rate();
        let old_quota = self.current_quota;

        if error_rate > self.error_threshold {
            // High error rate: reduce quota
            let reduction_factor = 1.0 - (error_rate - self.error_threshold) * 0.5;
            let new_quota = ((self.current_quota as f64) * reduction_factor) as u32;
            self.current_quota = new_quota.max(self.min_quota);
            
            warn!("High error rate {:.2}%, reducing quota from {} to {} req/s", 
                  error_rate * 100.0, old_quota, self.current_quota);
        } else if error_rate < self.error_threshold * 0.5 {
            // Low error rate: gradually increase quota back toward base
            let increase_factor = 1.0 + (self.error_threshold * 0.5 - error_rate) * 0.2;
            let new_quota = ((self.current_quota as f64) * increase_factor) as u32;
            self.current_quota = new_quota.min(self.max_quota).min(self.base_quota);
            
            debug!("Low error rate {:.2}%, increasing quota from {} to {} req/s", 
                   error_rate * 100.0, old_quota, self.current_quota);
        }

        // Update limiter if quota changed
        if self.current_quota != old_quota {
            self.update_limiter_quota();
            self.last_adjustment = Instant::now();
        }
    }

    /// Update the underlying limiter with new quota.
    fn update_limiter_quota(&mut self) {
        let new_quota = Quota::per_second(
            NonZeroU32::new(self.current_quota).unwrap_or(NonZeroU32::new(1).unwrap())
        );
        
        // Create new limiter with updated quota
        self.limiter = RateLimiter::direct(new_quota);
        
        debug!("Updated rate limiter quota to {} req/s", self.current_quota);
    }

    /// Calculate current error rate from the window.
    fn calculate_current_error_rate(&self) -> f64 {
        if self.error_window.is_empty() {
            return 0.0;
        }

        let errors = self.error_window
            .iter()
            .filter(|result| !result.success)
            .count();

        errors as f64 / self.error_window.len() as f64
    }

    /// Get current rate limiting statistics.
    pub fn get_stats(&self) -> RateLimitStats {
        let error_rate = self.calculate_current_error_rate();
        let total_requests = self.error_window.len();
        let successful_requests = self.error_window
            .iter()
            .filter(|result| result.success)
            .count();

        RateLimitStats {
            base_quota: self.base_quota,
            current_quota: self.current_quota,
            error_rate,
            total_requests,
            successful_requests,
            window_size: self.window_size,
            last_adjustment: self.last_adjustment,
        }
    }

    /// Reset to base quota and clear history.
    #[instrument(skip(self))]
    pub fn reset(&mut self) {
        self.current_quota = self.base_quota;
        self.error_window.clear();
        self.update_limiter_quota();
        self.last_adjustment = Instant::now();
        
        debug!("Reset adaptive rate limiter to base quota {}", self.base_quota);
    }

    /// Set new base quota.
    pub fn set_base_quota(&mut self, new_base: u32) {
        self.base_quota = new_base;
        self.min_quota = new_base / 4;
        self.max_quota = new_base * 2;
        
        // If current quota is higher than new base, adjust it
        if self.current_quota > new_base {
            self.current_quota = new_base;
            self.update_limiter_quota();
        }
        
        debug!("Set new base quota to {}", new_base);
    }

    /// Set error threshold for quota adjustment.
    pub fn set_error_threshold(&mut self, threshold: f64) {
        self.error_threshold = threshold.clamp(0.0, 1.0);
        debug!("Set error threshold to {:.2}%", self.error_threshold * 100.0);
    }

    /// Get current quota.
    pub fn get_current_quota(&self) -> u32 {
        self.current_quota
    }
}

/// Rate limiting statistics.
#[derive(Debug, Clone)]
pub struct RateLimitStats {
    pub base_quota: u32,
    pub current_quota: u32,
    pub error_rate: f64,
    pub total_requests: usize,
    pub successful_requests: usize,
    pub window_size: usize,
    pub last_adjustment: Instant,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_adaptive_rate_limiter_creation() {
        let limiter = AdaptiveRateLimiter::new(20, 100, 0.2);
        
        assert_eq!(limiter.base_quota, 20);
        assert_eq!(limiter.current_quota, 20);
        assert_eq!(limiter.window_size, 100);
        assert_eq!(limiter.error_threshold, 0.2);
        assert_eq!(limiter.min_quota, 5); // 25% of 20
        assert_eq!(limiter.max_quota, 40); // 200% of 20
    }

    #[tokio::test]
    async fn test_check_allows_requests() {
        let limiter = AdaptiveRateLimiter::new(10, 100, 0.2);
        
        // Should allow requests initially
        assert!(limiter.check().await.is_ok());
    }

    #[test]
    fn test_record_request_results() {
        let mut limiter = AdaptiveRateLimiter::new(20, 10, 0.2);
        
        limiter.record_success();
        limiter.record_failure();
        limiter.record_success();
        
        let stats = limiter.get_stats();
        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.successful_requests, 2);
        assert!((stats.error_rate - 0.333).abs() < 0.01);
    }

    #[test]
    fn test_error_rate_calculation() {
        let mut limiter = AdaptiveRateLimiter::new(20, 10, 0.2);
        
        // Add 8 successes and 2 failures
        for _ in 0..8 {
            limiter.record_success();
        }
        for _ in 0..2 {
            limiter.record_failure();
        }
        
        let error_rate = limiter.calculate_current_error_rate();
        assert!((error_rate - 0.2).abs() < 0.01); // 20% error rate
    }

    #[test]
    fn test_quota_reduction_on_high_errors() {
        let mut limiter = AdaptiveRateLimiter::new(20, 10, 0.2);
        
        // Fill window with high error rate
        for _ in 0..3 {
            limiter.record_success();
        }
        for _ in 0..7 {
            limiter.record_failure();
        }
        
        // Wait to allow adjustment
        thread::sleep(Duration::from_millis(100));
        
        // Force adjustment check
        limiter.last_adjustment = Instant::now() - Duration::from_secs(60);
        
        let old_quota = limiter.current_quota;
        limiter.adjust_quota();
        
        // Quota should be reduced due to high error rate (70%)
        assert!(limiter.current_quota < old_quota);
        assert!(limiter.current_quota >= limiter.min_quota);
    }

    #[test]
    fn test_quota_increase_on_low_errors() {
        let mut limiter = AdaptiveRateLimiter::new(20, 10, 0.2);
        
        // First reduce quota by introducing high errors
        for _ in 0..3 {
            limiter.record_success();
        }
        for _ in 0..7 {
            limiter.record_failure();
        }
        limiter.last_adjustment = Instant::now() - Duration::from_secs(60);
        limiter.adjust_quota();
        
        let reduced_quota = limiter.current_quota;
        
        // Now introduce low error rate
        limiter.error_window.clear();
        for _ in 0..9 {
            limiter.record_success();
        }
        limiter.record_failure();
        
        limiter.last_adjustment = Instant::now() - Duration::from_secs(60);
        limiter.adjust_quota();
        
        // Quota should increase
        assert!(limiter.current_quota > reduced_quota);
    }

    #[test]
    fn test_reset() {
        let mut limiter = AdaptiveRateLimiter::new(20, 10, 0.2);
        
        // Add some data and adjust quota
        for _ in 0..5 {
            limiter.record_failure();
        }
        for _ in 0..5 {
            limiter.record_success();
        }
        
        limiter.reset();
        
        assert_eq!(limiter.current_quota, limiter.base_quota);
        assert_eq!(limiter.error_window.len(), 0);
    }

    #[test]
    fn test_set_base_quota() {
        let mut limiter = AdaptiveRateLimiter::new(20, 10, 0.2);
        
        limiter.set_base_quota(40);
        
        assert_eq!(limiter.base_quota, 40);
        assert_eq!(limiter.min_quota, 10); // 25% of 40
        assert_eq!(limiter.max_quota, 80); // 200% of 40
    }

    #[test]
    fn test_set_error_threshold() {
        let mut limiter = AdaptiveRateLimiter::new(20, 10, 0.2);
        
        limiter.set_error_threshold(0.1);
        assert_eq!(limiter.error_threshold, 0.1);
        
        // Test bounds
        limiter.set_error_threshold(-0.1);
        assert_eq!(limiter.error_threshold, 0.0);
        
        limiter.set_error_threshold(1.5);
        assert_eq!(limiter.error_threshold, 1.0);
    }

    #[test]
    fn test_get_stats() {
        let mut limiter = AdaptiveRateLimiter::new(20, 10, 0.2);
        
        limiter.record_success();
        limiter.record_failure();
        
        let stats = limiter.get_stats();
        
        assert_eq!(stats.base_quota, 20);
        assert_eq!(stats.current_quota, 20);
        assert_eq!(stats.total_requests, 2);
        assert_eq!(stats.successful_requests, 1);
        assert_eq!(stats.window_size, 10);
        assert!((stats.error_rate - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_window_size_limit() {
        let mut limiter = AdaptiveRateLimiter::new(20, 5, 0.2); // Small window
        
        // Add more than window size
        for _ in 0..10 {
            limiter.record_success();
        }
        
        // Should be limited to window size
        assert_eq!(limiter.error_window.len(), 5);
    }

    #[test]
    fn test_adjustment_interval() {
        let mut limiter = AdaptiveRateLimiter::new(20, 10, 0.2);
        
        // Fill with high error rate
        for _ in 0..8 {
            limiter.record_failure();
        }
        for _ in 0..2 {
            limiter.record_success();
        }
        
        // Should not adjust immediately after last adjustment
        assert!(!limiter.should_adjust_quota());
        
        // After interval, should allow adjustment
        limiter.last_adjustment = Instant::now() - Duration::from_secs(60);
        assert!(limiter.should_adjust_quota());
    }
}
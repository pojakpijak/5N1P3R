//! Circuit breaker for RPC endpoint health tracking.
//!
//! This module provides circuit breaker functionality to temporarily 
//! quarantine unhealthy RPC endpoints and retry them after cooldown.

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::{debug, warn, instrument};

/// State of an RPC endpoint in the circuit breaker.
#[derive(Debug, Clone, PartialEq)]
pub enum EndpointState {
    /// Endpoint is healthy and can be used
    Healthy,
    /// Endpoint is degraded due to failures but still usable
    Degraded,
    /// Endpoint is in cooldown period after too many failures
    CoolingDown,
}

/// Health tracking for individual RPC endpoints.
#[derive(Debug, Clone)]
pub struct EndpointHealth {
    /// Current state of the endpoint
    pub state: EndpointState,
    /// Consecutive failure count
    pub consecutive_failures: u32,
    /// Success rate over recent attempts
    pub success_rate: f64,
    /// Total attempts in current window
    pub total_attempts: usize,
    /// Successful attempts in current window
    pub successful_attempts: usize,
    /// Timestamp of last failure
    pub last_failure: Option<Instant>,
    /// Timestamp when cooldown started
    pub cooldown_start: Option<Instant>,
    /// Recent attempt history (success=true, failure=false)
    pub recent_attempts: Vec<bool>,
}

/// Circuit breaker for managing RPC endpoint health.
pub struct CircuitBreaker {
    /// Health state per endpoint
    endpoint_health: HashMap<String, EndpointHealth>,
    /// Failure threshold before marking endpoint as degraded
    failure_threshold: u32,
    /// Cooldown duration for failed endpoints
    cooldown_duration: Duration,
    /// Sample size for success rate calculation
    sample_size: usize,
    /// Minimum success rate to keep endpoint healthy
    min_success_rate: f64,
}

impl CircuitBreaker {
    /// Create a new circuit breaker.
    pub fn new(
        failure_threshold: u32,
        cooldown_seconds: u64,
        sample_size: usize,
    ) -> Self {
        Self {
            endpoint_health: HashMap::new(),
            failure_threshold,
            cooldown_duration: Duration::from_secs(cooldown_seconds),
            sample_size,
            min_success_rate: 0.3, // 30% minimum success rate
        }
    }

    /// Record a successful request to an endpoint.
    #[instrument(skip(self), fields(endpoint = %endpoint))]
    pub fn record_success(&mut self, endpoint: &str) {
        {
            let health = self.endpoint_health
                .entry(endpoint.to_string())
                .or_insert_with(EndpointHealth::new);

            health.record_success();
        }
        
        self.update_endpoint_state(endpoint);
        
        let health = self.endpoint_health.get(endpoint).unwrap();
        debug!("Recorded success for endpoint {}: {} failures, {:.2}% success rate",
               endpoint, health.consecutive_failures, health.success_rate * 100.0);
    }

    /// Record a failed request to an endpoint.
    #[instrument(skip(self), fields(endpoint = %endpoint))]
    pub fn record_failure(&mut self, endpoint: &str) {
        {
            let health = self.endpoint_health
                .entry(endpoint.to_string())
                .or_insert_with(EndpointHealth::new);

            health.record_failure();
        }
        
        self.update_endpoint_state(endpoint);
        
        let health = self.endpoint_health.get(endpoint).unwrap();
        warn!("Recorded failure for endpoint {}: {} consecutive failures, {:.2}% success rate",
              endpoint, health.consecutive_failures, health.success_rate * 100.0);
    }

    /// Check if an endpoint is available for use.
    #[instrument(skip(self), fields(endpoint = %endpoint))]
    pub fn is_available(&mut self, endpoint: &str) -> bool {
        // Update state before checking availability
        self.update_endpoint_state(endpoint);

        let health = self.endpoint_health
            .entry(endpoint.to_string())
            .or_insert_with(EndpointHealth::new);

        match health.state {
            EndpointState::Healthy => true,
            EndpointState::Degraded => true, // Still usable but not preferred
            EndpointState::CoolingDown => {
                // Check if cooldown period has expired
                if let Some(cooldown_start) = health.cooldown_start {
                    if cooldown_start.elapsed() >= self.cooldown_duration {
                        health.state = EndpointState::Degraded;
                        health.cooldown_start = None;
                        health.consecutive_failures = 0; // Reset for retry
                        debug!("Endpoint {} cooldown expired, moving to degraded state", endpoint);
                        true
                    } else {
                        false
                    }
                } else {
                    // No cooldown start time, shouldn't happen but treat as available
                    true
                }
            }
        }
    }

    /// Get the current state of an endpoint.
    pub fn get_endpoint_state(&self, endpoint: &str) -> EndpointState {
        self.endpoint_health
            .get(endpoint)
            .map(|h| h.state.clone())
            .unwrap_or(EndpointState::Healthy)
    }

    /// Get all healthy endpoints.
    pub fn get_healthy_endpoints(&mut self) -> Vec<String> {
        let mut healthy = Vec::new();
        
        for endpoint in self.endpoint_health.keys().cloned().collect::<Vec<_>>() {
            if self.is_available(&endpoint) {
                let state = self.get_endpoint_state(&endpoint);
                if state == EndpointState::Healthy {
                    healthy.push(endpoint);
                }
            }
        }
        
        healthy
    }

    /// Get all available endpoints (healthy + degraded).
    pub fn get_available_endpoints(&mut self) -> Vec<String> {
        let mut available = Vec::new();
        
        for endpoint in self.endpoint_health.keys().cloned().collect::<Vec<_>>() {
            if self.is_available(&endpoint) {
                available.push(endpoint);
            }
        }
        
        available
    }

    /// Update the state of an endpoint based on its health metrics.
    fn update_endpoint_state(&mut self, endpoint: &str) {
        let health = self.endpoint_health.get_mut(endpoint).unwrap();

        match health.state {
            EndpointState::Healthy => {
                if health.consecutive_failures >= self.failure_threshold {
                    health.state = EndpointState::Degraded;
                    debug!("Endpoint {} degraded: {} consecutive failures", endpoint, health.consecutive_failures);
                }
            },
            EndpointState::Degraded => {
                // Move to cooling down if still failing and success rate is too low
                if health.consecutive_failures >= self.failure_threshold * 2 ||
                   (health.total_attempts >= self.sample_size && health.success_rate < self.min_success_rate) {
                    health.state = EndpointState::CoolingDown;
                    health.cooldown_start = Some(Instant::now());
                    warn!("Endpoint {} entering cooldown: {} failures, {:.2}% success rate", 
                          endpoint, health.consecutive_failures, health.success_rate * 100.0);
                } else if health.consecutive_failures == 0 && health.success_rate > 0.7 {
                    // Recover to healthy if we're doing well
                    health.state = EndpointState::Healthy;
                    debug!("Endpoint {} recovered to healthy state", endpoint);
                }
            },
            EndpointState::CoolingDown => {
                // State transitions handled in is_available()
            }
        }
    }

    /// Get health statistics for all endpoints.
    pub fn get_health_stats(&self) -> HashMap<String, EndpointHealthStats> {
        self.endpoint_health
            .iter()
            .map(|(endpoint, health)| {
                (endpoint.clone(), EndpointHealthStats {
                    state: health.state.clone(),
                    consecutive_failures: health.consecutive_failures,
                    success_rate: health.success_rate,
                    total_attempts: health.total_attempts,
                    successful_attempts: health.successful_attempts,
                })
            })
            .collect()
    }

    /// Reset all endpoints to healthy state.
    pub fn reset_all(&mut self) {
        for health in self.endpoint_health.values_mut() {
            health.state = EndpointState::Healthy;
            health.consecutive_failures = 0;
            health.cooldown_start = None;
            health.recent_attempts.clear();
            health.total_attempts = 0;
            health.successful_attempts = 0;
            health.success_rate = 1.0;
        }
        debug!("Reset all endpoints to healthy state");
    }
}

impl EndpointHealth {
    /// Create new endpoint health tracker.
    fn new() -> Self {
        Self {
            state: EndpointState::Healthy,
            consecutive_failures: 0,
            success_rate: 1.0,
            total_attempts: 0,
            successful_attempts: 0,
            last_failure: None,
            cooldown_start: None,
            recent_attempts: Vec::new(),
        }
    }

    /// Record a successful request.
    fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.last_failure = None;
        
        self.total_attempts += 1;
        self.successful_attempts += 1;
        self.recent_attempts.push(true);
        
        // Keep only recent attempts for rolling window
        if self.recent_attempts.len() > 100 {
            self.recent_attempts.remove(0);
        }
        
        self.update_success_rate();
    }

    /// Record a failed request.
    fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        self.last_failure = Some(Instant::now());
        
        self.total_attempts += 1;
        self.recent_attempts.push(false);
        
        // Keep only recent attempts for rolling window
        if self.recent_attempts.len() > 100 {
            self.recent_attempts.remove(0);
        }
        
        self.update_success_rate();
    }

    /// Update success rate based on recent attempts.
    fn update_success_rate(&mut self) {
        if self.recent_attempts.is_empty() {
            self.success_rate = 1.0;
            return;
        }

        let successes = self.recent_attempts.iter().filter(|&&success| success).count();
        self.success_rate = successes as f64 / self.recent_attempts.len() as f64;
        self.successful_attempts = successes;
        self.total_attempts = self.recent_attempts.len();
    }
}

/// Health statistics for external reporting.
#[derive(Debug, Clone)]
pub struct EndpointHealthStats {
    pub state: EndpointState,
    pub consecutive_failures: u32,
    pub success_rate: f64,
    pub total_attempts: usize,
    pub successful_attempts: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_circuit_breaker_creation() {
        let cb = CircuitBreaker::new(5, 60, 50);
        assert_eq!(cb.failure_threshold, 5);
        assert_eq!(cb.cooldown_duration, Duration::from_secs(60));
        assert_eq!(cb.sample_size, 50);
    }

    #[test]
    fn test_endpoint_initially_healthy() {
        let mut cb = CircuitBreaker::new(3, 60, 50);
        
        // First check availability to populate the endpoint entry
        assert!(cb.is_available("test-endpoint"));
        assert_eq!(cb.get_endpoint_state("test-endpoint"), EndpointState::Healthy);
    }

    #[test]
    fn test_record_success() {
        let mut cb = CircuitBreaker::new(3, 60, 50);
        
        cb.record_success("test-endpoint");
        assert!(cb.is_available("test-endpoint"));
        assert_eq!(cb.get_endpoint_state("test-endpoint"), EndpointState::Healthy);
    }

    #[test]
    fn test_record_failure_degradation() {
        let mut cb = CircuitBreaker::new(3, 60, 50);
        
        // Record failures up to threshold
        for _ in 0..3 {
            cb.record_failure("test-endpoint");
        }
        
        assert_eq!(cb.get_endpoint_state("test-endpoint"), EndpointState::Degraded);
        assert!(cb.is_available("test-endpoint")); // Still available but degraded
    }

    #[test]
    fn test_cooldown_transition() {
        let mut cb = CircuitBreaker::new(2, 60, 50);
        
        // Record enough failures to trigger cooldown
        for _ in 0..5 {
            cb.record_failure("test-endpoint");
        }
        
        assert_eq!(cb.get_endpoint_state("test-endpoint"), EndpointState::CoolingDown);
        assert!(!cb.is_available("test-endpoint"));
    }

    #[test]
    fn test_recovery_after_success() {
        let mut cb = CircuitBreaker::new(3, 60, 50);
        
        // Degrade endpoint
        for _ in 0..3 {
            cb.record_failure("test-endpoint");
        }
        assert_eq!(cb.get_endpoint_state("test-endpoint"), EndpointState::Degraded);
        
        // Recover with successes
        for _ in 0..10 {
            cb.record_success("test-endpoint");
        }
        
        assert_eq!(cb.get_endpoint_state("test-endpoint"), EndpointState::Healthy);
    }

    #[test]
    fn test_get_healthy_endpoints() {
        let mut cb = CircuitBreaker::new(3, 60, 50);
        
        cb.record_success("healthy-1");
        cb.record_success("healthy-2");
        
        for _ in 0..3 {
            cb.record_failure("degraded");
        }
        
        let healthy = cb.get_healthy_endpoints();
        assert!(healthy.contains(&"healthy-1".to_string()));
        assert!(healthy.contains(&"healthy-2".to_string()));
        assert!(!healthy.contains(&"degraded".to_string()));
    }

    #[test]
    fn test_get_available_endpoints() {
        let mut cb = CircuitBreaker::new(3, 60, 50);
        
        cb.record_success("healthy");
        
        for _ in 0..3 {
            cb.record_failure("degraded");
        }
        
        for _ in 0..6 {
            cb.record_failure("cooling-down");
        }
        
        let available = cb.get_available_endpoints();
        assert!(available.contains(&"healthy".to_string()));
        assert!(available.contains(&"degraded".to_string()));
        assert!(!available.contains(&"cooling-down".to_string()));
    }

    #[test]
    fn test_reset_all() {
        let mut cb = CircuitBreaker::new(2, 60, 50);
        
        // Degrade some endpoints
        for _ in 0..3 {
            cb.record_failure("endpoint-1");
            cb.record_failure("endpoint-2");
        }
        
        cb.reset_all();
        
        assert_eq!(cb.get_endpoint_state("endpoint-1"), EndpointState::Healthy);
        assert_eq!(cb.get_endpoint_state("endpoint-2"), EndpointState::Healthy);
    }

    #[test]
    fn test_health_stats() {
        let mut cb = CircuitBreaker::new(3, 60, 50);
        
        cb.record_success("test");
        cb.record_failure("test");
        cb.record_success("test");
        
        let stats = cb.get_health_stats();
        let test_stats = stats.get("test").unwrap();
        
        assert_eq!(test_stats.consecutive_failures, 0); // Reset after success
        assert_eq!(test_stats.total_attempts, 3);
        assert_eq!(test_stats.successful_attempts, 2);
        assert!((test_stats.success_rate - 0.666).abs() < 0.01);
    }
}
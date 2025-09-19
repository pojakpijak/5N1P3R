//! Metrics collection and Prometheus exporter for Oracle system.
//!
//! This module provides metrics collection and optional Prometheus HTTP server
//! for monitoring Oracle performance and health.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn, instrument};

#[cfg(feature = "prometheus_exporter")]
use prometheus::{Counter, Gauge, Histogram, Registry, Encoder, TextEncoder};

#[cfg(feature = "prometheus_exporter")]
use hyper::service::{make_service_fn, service_fn};
#[cfg(feature = "prometheus_exporter")]
use hyper::{Body, Method, Request, Response, Result as HyperResult, Server, StatusCode};

/// Oracle metrics collector.
pub struct OracleMetricsCollector {
    /// Internal metrics storage
    metrics: Arc<RwLock<InternalMetrics>>,
    
    #[cfg(feature = "prometheus_exporter")]
    /// Prometheus registry
    prometheus_registry: Registry,
    
    #[cfg(feature = "prometheus_exporter")]
    /// Prometheus counters
    prometheus_counters: HashMap<String, Counter>,
    
    #[cfg(feature = "prometheus_exporter")]
    /// Prometheus gauges  
    prometheus_gauges: HashMap<String, Gauge>,
    
    #[cfg(feature = "prometheus_exporter")]
    /// Prometheus histograms
    prometheus_histograms: HashMap<String, Histogram>,
}

/// Internal metrics storage.
#[derive(Debug, Default)]
struct InternalMetrics {
    /// Counter metrics
    counters: HashMap<String, u64>,
    /// Gauge metrics
    gauges: HashMap<String, f64>,
    /// Histogram data (simplified)
    histograms: HashMap<String, Vec<f64>>,
    /// Last update times
    last_updates: HashMap<String, Instant>,
}

impl OracleMetricsCollector {
    /// Create a new metrics collector.
    pub fn new() -> Self {
        #[cfg(feature = "prometheus_exporter")]
        {
            let registry = Registry::new();
            let mut counters = HashMap::new();
            let mut gauges = HashMap::new();
            let mut histograms = HashMap::new();

            // Initialize standard Oracle metrics
            Self::register_standard_metrics(&registry, &mut counters, &mut gauges, &mut histograms);

            Self {
                metrics: Arc::new(RwLock::new(InternalMetrics::default())),
                prometheus_registry: registry,
                prometheus_counters: counters,
                prometheus_gauges: gauges,
                prometheus_histograms: histograms,
            }
        }
        
        #[cfg(not(feature = "prometheus_exporter"))]
        {
            Self {
                metrics: Arc::new(RwLock::new(InternalMetrics::default())),
            }
        }
    }

    /// Register standard Oracle metrics.
    #[cfg(feature = "prometheus_exporter")]
    fn register_standard_metrics(
        registry: &Registry,
        counters: &mut HashMap<String, Counter>,
        gauges: &mut HashMap<String, Gauge>,
        histograms: &mut HashMap<String, Histogram>,
    ) {
        use prometheus::opts;

        // Counters
        let oracle_scored_total = Counter::with_opts(opts!(
            "oracle_scored_total",
            "Total number of tokens scored by Oracle"
        )).unwrap();
        registry.register(Box::new(oracle_scored_total.clone())).unwrap();
        counters.insert("oracle_scored_total".to_string(), oracle_scored_total);

        let oracle_high_score_total = Counter::with_opts(opts!(
            "oracle_high_score_total", 
            "Total number of high-scoring tokens (>=80)"
        )).unwrap();
        registry.register(Box::new(oracle_high_score_total.clone())).unwrap();
        counters.insert("oracle_high_score_total".to_string(), oracle_high_score_total);

        let oracle_cache_hits_total = Counter::with_opts(opts!(
            "oracle_cache_hits_total",
            "Total number of cache hits"
        )).unwrap();
        registry.register(Box::new(oracle_cache_hits_total.clone())).unwrap();
        counters.insert("oracle_cache_hits_total".to_string(), oracle_cache_hits_total);

        let oracle_cache_misses_total = Counter::with_opts(opts!(
            "oracle_cache_misses_total",
            "Total number of cache misses"
        )).unwrap();
        registry.register(Box::new(oracle_cache_misses_total.clone())).unwrap();
        counters.insert("oracle_cache_misses_total".to_string(), oracle_cache_misses_total);

        let oracle_rpc_errors_total = Counter::with_opts(opts!(
            "oracle_rpc_errors_total",
            "Total number of RPC errors"
        )).unwrap();
        registry.register(Box::new(oracle_rpc_errors_total.clone())).unwrap();
        counters.insert("oracle_rpc_errors_total".to_string(), oracle_rpc_errors_total);

        let oracle_api_errors_total = Counter::with_opts(opts!(
            "oracle_api_errors_total",
            "Total number of API errors"
        )).unwrap();
        registry.register(Box::new(oracle_api_errors_total.clone())).unwrap();
        counters.insert("oracle_api_errors_total".to_string(), oracle_api_errors_total);

        // Gauges
        let oracle_avg_scoring_time = Gauge::with_opts(opts!(
            "oracle_avg_scoring_time_seconds",
            "Average scoring time in seconds"
        )).unwrap();
        registry.register(Box::new(oracle_avg_scoring_time.clone())).unwrap();
        gauges.insert("oracle_avg_scoring_time_seconds".to_string(), oracle_avg_scoring_time);

        // Histograms
        let oracle_scoring_duration = Histogram::with_opts(
            prometheus::HistogramOpts::new(
                "oracle_scoring_duration_seconds",
                "Distribution of scoring durations"
            ).buckets(vec![0.001, 0.01, 0.1, 0.5, 1.0, 5.0, 10.0])
        ).unwrap();
        registry.register(Box::new(oracle_scoring_duration.clone())).unwrap();
        histograms.insert("oracle_scoring_duration_seconds".to_string(), oracle_scoring_duration);
    }

    /// Increment a counter metric.
    #[instrument(skip(self), fields(metric = %name))]
    pub async fn increment_counter(&self, name: &str) {
        let mut metrics = self.metrics.write().await;
        *metrics.counters.entry(name.to_string()).or_insert(0) += 1;
        metrics.last_updates.insert(name.to_string(), Instant::now());

        #[cfg(feature = "prometheus_exporter")]
        {
            if let Some(counter) = self.prometheus_counters.get(name) {
                counter.inc();
            }
        }

        debug!("Incremented counter: {}", name);
    }

    /// Set a gauge metric.
    #[instrument(skip(self), fields(metric = %name, value = %value))]
    pub async fn set_gauge(&self, name: &str, value: f64) {
        let mut metrics = self.metrics.write().await;
        metrics.gauges.insert(name.to_string(), value);
        metrics.last_updates.insert(name.to_string(), Instant::now());

        #[cfg(feature = "prometheus_exporter")]
        {
            if let Some(gauge) = self.prometheus_gauges.get(name) {
                gauge.set(value);
            }
        }

        debug!("Set gauge {}: {}", name, value);
    }

    /// Record a histogram value.
    #[instrument(skip(self), fields(metric = %name, value = %value))]
    pub async fn record_histogram(&self, name: &str, value: f64) {
        let mut metrics = self.metrics.write().await;
        metrics.histograms.entry(name.to_string()).or_insert_with(Vec::new).push(value);
        metrics.last_updates.insert(name.to_string(), Instant::now());

        #[cfg(feature = "prometheus_exporter")]
        {
            if let Some(histogram) = self.prometheus_histograms.get(name) {
                histogram.observe(value);
            }
        }

        debug!("Recorded histogram {}: {}", name, value);
    }

    /// Record timing for scoring operations.
    pub async fn record_scoring_time(&self, duration: Duration) {
        let seconds = duration.as_secs_f64();
        self.record_histogram("oracle_scoring_duration_seconds", seconds).await;
        self.set_gauge("oracle_avg_scoring_time_seconds", seconds).await;
    }

    /// Get current metric values.
    pub async fn get_metrics_snapshot(&self) -> MetricsSnapshot {
        let metrics = self.metrics.read().await;
        
        MetricsSnapshot {
            counters: metrics.counters.clone(),
            gauges: metrics.gauges.clone(),
            histograms: metrics.histograms.clone(),
            timestamp: Instant::now(),
        }
    }

    /// Get Prometheus metrics text (if feature enabled).
    #[cfg(feature = "prometheus_exporter")]
    pub fn get_prometheus_metrics(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let encoder = TextEncoder::new();
        let metric_families = self.prometheus_registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        Ok(String::from_utf8(buffer)?)
    }

    #[cfg(not(feature = "prometheus_exporter"))]
    pub fn get_prometheus_metrics(&self) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Ok("Prometheus exporter not enabled".to_string())
    }

    /// Start Prometheus HTTP server (if feature enabled).
    #[cfg(feature = "prometheus_exporter")]
    pub async fn start_metrics_server(
        &self,
        addr: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use std::convert::Infallible;
        use std::net::SocketAddr;

        let registry = self.prometheus_registry.clone();
        
        let make_svc = make_service_fn(move |_conn| {
            let registry = registry.clone();
            async move {
                Ok::<_, Infallible>(service_fn(move |req| {
                    let registry = registry.clone();
                    async move { serve_metrics(req, registry).await }
                }))
            }
        });

        let addr: SocketAddr = addr.parse()?;
        let server = Server::bind(&addr).serve(make_svc);

        info!("Starting Prometheus metrics server on {}", addr);
        
        if let Err(e) = server.await {
            warn!("Metrics server error: {}", e);
        }

        Ok(())
    }

    #[cfg(not(feature = "prometheus_exporter"))]
    pub async fn start_metrics_server(
        &self,
        _addr: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        warn!("Prometheus exporter not enabled, cannot start metrics server");
        Ok(())
    }
}

/// Snapshot of current metrics.
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub counters: HashMap<String, u64>,
    pub gauges: HashMap<String, f64>,
    pub histograms: HashMap<String, Vec<f64>>,
    pub timestamp: Instant,
}

/// Serve Prometheus metrics via HTTP.
#[cfg(feature = "prometheus_exporter")]
async fn serve_metrics(
    req: Request<Body>,
    registry: Registry,
) -> HyperResult<Response<Body>> {
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/metrics") => {
            let encoder = TextEncoder::new();
            let metric_families = registry.gather();
            let mut buffer = Vec::new();
            
            if let Err(e) = encoder.encode(&metric_families, &mut buffer) {
                warn!("Failed to encode metrics: {}", e);
                return Ok(Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("Failed to encode metrics"))?);
            }

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/plain; version=0.0.4")
                .body(Body::from(buffer))?)
        }
        (&Method::GET, "/health") => {
            Ok(Response::builder()
                .status(StatusCode::OK)
                .body(Body::from("OK"))?)
        }
        _ => {
            Ok(Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("Not Found"))?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_metrics_collector_creation() {
        let collector = OracleMetricsCollector::new();
        
        // Should create without panicking
        let snapshot = collector.get_metrics_snapshot().await;
        assert!(snapshot.counters.is_empty());
        assert!(snapshot.gauges.is_empty());
        assert!(snapshot.histograms.is_empty());
    }

    #[tokio::test]
    async fn test_increment_counter() {
        let collector = OracleMetricsCollector::new();
        
        collector.increment_counter("test_counter").await;
        collector.increment_counter("test_counter").await;
        
        let snapshot = collector.get_metrics_snapshot().await;
        assert_eq!(snapshot.counters.get("test_counter"), Some(&2));
    }

    #[tokio::test]
    async fn test_set_gauge() {
        let collector = OracleMetricsCollector::new();
        
        collector.set_gauge("test_gauge", 42.5).await;
        
        let snapshot = collector.get_metrics_snapshot().await;
        assert_eq!(snapshot.gauges.get("test_gauge"), Some(&42.5));
    }

    #[tokio::test]
    async fn test_record_histogram() {
        let collector = OracleMetricsCollector::new();
        
        collector.record_histogram("test_histogram", 1.5).await;
        collector.record_histogram("test_histogram", 2.5).await;
        
        let snapshot = collector.get_metrics_snapshot().await;
        let histogram = snapshot.histograms.get("test_histogram").unwrap();
        assert_eq!(histogram.len(), 2);
        assert!(histogram.contains(&1.5));
        assert!(histogram.contains(&2.5));
    }

    #[tokio::test]
    async fn test_record_scoring_time() {
        let collector = OracleMetricsCollector::new();
        
        let duration = Duration::from_millis(500);
        collector.record_scoring_time(duration).await;
        
        let snapshot = collector.get_metrics_snapshot().await;
        
        // Should have recorded both histogram and gauge
        assert!(snapshot.histograms.contains_key("oracle_scoring_duration_seconds"));
        assert!(snapshot.gauges.contains_key("oracle_avg_scoring_time_seconds"));
        
        let avg_time = snapshot.gauges.get("oracle_avg_scoring_time_seconds").unwrap();
        assert!((*avg_time - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_get_prometheus_metrics() {
        let collector = OracleMetricsCollector::new();
        
        // Should not panic
        let result = collector.get_prometheus_metrics();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_metrics_snapshot() {
        let collector = OracleMetricsCollector::new();
        
        collector.increment_counter("counter1").await;
        collector.set_gauge("gauge1", 123.45).await;
        collector.record_histogram("hist1", 0.5).await;
        
        let snapshot = collector.get_metrics_snapshot().await;
        
        assert_eq!(snapshot.counters.get("counter1"), Some(&1));
        assert_eq!(snapshot.gauges.get("gauge1"), Some(&123.45));
        assert_eq!(snapshot.histograms.get("hist1").unwrap().len(), 1);
    }
}
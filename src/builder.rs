//! Builder pattern for configuring instrumented Neo4j connections
//!
//! This module provides a fluent builder API for creating instrumented Neo4j
//! connections with configurable tracing and metrics collection.

use crate::graph::InstrumentedGraph;
use crate::metrics::{MetricsBuilder, Neo4jMetrics};
use neo4rs::{Config, Graph};
use opentelemetry::metrics::Meter;
use std::sync::Arc;

/// Builder for creating an instrumented Neo4j graph connection with configurable telemetry
///
/// # Example
///
/// ```rust,ignore
/// use otel_instrumentation_neo4jrs::InstrumentedGraphBuilder;
/// use neo4rs::ConfigBuilder;
/// use opentelemetry::metrics::MeterProvider;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let config = ConfigBuilder::default()
///     .uri("bolt://localhost:7687")
///     .user("neo4j")
///     .password("password")
///     .build()?;
///
/// let meter_provider = // ... initialize meter provider
/// let meter = meter_provider.meter("neo4j");
///
/// let graph = InstrumentedGraphBuilder::new(config)
///     .with_tracing(true)
///     .with_metrics(meter)
///     .with_service_name("my-service")
///     .build()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct InstrumentedGraphBuilder {
    config: Config,
    enable_tracing: bool,
    metrics_builder: MetricsBuilder,
    service_name: Option<String>,
    record_statement: bool,
    max_statement_length: usize,
}

impl InstrumentedGraphBuilder {
    /// Create a new builder with the given Neo4j configuration
    ///
    /// # Arguments
    ///
    /// * `config` - The Neo4j connection configuration
    pub fn new(config: Config) -> Self {
        Self {
            config,
            enable_tracing: true,
            metrics_builder: MetricsBuilder::new(),
            service_name: None,
            record_statement: false,
            max_statement_length: 1024,
        }
    }
    
    /// Enable or disable tracing
    ///
    /// Tracing is enabled by default.
    ///
    /// # Arguments
    ///
    /// * `enabled` - Whether to enable tracing
    pub fn with_tracing(mut self, enabled: bool) -> Self {
        self.enable_tracing = enabled;
        self
    }
    
    /// Enable metrics collection with the provided meter
    ///
    /// # Arguments
    ///
    /// * `meter` - The OpenTelemetry meter to use for metrics
    pub fn with_metrics(mut self, meter: Meter) -> Self {
        self.metrics_builder = self.metrics_builder.with_meter(meter);
        self
    }
    
    /// Set the service name for telemetry
    ///
    /// # Arguments
    ///
    /// * `name` - The service name
    pub fn with_service_name(mut self, name: impl Into<String>) -> Self {
        self.service_name = Some(name.into());
        self
    }
    
    /// Enable recording of Cypher statements in spans
    ///
    /// **Warning**: This may expose sensitive information in your queries.
    /// Only enable in development or ensure your queries don't contain sensitive data.
    ///
    /// # Arguments
    ///
    /// * `enabled` - Whether to record statements
    pub fn with_statement_recording(mut self, enabled: bool) -> Self {
        self.record_statement = enabled;
        self
    }
    
    /// Set the maximum length for recorded statements
    ///
    /// Statements longer than this will be truncated.
    ///
    /// # Arguments
    ///
    /// * `length` - The maximum statement length
    pub fn with_max_statement_length(mut self, length: usize) -> Self {
        self.max_statement_length = length;
        self
    }
    
    /// Build the instrumented graph connection
    ///
    /// # Errors
    ///
    /// Returns an error if the connection to Neo4j fails or if
    /// connection information cannot be retrieved from the server.
    pub async fn build(self) -> Result<InstrumentedGraph, neo4rs::Error> {
        let graph = Graph::connect(self.config).await?;
        let metrics = self.metrics_builder.build();
        
        // Increment connection counter if metrics are enabled
        if let Some(ref m) = metrics {
            m.increment_connections();
        }
        
        InstrumentedGraph::with_options(
            graph,
            self.enable_tracing,
            metrics,
            self.service_name,
            self.record_statement,
            self.max_statement_length,
        ).await
    }
}

/// Configuration for telemetry behavior
#[derive(Clone, Debug)]
pub struct TelemetryConfig {
    /// Whether tracing is enabled
    pub tracing_enabled: bool,
    /// Whether to record Cypher statements in spans
    pub record_statement: bool,
    /// Maximum length for recorded statements
    pub max_statement_length: usize,
    /// Service name for telemetry
    pub service_name: Option<String>,
    /// Metrics instance if metrics are enabled
    pub metrics: Option<Arc<Neo4jMetrics>>,
}

impl TelemetryConfig {
    /// Create a new configuration with defaults
    pub fn new() -> Self {
        Self::default()
    }
    
    /// Create a configuration with tracing only (no metrics)
    pub fn tracing_only() -> Self {
        Self {
            tracing_enabled: true,
            record_statement: false,
            max_statement_length: 1024,
            service_name: None,
            metrics: None,
        }
    }
    
    /// Create a configuration with both tracing and metrics
    pub fn with_metrics(meter: Meter) -> Self {
        Self {
            tracing_enabled: true,
            record_statement: false,
            max_statement_length: 1024,
            service_name: None,
            metrics: Some(Arc::new(Neo4jMetrics::new(meter))),
        }
    }
    
    /// Check if any telemetry is enabled
    pub fn is_enabled(&self) -> bool {
        self.tracing_enabled || self.metrics.is_some()
    }
    
    /// Check if metrics are enabled
    pub fn has_metrics(&self) -> bool {
        self.metrics.is_some()
    }
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            tracing_enabled: true,
            record_statement: false,
            max_statement_length: 1024,
            service_name: std::env::var("SERVICE_NAME").ok(),
            metrics: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo4rs::ConfigBuilder;
    
    #[test]
    fn test_builder_defaults() {
        let config = ConfigBuilder::default()
            .uri("bolt://localhost:7687")
            .build()
            .unwrap();
        
        let builder = InstrumentedGraphBuilder::new(config);
        assert!(builder.enable_tracing);
        assert!(!builder.record_statement);
        assert_eq!(builder.max_statement_length, 1024);
    }
    
    #[test]
    fn test_telemetry_config_defaults() {
        let config = TelemetryConfig::default();
        assert!(config.tracing_enabled);
        assert!(!config.record_statement);
        assert_eq!(config.max_statement_length, 1024);
        assert!(config.metrics.is_none());
    }
    
    #[test]
    fn test_telemetry_config_tracing_only() {
        let config = TelemetryConfig::tracing_only();
        assert!(config.tracing_enabled);
        assert!(config.is_enabled());
        assert!(!config.has_metrics());
    }
}
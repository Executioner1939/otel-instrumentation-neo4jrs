//! Builder pattern for configuring instrumented Neo4j connections
//!
//! This module provides a flexible API for creating instrumented Neo4j
//! connections with optional tracing and metrics collection.
//! All providers must be explicitly passed - no global providers are used.

use crate::graph::InstrumentedGraphConfig;
use opentelemetry::global::BoxedTracer;
use opentelemetry::metrics::Meter;

/// Builder for creating an instrumented Neo4j graph connection with configurable telemetry
///
/// This builder follows the pay-for-what-you-use principle. All providers must be
/// explicitly passed - no global providers are used.
///
/// # Example
///
/// ```rust,ignore
/// let graph = InstrumentedGraph::builder()
///     .with_tracer(my_tracer)        // Optional - only if you want tracing
///     .with_meter(my_meter)          // Optional - only if you want metrics
///     .build()                       // Returns InstrumentedGraphConfig
///     .connect("bolt://localhost:7687", "neo4j", "password")
///     .await?;
/// ```
pub struct InstrumentedGraphBuilder {
    tracer: Option<BoxedTracer>,
    meter: Option<Meter>,
}

impl InstrumentedGraphBuilder {
    /// Create a new builder with no telemetry enabled
    #[must_use]
    pub fn new() -> Self {
        Self {
            tracer: None,
            meter: None,
        }
    }

    /// Add a tracer for tracing instrumentation
    ///
    /// # Arguments
    ///
    /// * `tracer` - The BoxedTracer instance to use for creating spans
    #[must_use]
    pub fn with_tracer(mut self, tracer: BoxedTracer) -> Self {
        self.tracer = Some(tracer);
        self
    }

    /// Add a meter for metrics collection
    ///
    /// # Arguments
    ///
    /// * `meter` - The meter instance to use for creating metrics
    #[must_use]
    pub fn with_meter(mut self, meter: Meter) -> Self {
        self.meter = Some(meter);
        self
    }

    /// Build the configured instrumented graph wrapper
    ///
    /// Returns an `InstrumentedGraphConfig` that can be used to connect to Neo4j
    /// using the same method signatures as the underlying neo4rs library.
    #[must_use]
    pub fn build(self) -> InstrumentedGraphConfig {
        InstrumentedGraphConfig::new(self.tracer, self.meter)
    }
}

impl Default for InstrumentedGraphBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_defaults() {
        let builder = InstrumentedGraphBuilder::new();
        assert!(builder.tracer.is_none());
        assert!(builder.meter.is_none());
    }
}

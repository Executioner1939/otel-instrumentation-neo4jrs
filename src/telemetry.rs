//! Telemetry utilities for OpenTelemetry instrumentation
//!
//! This module provides utilities for managing OpenTelemetry tracing and metrics
//! with explicit provider passing - no global providers are used.

use opentelemetry::global::BoxedTracer;
use opentelemetry::metrics::Meter;

/// Telemetry configuration for an instrumented Neo4j connection
///
/// All providers must be explicitly passed - no global providers are used
#[derive(Debug)]
pub struct TelemetryConfig {
    pub tracer: Option<BoxedTracer>,
    pub meter: Option<Meter>,
}

impl TelemetryConfig {
    /// Create a new telemetry configuration with no instrumentation
    #[must_use]
    pub fn new() -> Self {
        Self {
            tracer: None,
            meter: None,
        }
    }

    /// Create a telemetry configuration with custom providers
    #[must_use]
    pub fn with_providers(tracer: Option<BoxedTracer>, meter: Option<Meter>) -> Self {
        Self { tracer, meter }
    }
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self::new()
    }
}

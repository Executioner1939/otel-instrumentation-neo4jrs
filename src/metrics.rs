//! OpenTelemetry metrics support for Neo4j operations
//!
//! This module provides comprehensive metrics collection for Neo4j database operations,
//! including query execution times, transaction durations, error rates, and connection statistics.

use opentelemetry::metrics::{Counter, Histogram, Meter, UpDownCounter};
use opentelemetry::KeyValue;
use std::sync::Arc;
use std::time::Duration;

/// Neo4j metrics collector that tracks various database operation metrics
#[derive(Clone, Debug)]
pub struct Neo4jMetrics {
    /// Total number of queries executed
    queries_total: Counter<u64>,
    /// Duration of query execution in milliseconds
    query_duration: Histogram<f64>,
    /// Total number of transactions started
    transactions_total: Counter<u64>,
    /// Duration of transaction execution in milliseconds
    transaction_duration: Histogram<f64>,
    /// Total number of errors encountered
    errors_total: Counter<u64>,
    /// Number of active database connections
    active_connections: UpDownCounter<i64>,
    /// Number of transaction commits
    transaction_commits: Counter<u64>,
    /// Number of transaction rollbacks
    transaction_rollbacks: Counter<u64>,
}

impl Neo4jMetrics {
    /// Create a new metrics instance with the provided meter
    ///
    /// # Arguments
    ///
    /// * `meter` - The OpenTelemetry meter to use for creating metrics
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use opentelemetry::metrics::MeterProvider;
    /// use otel_instrumentation_neo4jrs::metrics::Neo4jMetrics;
    ///
    /// let meter_provider = // ... initialize meter provider
    /// let meter = meter_provider.meter("neo4j");
    /// let metrics = Neo4jMetrics::new(&meter);
    /// ```
    #[must_use]
    pub fn new(meter: &Meter) -> Self {
        Self {
            queries_total: meter
                .u64_counter("neo4j.queries.total")
                .with_description("Total number of Neo4j queries executed")
                .build(),

            query_duration: meter
                .f64_histogram("neo4j.query.duration")
                .with_description("Duration of Neo4j query execution in milliseconds")
                .build(),

            transactions_total: meter
                .u64_counter("neo4j.transactions.total")
                .with_description("Total number of Neo4j transactions started")
                .build(),

            transaction_duration: meter
                .f64_histogram("neo4j.transaction.duration")
                .with_description("Duration of Neo4j transactions in milliseconds")
                .build(),

            errors_total: meter
                .u64_counter("neo4j.errors.total")
                .with_description("Total number of Neo4j errors encountered")
                .build(),

            active_connections: meter
                .i64_up_down_counter("neo4j.connections.active")
                .with_description("Number of active Neo4j connections")
                .build(),

            transaction_commits: meter
                .u64_counter("neo4j.transaction.commits")
                .with_description("Number of successful transaction commits")
                .build(),

            transaction_rollbacks: meter
                .u64_counter("neo4j.transaction.rollbacks")
                .with_description("Number of transaction rollbacks")
                .build(),
        }
    }

    /// Record a query execution
    ///
    /// # Arguments
    ///
    /// * `duration` - The duration of the query execution
    /// * `success` - Whether the query executed successfully
    /// * `operation` - The type of operation (e.g., "MATCH", "CREATE", "MERGE")
    /// * `database` - The database name
    pub fn record_query(
        &self,
        duration: Duration,
        success: bool,
        operation: Option<&str>,
        database: &str,
    ) {
        let mut attributes = vec![
            KeyValue::new("success", success),
            KeyValue::new("database", database.to_string()),
        ];

        if let Some(op) = operation {
            attributes.push(KeyValue::new("operation", op.to_string()));
        }

        self.queries_total.add(1, &attributes);
        // Convert duration to milliseconds safely
        // For durations up to ~24 days, this will be accurate to the millisecond
        let millis = duration.as_secs_f64() * 1000.0;
        self.query_duration.record(millis, &attributes);

        if !success {
            self.errors_total.add(1, &attributes);
        }
    }

    /// Record a transaction start
    ///
    /// # Arguments
    ///
    /// * `database` - The database name
    pub fn record_transaction_start(&self, database: &str) {
        let attributes = vec![KeyValue::new("database", database.to_string())];

        self.transactions_total.add(1, &attributes);
    }

    /// Record a transaction completion
    ///
    /// # Arguments
    ///
    /// * `duration` - The duration of the transaction
    /// * `committed` - Whether the transaction was committed (true) or rolled back (false)
    /// * `database` - The database name
    pub fn record_transaction_end(&self, duration: Duration, committed: bool, database: &str) {
        let attributes = vec![
            KeyValue::new("database", database.to_string()),
            KeyValue::new("outcome", if committed { "commit" } else { "rollback" }),
        ];

        // Convert duration to milliseconds safely
        let millis = duration.as_secs_f64() * 1000.0;
        self.transaction_duration.record(millis, &attributes);

        if committed {
            self.transaction_commits.add(1, &attributes);
        } else {
            self.transaction_rollbacks.add(1, &attributes);
        }
    }

    /// Increment the active connections counter
    pub fn increment_connections(&self) {
        self.active_connections.add(1, &[]);
    }

    /// Decrement the active connections counter
    pub fn decrement_connections(&self) {
        self.active_connections.add(-1, &[]);
    }

    /// Record an error
    ///
    /// # Arguments
    ///
    /// * `error_type` - The type/category of the error
    /// * `operation` - The operation that caused the error
    /// * `database` - The database name
    pub fn record_error(&self, error_type: &str, operation: Option<&str>, database: &str) {
        let mut attributes = vec![
            KeyValue::new("error_type", error_type.to_string()),
            KeyValue::new("database", database.to_string()),
        ];

        if let Some(op) = operation {
            attributes.push(KeyValue::new("operation", op.to_string()));
        }

        self.errors_total.add(1, &attributes);
    }
}

/// Builder for configuring a metrics collection
pub struct MetricsBuilder {
    meter: Option<Meter>,
    enabled: bool,
}

impl MetricsBuilder {
    /// Create a new metrics builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            meter: None,
            enabled: false,
        }
    }

    /// Enable a metrics collection with the provided meter
    ///
    /// # Arguments
    ///
    /// * `meter` - The OpenTelemetry meter to use
    #[must_use]
    pub fn with_meter(mut self, meter: Meter) -> Self {
        self.meter = Some(meter);
        self.enabled = true;
        self
    }

    /// Build the metrics instance
    ///
    /// Returns `None` if metrics are not enabled
    #[must_use]
    pub fn build(self) -> Option<Arc<Neo4jMetrics>> {
        if self.enabled {
            self.meter.as_ref().map(Neo4jMetrics::new).map(Arc::new)
        } else {
            None
        }
    }
}

impl Default for MetricsBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Timer utility for measuring operation durations
pub struct OperationTimer {
    start: std::time::Instant,
}

impl OperationTimer {
    /// Start a new timer
    #[must_use]
    pub fn start() -> Self {
        Self {
            start: std::time::Instant::now(),
        }
    }

    /// Get the elapsed duration
    #[must_use]
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Record the elapsed time to metrics and return the duration
    ///
    /// # Arguments
    ///
    /// * `metrics` - The metrics instance to record to
    /// * `success` - Whether the operation was successful
    /// * `operation` - The operation type
    /// * `database` - The database name
    #[must_use]
    pub fn record_query(
        self,
        metrics: &Neo4jMetrics,
        success: bool,
        operation: Option<&str>,
        database: &str,
    ) -> Duration {
        let duration = self.elapsed();
        metrics.record_query(duration, success, operation, database);
        duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::metrics::MeterProvider as _;
    use opentelemetry_sdk::metrics::SdkMeterProvider;

    #[test]
    fn test_metrics_creation() {
        let provider = SdkMeterProvider::default();
        let meter = provider.meter("test");
        let metrics = Neo4jMetrics::new(&meter);

        // Test that we can record metrics without panicking
        metrics.record_query(Duration::from_millis(100), true, Some("MATCH"), "neo4j");
        metrics.record_transaction_start("neo4j");
        metrics.record_transaction_end(Duration::from_secs(1), true, "neo4j");
        metrics.increment_connections();
        metrics.decrement_connections();
        metrics.record_error("connection", Some("MATCH"), "neo4j");
    }

    #[test]
    fn test_metrics_builder() {
        let provider = SdkMeterProvider::default();
        let meter = provider.meter("test");

        let metrics = MetricsBuilder::new().with_meter(meter).build();

        assert!(metrics.is_some());
    }

    #[test]
    fn test_metrics_builder_disabled() {
        let metrics = MetricsBuilder::new().build();
        assert!(metrics.is_none());
    }

    #[test]
    fn test_operation_timer() {
        let provider = SdkMeterProvider::default();
        let meter = provider.meter("test");
        let metrics = Neo4jMetrics::new(&meter);

        let timer = OperationTimer::start();
        std::thread::sleep(Duration::from_millis(10));
        let duration = timer.record_query(&metrics, true, Some("CREATE"), "testdb");

        assert!(duration.as_millis() >= 10);
    }
}

# OpenTelemetry Instrumentation Architecture Recommendations

## Analysis of actix-web-opentelemetry Patterns

Based on the actix-web-opentelemetry implementation, here are the key architectural patterns that should be adopted across your OpenTelemetry instrumentation crates:

### 1. Client Instrumentation Wrapper Pattern
The `InstrumentedClientRequest` pattern provides a clean wrapper around the original client that:
- Preserves the original API surface
- Adds instrumentation transparently
- Chains naturally with existing method calls

### 2. Extension Trait Pattern
The `ClientExt` trait allows for:
- Easy opt-in instrumentation via trait import
- Minimal disruption to existing code
- Clear separation of instrumentation logic

### 3. Metrics Middleware with Builder Pattern
Separate metrics tracking that can be:
- Enabled independently from tracing
- Configured via builder pattern
- Combined with tracing for comprehensive observability

### 4. Centralized Utility Functions
Consolidate common functionality for:
- Attribute extraction
- Span naming conventions
- Error recording
- Metric recording

## Recommended Implementation for otel-instrumentation-neo4jrs

### Phase 1: Add Metrics Support

```rust
// src/metrics.rs
use opentelemetry::metrics::{Counter, Histogram, Meter, Unit};
use std::sync::Arc;
use std::time::Duration;

pub struct Neo4jMetrics {
    queries_total: Counter<u64>,
    query_duration: Histogram<f64>,
    transactions_total: Counter<u64>,
    transaction_duration: Histogram<f64>,
    errors_total: Counter<u64>,
    active_connections: UpDownCounter<i64>,
}

impl Neo4jMetrics {
    pub fn new(meter: Meter) -> Self {
        Self {
            queries_total: meter
                .u64_counter("neo4j.queries.total")
                .with_description("Total number of Neo4j queries executed")
                .with_unit(Unit::new("{query}"))
                .init(),
            
            query_duration: meter
                .f64_histogram("neo4j.query.duration")
                .with_description("Duration of Neo4j query execution")
                .with_unit(Unit::new("ms"))
                .init(),
            
            transactions_total: meter
                .u64_counter("neo4j.transactions.total")
                .with_description("Total number of Neo4j transactions")
                .with_unit(Unit::new("{transaction}"))
                .init(),
            
            transaction_duration: meter
                .f64_histogram("neo4j.transaction.duration")
                .with_description("Duration of Neo4j transactions")
                .with_unit(Unit::new("ms"))
                .init(),
            
            errors_total: meter
                .u64_counter("neo4j.errors.total")
                .with_description("Total number of Neo4j errors")
                .with_unit(Unit::new("{error}"))
                .init(),
            
            active_connections: meter
                .i64_up_down_counter("neo4j.connections.active")
                .with_description("Number of active Neo4j connections")
                .with_unit(Unit::new("{connection}"))
                .init(),
        }
    }
    
    pub fn record_query(&self, duration: Duration, success: bool, operation: &str) {
        let attributes = vec![
            KeyValue::new("operation", operation.to_string()),
            KeyValue::new("success", success),
        ];
        
        self.queries_total.add(1, &attributes);
        self.query_duration.record(duration.as_millis() as f64, &attributes);
        
        if !success {
            self.errors_total.add(1, &attributes);
        }
    }
}
```

### Phase 2: Builder Pattern for Configuration

```rust
// src/builder.rs
use crate::graph::InstrumentedGraph;
use crate::metrics::Neo4jMetrics;
use neo4rs::{Config, Graph};
use opentelemetry::metrics::Meter;
use std::sync::Arc;

pub struct InstrumentedGraphBuilder {
    config: Config,
    enable_tracing: bool,
    enable_metrics: bool,
    meter: Option<Meter>,
    service_name: Option<String>,
}

impl InstrumentedGraphBuilder {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            enable_tracing: true,
            enable_metrics: false,
            meter: None,
            service_name: None,
        }
    }
    
    pub fn with_tracing(mut self, enabled: bool) -> Self {
        self.enable_tracing = enabled;
        self
    }
    
    pub fn with_metrics(mut self, meter: Meter) -> Self {
        self.enable_metrics = true;
        self.meter = Some(meter);
        self
    }
    
    pub fn with_service_name(mut self, name: impl Into<String>) -> Self {
        self.service_name = Some(name.into());
        self
    }
    
    pub async fn build(self) -> Result<InstrumentedGraph, neo4rs::Error> {
        let graph = Graph::connect(self.config).await?;
        let metrics = self.meter.map(Neo4jMetrics::new).map(Arc::new);
        
        Ok(InstrumentedGraph::with_options(
            graph,
            self.enable_tracing,
            metrics,
        ).await?)
    }
}
```

### Phase 3: Extension Trait Pattern

```rust
// src/ext.rs
use neo4rs::{Graph, Query};
use crate::graph::InstrumentedGraph;

/// Extension trait for Neo4j Graph to add instrumentation capabilities
pub trait GraphExt: Sized {
    /// Wrap the graph with OpenTelemetry instrumentation
    fn with_telemetry(self) -> impl std::future::Future<Output = Result<InstrumentedGraph, neo4rs::Error>> + Send;
    
    /// Execute a query with ad-hoc tracing
    fn execute_traced(
        &self,
        query: Query,
    ) -> impl std::future::Future<Output = Result<(), neo4rs::Error>> + Send;
}

impl GraphExt for Graph {
    async fn with_telemetry(self) -> Result<InstrumentedGraph, neo4rs::Error> {
        InstrumentedGraph::from_graph(self).await
    }
    
    async fn execute_traced(&self, query: Query) -> Result<(), neo4rs::Error> {
        let span = tracing::info_span!(
            "neo4j.query",
            db.system = "neo4j",
            otel.kind = "client"
        );
        
        let _guard = span.enter();
        let mut stream = self.execute(query).await?;
        while let Ok(Some(_)) = stream.next().await {
            // Consume stream
        }
        Ok(())
    }
}
```

### Phase 4: Improved Utility Functions

```rust
// src/common.rs
use opentelemetry::KeyValue;
use std::time::{Duration, Instant};

pub struct QueryAttributes {
    pub operation: Option<String>,
    pub labels: Vec<String>,
    pub database: String,
}

pub fn extract_query_attributes(query_text: &str) -> QueryAttributes {
    // Parse Cypher query to extract operation type and node labels
    let operation = extract_operation(query_text);
    let labels = extract_node_labels(query_text);
    
    QueryAttributes {
        operation,
        labels,
        database: "neo4j".to_string(),
    }
}

fn extract_operation(query: &str) -> Option<String> {
    let query_upper = query.to_uppercase();
    for op in &["MATCH", "CREATE", "MERGE", "DELETE", "SET", "REMOVE"] {
        if query_upper.starts_with(op) {
            return Some(op.to_string());
        }
    }
    None
}

fn extract_node_labels(query: &str) -> Vec<String> {
    // Regex to extract node labels from Cypher
    // Example: (n:Person) -> ["Person"]
    let re = regex::Regex::new(r"\((?:\w+)?:(\w+)\)").unwrap();
    re.captures_iter(query)
        .map(|cap| cap[1].to_string())
        .collect()
}

pub struct SpanTimer {
    start: Instant,
}

impl SpanTimer {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }
    
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
    
    pub fn record_to_histogram(&self, histogram: &Histogram<f64>, attributes: &[KeyValue]) {
        histogram.record(self.elapsed().as_millis() as f64, attributes);
    }
}
```

## Recommended Implementation for otel-instrumentation-redis

### Metrics Support

```rust
// src/metrics.rs
pub struct RedisMetrics {
    commands_total: Counter<u64>,
    command_duration: Histogram<f64>,
    pipeline_size: Histogram<u64>,
    connection_errors: Counter<u64>,
    pool_size: UpDownCounter<i64>,
    cache_hits: Counter<u64>,
    cache_misses: Counter<u64>,
}

impl RedisMetrics {
    pub fn record_command(&self, cmd: &str, duration: Duration, success: bool) {
        let attributes = vec![
            KeyValue::new("command", cmd.to_string()),
            KeyValue::new("success", success),
        ];
        
        self.commands_total.add(1, &attributes);
        self.command_duration.record(duration.as_millis() as f64, &attributes);
    }
    
    pub fn record_cache_access(&self, hit: bool, key_pattern: Option<&str>) {
        let mut attributes = vec![KeyValue::new("hit", hit)];
        if let Some(pattern) = key_pattern {
            attributes.push(KeyValue::new("pattern", pattern.to_string()));
        }
        
        if hit {
            self.cache_hits.add(1, &attributes);
        } else {
            self.cache_misses.add(1, &attributes);
        }
    }
}
```

### Enhanced Client Wrapper

```rust
// src/client.rs
pub struct InstrumentedClient {
    inner: redis::Client,
    metrics: Option<Arc<RedisMetrics>>,
    trace_config: TraceConfig,
}

pub struct TraceConfig {
    pub record_db_statement: bool,
    pub record_key_names: bool,
    pub max_key_length: usize,
}

impl InstrumentedClient {
    pub fn builder(client: redis::Client) -> InstrumentedClientBuilder {
        InstrumentedClientBuilder::new(client)
    }
}

pub struct InstrumentedClientBuilder {
    client: redis::Client,
    metrics: Option<Arc<RedisMetrics>>,
    trace_config: TraceConfig,
}

impl InstrumentedClientBuilder {
    pub fn with_metrics(mut self, meter: Meter) -> Self {
        self.metrics = Some(Arc::new(RedisMetrics::new(meter)));
        self
    }
    
    pub fn with_statement_recording(mut self, enabled: bool) -> Self {
        self.trace_config.record_db_statement = enabled;
        self
    }
    
    pub fn build(self) -> InstrumentedClient {
        InstrumentedClient {
            inner: self.client,
            metrics: self.metrics,
            trace_config: self.trace_config,
        }
    }
}
```

## Recommended Implementation for otel-instrumentation-diesel

### Metrics Integration

```rust
// src/metrics.rs
pub struct DieselMetrics {
    queries_total: Counter<u64>,
    query_duration: Histogram<f64>,
    transactions_total: Counter<u64>,
    connection_pool_size: UpDownCounter<i64>,
    slow_queries: Counter<u64>,
}

impl DieselMetrics {
    pub fn record_query(&self, query_type: QueryType, duration: Duration, success: bool) {
        let attributes = vec![
            KeyValue::new("query_type", query_type.to_string()),
            KeyValue::new("success", success),
        ];
        
        self.queries_total.add(1, &attributes);
        self.query_duration.record(duration.as_millis() as f64, &attributes);
        
        // Track slow queries (> 1 second)
        if duration.as_secs() > 1 {
            self.slow_queries.add(1, &attributes);
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum QueryType {
    Select,
    Insert,
    Update,
    Delete,
    Other,
}

impl QueryType {
    pub fn from_sql(sql: &str) -> Self {
        let sql_upper = sql.trim().to_uppercase();
        if sql_upper.starts_with("SELECT") {
            Self::Select
        } else if sql_upper.starts_with("INSERT") {
            Self::Insert
        } else if sql_upper.starts_with("UPDATE") {
            Self::Update
        } else if sql_upper.starts_with("DELETE") {
            Self::Delete
        } else {
            Self::Other
        }
    }
}
```

### Connection Pool Instrumentation

```rust
// src/r2d2_instrumented.rs
use diesel::r2d2::{ConnectionManager, Pool};
use std::sync::Arc;

pub struct InstrumentedPool<C> {
    inner: Pool<ConnectionManager<C>>,
    metrics: Arc<DieselMetrics>,
}

impl<C> InstrumentedPool<C> {
    pub fn new(pool: Pool<ConnectionManager<C>>, metrics: Arc<DieselMetrics>) -> Self {
        Self { inner: pool, metrics }
    }
    
    pub fn get(&self) -> Result<InstrumentedPooledConnection<C>, r2d2::Error> {
        let conn = self.inner.get()?;
        self.metrics.connection_pool_size.add(1, &[]);
        Ok(InstrumentedPooledConnection {
            inner: conn,
            metrics: self.metrics.clone(),
        })
    }
}

pub struct InstrumentedPooledConnection<C> {
    inner: PooledConnection<ConnectionManager<C>>,
    metrics: Arc<DieselMetrics>,
}

impl<C> Drop for InstrumentedPooledConnection<C> {
    fn drop(&mut self) {
        self.metrics.connection_pool_size.add(-1, &[]);
    }
}
```

## Common Patterns Across All Crates

### 1. Semantic Convention Helpers

```rust
// src/semconv.rs
use opentelemetry::KeyValue;
use opentelemetry_semantic_conventions as semconv;

pub struct DbAttributes {
    attributes: Vec<KeyValue>,
}

impl DbAttributes {
    pub fn new(system: &str) -> Self {
        Self {
            attributes: vec![
                KeyValue::new(semconv::attribute::DB_SYSTEM_NAME, system.to_string()),
                KeyValue::new("otel.kind", "client"),
            ],
        }
    }
    
    pub fn with_operation(mut self, operation: &str) -> Self {
        self.attributes.push(KeyValue::new(
            semconv::attribute::DB_OPERATION_NAME,
            operation.to_string(),
        ));
        self
    }
    
    pub fn with_database(mut self, database: &str) -> Self {
        self.attributes.push(KeyValue::new(
            semconv::attribute::DB_NAME,
            database.to_string(),
        ));
        self
    }
    
    pub fn build(self) -> Vec<KeyValue> {
        self.attributes
    }
}
```

### 2. Error Classification

```rust
// src/error_classification.rs
pub enum ErrorCategory {
    Connection,
    Authentication,
    Timeout,
    Syntax,
    Constraint,
    Concurrency,
    Other,
}

impl ErrorCategory {
    pub fn classify_neo4j(error: &neo4rs::Error) -> Self {
        // Classify based on error type
        match error {
            neo4rs::Error::ConnectionError { .. } => Self::Connection,
            neo4rs::Error::AuthenticationError { .. } => Self::Authentication,
            _ => Self::Other,
        }
    }
    
    pub fn classify_redis(error: &redis::RedisError) -> Self {
        match error.kind() {
            redis::ErrorKind::IoError => Self::Connection,
            redis::ErrorKind::AuthenticationFailed => Self::Authentication,
            redis::ErrorKind::BusyLoadingError => Self::Concurrency,
            _ => Self::Other,
        }
    }
}
```

### 3. Configuration Management

```rust
// src/config.rs
#[derive(Clone, Debug)]
pub struct InstrumentationConfig {
    pub service_name: String,
    pub tracing: TracingConfig,
    pub metrics: MetricsConfig,
}

#[derive(Clone, Debug)]
pub struct TracingConfig {
    pub enabled: bool,
    pub record_db_statement: bool,
    pub max_db_statement_length: usize,
    pub sample_rate: f64,
}

#[derive(Clone, Debug)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub record_histograms: bool,
    pub bucket_boundaries: Vec<f64>,
}

impl Default for InstrumentationConfig {
    fn default() -> Self {
        Self {
            service_name: env::var("SERVICE_NAME").unwrap_or_else(|_| "unknown".to_string()),
            tracing: TracingConfig::default(),
            metrics: MetricsConfig::default(),
        }
    }
}
```

## Implementation Priority

1. **Phase 1**: Add metrics support to otel-instrumentation-neo4jrs
   - Implement basic counters and histograms
   - Add builder pattern for configuration
   - Test with Prometheus exporter

2. **Phase 2**: Implement extension trait pattern
   - Create GraphExt for Neo4j
   - Create ClientExt for Redis
   - Ensure backward compatibility

3. **Phase 3**: Enhance otel-instrumentation-redis
   - Add metrics support
   - Implement cache hit/miss tracking
   - Add pipeline instrumentation

4. **Phase 4**: Enhance otel-instrumentation-diesel
   - Add metrics for all backends
   - Implement connection pool metrics
   - Add slow query detection

5. **Phase 5**: Standardize across all crates
   - Unified configuration approach
   - Common error classification
   - Shared semantic convention helpers

## Testing Strategy

1. **Unit Tests**: Test metric recording logic
2. **Integration Tests**: Test with actual databases
3. **Performance Tests**: Measure instrumentation overhead
4. **Example Applications**: Create examples showing metrics + tracing

## Documentation Updates

1. Update README with metrics examples
2. Add configuration guide
3. Create migration guide from tracing-only to tracing+metrics
4. Add performance impact documentation

## Backward Compatibility

- Keep existing APIs unchanged
- Add new functionality via builder pattern and extension traits
- Use feature flags for metrics support
- Provide migration path for existing users
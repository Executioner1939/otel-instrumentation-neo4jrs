//! Error types for OpenTelemetry Neo4j instrumentation

use std::fmt;

/// Errors that can occur during Neo4j instrumentation setup or operation
#[derive(Debug)]
pub enum InstrumentationError {
    /// The underlying Neo4j driver returned an error
    Neo4jError(neo4rs::Error),
    /// Failed to retrieve connection information from the Neo4j server
    ConnectionInfoError {
        /// The underlying Neo4j error that caused the failure
        source: neo4rs::Error,
        /// Additional context about what operation failed
        context: String,
    },
    /// Failed to parse server configuration from environment variables
    ConfigurationError {
        /// The environment variable that failed to parse
        variable: String,
        /// The value that failed to parse
        value: String,
        /// The parsing error message
        error: String,
    },
}

impl fmt::Display for InstrumentationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstrumentationError::Neo4jError(err) => {
                write!(f, "Neo4j driver error: {err}")
            }
            InstrumentationError::ConnectionInfoError { source, context } => {
                write!(
                    f,
                    "Failed to retrieve connection information ({context}): {source}"
                )
            }
            InstrumentationError::ConfigurationError {
                variable,
                value,
                error,
            } => {
                write!(
                    f,
                    "Failed to parse environment variable {variable}='{value}': {error}"
                )
            }
        }
    }
}

impl std::error::Error for InstrumentationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            InstrumentationError::Neo4jError(err) => Some(err),
            InstrumentationError::ConnectionInfoError { source, .. } => Some(source),
            InstrumentationError::ConfigurationError { .. } => None,
        }
    }
}

impl From<neo4rs::Error> for InstrumentationError {
    fn from(err: neo4rs::Error) -> Self {
        InstrumentationError::Neo4jError(err)
    }
}

impl InstrumentationError {
    /// Create a new connection info error with context
    #[must_use]
    pub fn connection_info_error(source: neo4rs::Error, context: impl Into<String>) -> Self {
        Self::ConnectionInfoError {
            source,
            context: context.into(),
        }
    }

    /// Create a new configuration error
    #[must_use]
    pub fn configuration_error(
        variable: impl Into<String>,
        value: impl Into<String>,
        error: impl Into<String>,
    ) -> Self {
        Self::ConfigurationError {
            variable: variable.into(),
            value: value.into(),
            error: error.into(),
        }
    }
}

/// Result type alias for instrumentation operations
pub type InstrumentationResult<T> = Result<T, InstrumentationError>;

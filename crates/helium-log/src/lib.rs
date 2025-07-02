//! Logging utilities and abstractions for the helium blockchain framework.
//!
//! This crate provides a unified logging interface for helium applications,
//! supporting structured logging, log levels, and integration with tracing framework
//! as specified in PLAN.md.

pub use tracing::{debug, error, info, instrument, span, trace, warn, Level, Span};
pub use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialize the global tracing subscriber with structured output
///
/// This function sets up tracing-subscriber with proper formatting and filtering
/// to match the architecture specified in PLAN.md.
pub fn init_tracing() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(
            fmt::layer()
                .with_target(true)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_file(true)
                .with_line_number(true)
                .json(), // Use structured JSON output as specified in PLAN.md
        )
        .try_init()?;

    Ok(())
}

/// Initialize tracing with a specific level filter
pub fn init_tracing_with_level(
    level: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::registry()
        .with(EnvFilter::new(level))
        .with(
            fmt::layer()
                .with_target(true)
                .with_thread_ids(true)
                .with_thread_names(true)
                .with_file(true)
                .with_line_number(true)
                .json(),
        )
        .try_init()?;

    Ok(())
}

/// Initialize tracing for testing with simplified output
pub fn init_tracing_test() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::registry()
        .with(EnvFilter::new("debug"))
        .with(fmt::layer().with_test_writer())
        .try_init()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracing_macros() {
        // Test that tracing macros work
        info!("Test info message");
        debug!("Test debug message");
        warn!("Test warning message");
        error!("Test error message");
    }

    #[test]
    #[tracing::instrument]
    fn test_instrument_attribute() {
        info!("This function is instrumented");
    }
}

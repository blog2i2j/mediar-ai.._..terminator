mod e2e_tests;
mod firefox_window_tests;
mod functional_verification_tests;
mod google_workflow_tests;
mod performance_tests;
mod test_serialization;

// Initialize tracing for tests
pub fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt};
    fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive(tracing::Level::DEBUG.into()))
        .with_target(true)
        .with_thread_ids(false)
        .with_file(true)
        .with_line_number(true)
        .init();
}

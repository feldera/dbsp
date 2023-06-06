#[cfg(test)]
pub(crate) fn test_logger() {
    use is_terminal::IsTerminal;
    use tracing_subscriber::{filter::EnvFilter, fmt, prelude::*};

    let filter = EnvFilter::try_from_env("DATAFLOW_JIT_LOG")
        .or_else(|_| EnvFilter::try_new("info,cranelift_codegen=off,cranelift_jit=off"))
        .unwrap();
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .with_test_writer()
                .with_ansi(std::io::stdout().is_terminal()),
        )
        .try_init();
}

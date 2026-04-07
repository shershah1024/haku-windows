use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub fn init() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let log_dir = if cfg!(target_os = "windows") { std::env::temp_dir() } else { "/tmp".into() };
    let file_appender = tracing_appender::rolling::never(log_dir, "haku.log");
    let (file_writer, _guard) = tracing_appender::non_blocking(file_appender);

    // Leak the guard so it lives for the process lifetime
    std::mem::forget(_guard);

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(std::io::stderr))
        .with(fmt::layer().with_writer(file_writer).with_ansi(false))
        .init();
}

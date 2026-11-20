use std::str::FromStr;
use systemd_journal_logger::{JournalLog, connected_to_journal};

pub fn setup_logger() {
    if connected_to_journal() {
        JournalLog::new()
            .expect("could not open systemd journal")
            .with_extra_fields(vec![("VERSION", env!("CARGO_PKG_VERSION"))])
            .install()
            .expect("could not install systemd journal");
    } else {
        env_logger::init();
    }

    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|s| log::LevelFilter::from_str(&s).ok())
        .unwrap_or(log::LevelFilter::Info);
    log::set_max_level(level);

    log::info!("log level set to {}", level);
}

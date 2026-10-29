use systemd_journal_logger::{JournalLog, connected_to_journal};

pub fn setup_logger() {
    if connected_to_journal() {
        JournalLog::new()
            .expect("could not open systemd journal")
            .with_extra_fields(vec![("VERSION", env!("CARGO_PKG_VERSION"))])
            .install()
            .expect("could not install systemd journal");
        log::set_max_level(log::LevelFilter::Info);
    } else {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    }
}

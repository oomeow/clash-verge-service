mod log_config;
mod service;

use log_config::LogConfig;

#[cfg(windows)]
fn main() -> windows_service::Result<()> {
    let _ = LogConfig::global().lock().init(None);
    service::main()
}

#[cfg(not(windows))]
fn main() {
    let _ = LogConfig::global().lock().init(None);
    service::main();
}

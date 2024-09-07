mod log_config;
mod service;

#[cfg(windows)]
fn main() -> windows_service::Result<()> {
    service::main()
}

#[cfg(not(windows))]
fn main() {
    service::main();
}

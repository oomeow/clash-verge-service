mod log_config;

use anyhow::Error;

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn main() {
    log::error!("Unsupported platform");
    panic!("This program is not intended to run on this platform.");
}

#[cfg(target_os = "macos")]
fn main() -> Result<(), Error> {
    use log_config::{log_expect, parse_args, LogConfig};
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;

    let log_dir = parse_args();
    LogConfig::global().init(log_dir)?;

    log::debug!("Start install Clash Verge Service.");

    let service_binary_path = std::env::current_exe()
        .unwrap()
        .with_file_name("clash-verge-service");
    let target_binary_path = "/Library/PrivilegedHelperTools/io.github.clashverge.helper";
    log::debug!("Generate service file at {}", target_binary_path);
    let target_binary_dir = Path::new("/Library/PrivilegedHelperTools");
    if !service_binary_path.exists() {
        log::error!("The clash-verge-service binary not found.");
        std::process::exit(2);
    }
    if !target_binary_dir.exists() {
        log::debug!(
            "Create directory for service file [{}].",
            target_binary_dir.display()
        );
        log_expect(
            std::fs::create_dir(target_binary_dir),
            "Unable to create directory for service file",
        );
    }

    log::debug!(
        "Copy service file from {} to {}",
        service_binary_path.display(),
        target_binary_path
    );
    log_expect(
        std::fs::copy(service_binary_path, target_binary_path),
        "Unable to copy service file",
    );

    let plist_file = "/Library/LaunchDaemons/io.github.clashverge.helper.plist";
    log::debug!("Create plist file at {}", plist_file);
    let plist_file = Path::new(plist_file);
    let plist_file_content = include_str!("io.github.clashverge.helper.plist");
    let mut file = log_expect(
        File::create(plist_file),
        "Failed to create file for writing.",
    );
    log::debug!("Create plist file done.");

    log::debug!("Write plist file content.");
    log_expect(
        file.write_all(plist_file_content.as_bytes()),
        "Unable to write plist file",
    );
    log::debug!("Write plist file content done.");

    log::debug!("Chmod and chown plist file.");
    log_expect(
        std::process::Command::new("chmod")
            .arg("644")
            .arg(plist_file)
            .output(),
        "Failed to chmod",
    );
    log_expect(
        std::process::Command::new("chown")
            .arg("root:wheel")
            .arg(plist_file)
            .output(),
        "Failed to chown",
    );
    log::debug!("Chmod and chown plist file done.");

    log::debug!("Chmod and chown service file.");
    log_expect(
        std::process::Command::new("chmod")
            .arg("544")
            .arg(target_binary_path)
            .output(),
        "Failed to chmod",
    );
    log_expect(
        std::process::Command::new("chown")
            .arg("root:wheel")
            .arg(target_binary_path)
            .output(),
        "Failed to chown",
    );
    log::debug!("Chmod and chown service file done.");

    // Unload before load the service.
    log::debug!("Unload service before load the service.");
    log_expect(
        std::process::Command::new("launchctl")
            .arg("unload")
            .arg(plist_file)
            .output(),
        "Failed to unload service.",
    );
    // Load the service.
    log::debug!("Load service.");
    log_expect(
        std::process::Command::new("launchctl")
            .arg("load")
            .arg(plist_file)
            .output(),
        "Failed to load service.",
    );
    // Start the service.
    log::debug!("Start service.");
    log_expect(
        std::process::Command::new("launchctl")
            .arg("start")
            .arg("io.github.clashverge.helper")
            .output(),
        "Failed to load service.",
    );

    log::debug!("Service installed successfully.");
    Ok(())
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), Error> {
    const SERVICE_NAME: &str = "clash-verge-service";
    use core::panic;
    use log_config::{log_expect, parse_args, LogConfig};
    use std::path::Path;
    use std::{fs::File, io::Write};

    let log_dir = parse_args();
    LogConfig::global().init(log_dir)?;

    log::debug!("Start install Clash Verge Service.");
    let service_binary_path = std::env::current_exe()
        .unwrap()
        .with_file_name("clash-verge-service");
    if !service_binary_path.exists() {
        log::error!("The clash-verge-service binary not found.");
        std::process::exit(2);
    }

    // Peek the status of the service.
    log::debug!("Checking the status of the service.");
    let status_code = log_expect(
        std::process::Command::new("systemctl")
            .arg("status")
            .arg(format!("{}.service", SERVICE_NAME))
            .arg("--no-pager")
            .output(),
        "Failed to execute 'systemctl status' command",
    )
    .status
    .code();

    /*
     * https://www.freedesktop.org/software/systemd/man/latest/systemctl.html#Exit%20status
     */
    match status_code {
        Some(code) => match code {
            0 => {
                return {
                    log::debug!("The service is already installed and actived. (status code: 0)");
                    Ok(())
                }
            }
            ucode @ (1 | 2 | 3) => {
                log::debug!("The service is installed but it not active, start run service. (status code: {})", ucode);
                log_expect(
                    std::process::Command::new("systemctl")
                        .arg("start")
                        .arg(format!("{}.service", SERVICE_NAME))
                        .output(),
                    "Failed to execute 'systemctl start' command",
                );
                return Ok(());
            }
            4 => {
                log::debug!("The service status is unknown, continue to install. (status code: 4)")
            }
            _ => {
                log::error!("Unexpected status code from systemctl status");
                panic!("Unexpected status code from systemctl status")
            }
        },
        None => {
            log::error!("systemctl was improperly terminated.");
            panic!("systemctl was improperly terminated.");
        }
    }

    let unit_file = format!("/etc/systemd/system/{}.service", SERVICE_NAME);
    log::debug!("Generating service file [{}].", unit_file);
    let unit_file = Path::new(&unit_file);

    let unit_file_content = format!(
        include_str!("systemd_service_unit.tmpl"),
        service_binary_path.to_str().unwrap()
    );
    let mut file = log_expect(File::create(unit_file), "Failed to create file for writing");
    log_expect(
        file.write_all(unit_file_content.as_bytes()),
        "Failed to write to file",
    );
    log::debug!("Generated service file done.");

    // Reload unit files and start service.
    log::debug!("Reloading unit files and starting service.");
    log_expect(
        std::process::Command::new("systemctl")
            .arg("daemon-reload")
            .output()
            .and_then(|_| {
                std::process::Command::new("systemctl")
                    .arg("enable")
                    .arg(SERVICE_NAME)
                    .arg("--now")
                    .output()
            }),
        "Failed to reload unit files and start service",
    );

    log::debug!("Service installed successfully.");
    Ok(())
}

/// install and start the service
#[cfg(windows)]
fn main() -> Result<(), Error> {
    use log_config::{parse_args, LogConfig};
    use std::ffi::{OsStr, OsString};
    use windows_service::{
        service::{
            ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceState,
            ServiceType,
        },
        service_manager::{ServiceManager, ServiceManagerAccess},
    };

    let log_dir = parse_args();
    LogConfig::global().init(log_dir)?;

    log::debug!("Start installing Clash Verge Service.");

    log::debug!("Connecting to the service manager.");
    let manager_access = ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE;
    let service_manager = ServiceManager::local_computer(None::<&str>, manager_access)?;

    log::debug!("Checking if the service is installed and active.");
    let service_access = ServiceAccess::QUERY_STATUS | ServiceAccess::START;
    if let Ok(service) = service_manager.open_service("clash_verge_service", service_access) {
        log::debug!("The service is installed, checking if it is active.");
        if let Ok(status) = service.query_status() {
            match status.current_state {
                ServiceState::StopPending
                | ServiceState::Stopped
                | ServiceState::PausePending
                | ServiceState::Paused => {
                    log::debug!("Service is not active, starting it.");
                    service.start(&Vec::<&OsStr>::new())?;
                }
                _ => {}
            };

            return Ok(());
        }
    }
    log::debug!("The service is not installed, installing it.");

    let service_binary_path = std::env::current_exe()
        .unwrap()
        .with_file_name("clash-verge-service.exe");

    if !service_binary_path.exists() {
        log::error!("clash-verge-service.exe not found");
        std::process::exit(2);
    }

    let service_info = ServiceInfo {
        name: OsString::from("clash_verge_service"),
        display_name: OsString::from("Clash Verge Service"),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: service_binary_path,
        launch_arguments: vec![],
        dependencies: vec![],
        account_name: None, // run as System
        account_password: None,
    };

    log::debug!("Creating service: {:?}", service_info);
    let start_access = ServiceAccess::CHANGE_CONFIG | ServiceAccess::START;
    let service = service_manager.create_service(&service_info, start_access)?;

    log::debug!("Setting service description.");
    service.set_description("Clash Verge Service helps to launch clash core")?;
    log::debug!("Starting service.");
    service.start(&Vec::<&OsStr>::new())?;

    log::debug!("Service installed successfully.");
    Ok(())
}

use anyhow::Result;

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn process() -> Result<()> {
    log::error!("Unsupported platform");
    anyhow::bail!("This program is not intended to run on this platform.");
}

#[cfg(target_os = "macos")]
pub fn process() -> Result<()> {
    use crate::utils::log_expect;
    use std::{fs::remove_file, path::Path};

    log::debug!("Start uninstall Clash Verge Service");

    let plist_file = "/Library/LaunchDaemons/io.github.clashverge.helper.plist";

    // Unload the service.
    log::debug!("Unloading service");
    log_expect(
        std::process::Command::new("launchctl")
            .arg("unload")
            .arg(plist_file)
            .output(),
        "Failed to unload service.",
    );

    // Remove the service file.
    log::debug!(
        "Removing service file [/Library/PrivilegedHelperTools/io.github.clashverge.helper]"
    );
    let service_file = Path::new("/Library/PrivilegedHelperTools/io.github.clashverge.helper");
    if service_file.exists() {
        remove_file(service_file).expect("Failed to remove service file.");
    }

    // Remove the plist file.
    log::debug!("Removing plist file [{}]", plist_file);
    let plist_file = Path::new(plist_file);
    if plist_file.exists() {
        remove_file(plist_file).expect("Failed to remove plist file.");
    }

    log::debug!("Service uninstalled successfully.");
    Ok(())
}

#[cfg(target_os = "linux")]
pub fn process() -> Result<()> {
    use crate::service::SERVICE_NAME;
    use crate::utils::log_expect;
    use std::{fs::remove_file, path::Path};

    log::debug!("Start uninstall Clash Verge Service");

    // Disable the service
    log::debug!("Disabling [{SERVICE_NAME}] service");
    log_expect(
        std::process::Command::new("systemctl")
            .arg("disable")
            .arg(SERVICE_NAME)
            .arg("--now")
            .output(),
        "Failed to disable service.",
    );

    // Remove the unit file.
    let unit_file = format!("/etc/systemd/system/{SERVICE_NAME}.service");
    log::debug!("Removing unit service file [{unit_file}].");
    let unit_file = Path::new(&unit_file);
    if unit_file.exists() {
        log::debug!("Service file exists, removing it");
        log_expect(remove_file(unit_file), "Failed to remove unit file.");
    }
    log::debug!("Service file removed");

    log::debug!("Reloading systemd daemon");
    log_expect(
        std::process::Command::new("systemctl")
            .arg("daemon-reload")
            .output(),
        "Failed to reload systemd daemon.",
    );

    log::debug!("Service uninstalled successfully.");
    Ok(())
}

/// stop and uninstall the service
#[cfg(windows)]
pub fn process() -> Result<()> {
    use std::{thread, time::Duration};
    use windows_service::{
        service::{ServiceAccess, ServiceState},
        service_manager::{ServiceManager, ServiceManagerAccess},
    };

    log::debug!("Start uninstall Clash Verge Service.");

    log::debug!("Connecting to service manager.");
    let manager_access = ServiceManagerAccess::CONNECT;
    let service_manager = ServiceManager::local_computer(None::<&str>, manager_access)?;

    log::debug!("Opening existing service.");
    let service_access = ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE;
    let service = service_manager.open_service("clash_verge_service", service_access)?;

    log::debug!("Checking service status.");
    let service_status = service.query_status()?;
    if service_status.current_state != ServiceState::Stopped {
        log::debug!("Service status is not stopped, stopping it first.");
        if let Err(err) = service.stop() {
            log::error!("Failed to stop service: {err}");
        }
        // Wait for service to stop
        thread::sleep(Duration::from_secs(1));
    }

    log::debug!("Deleting service");
    service.delete()?;

    log::debug!("Service uninstalled successfully.");
    Ok(())
}

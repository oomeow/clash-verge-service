use std::path::PathBuf;

use clap::{Parser, Subcommand};
use clash_verge_service::log_config::LogConfig;
#[cfg(windows)]
use once_cell::sync::OnceCell;
#[cfg(windows)]
use windows_service::{define_windows_service, service_dispatcher};

#[derive(Parser)]
#[command(version, about = "install, uninstall or run Clash Verge Service", long_about = None)]
struct Cli {
    #[arg(short, long, help = "Run the IPC server with server-id as the socket path")]
    server_id: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Install Clash Verge Service")]
    Install {
        #[arg(short, long, help = "Log directory")]
        log_dir: Option<PathBuf>,

        #[arg(short, long, help = "The socket path of the IPC server")]
        server_id: Option<String>,
    },
    #[command(about = "Uninstall Clash Verge Service")]
    Uninstall {
        #[arg(short, long, help = "Log directory")]
        log_dir: Option<PathBuf>,
    },
}

/// used to store the server_id resolved by the clap
#[cfg(windows)]
static SERVER_ID: OnceCell<Option<String>> = OnceCell::new();

#[cfg(windows)]
define_windows_service!(ffi_service_main, my_service_main);

#[cfg(windows)]
pub fn my_service_main(_arguments: Vec<std::ffi::OsString>) {
    // this arguments is not same as launch arguments
    if let Ok(rt) = tokio::runtime::Runtime::new() {
        let server_id = SERVER_ID.get().expect("failed to get server id").clone();
        rt.block_on(async move {
            let psk = b"asdoekxfsdedadxjasd";
            let _ = clash_verge_service::service::run_service(server_id, Some(psk)).await;
        });
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Commands::Install { log_dir, server_id }) => {
            LogConfig::global().lock().init(log_dir)?;
            clash_verge_service::install::process(server_id)?;
        }
        Some(Commands::Uninstall { log_dir }) => {
            LogConfig::global().lock().init(log_dir)?;
            clash_verge_service::uninstall::process()?;
        }
        None => {
            LogConfig::global().lock().init(None)?;
            let server_id = cli.server_id;
            log::info!("Server ID: {:?}", server_id);
            #[cfg(not(windows))]
            {
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(async move {
                    let psk = b"asdoekxfsdedadxjasd";
                    let _ = clash_verge_service::service::run_service(server_id, Some(psk)).await;
                });
            }
            #[cfg(windows)]
            {
                SERVER_ID.set(server_id).expect("failed to set server id");
                service_dispatcher::start(clash_verge_service::service::SERVICE_NAME, ffi_service_main)?;
            }
        }
    }

    Ok(())
}

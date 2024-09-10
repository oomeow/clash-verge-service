use std::{env, fs, path::PathBuf, sync::Arc};

use anyhow::{bail, Result};
use log::LevelFilter;
use log4rs::{
    append::{console::ConsoleAppender, file::FileAppender},
    config::{Appender, Logger, Root},
    encode::pattern::PatternEncoder,
    Config, Handle,
};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;

#[derive(Debug, Clone)]
pub struct LogConfig {
    log_file_name: Arc<Mutex<String>>,
    log_dir: Arc<Mutex<Option<PathBuf>>>,
    limited_file_size: Arc<Mutex<Option<u64>>>,
    log_level: Arc<Mutex<Option<LevelFilter>>>,
    log_handle: Arc<Mutex<Option<Handle>>>,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            log_file_name: Arc::new(Mutex::new("clash-verge-service.log".to_string())),
            log_dir: Arc::new(Mutex::new(None)),
            limited_file_size: Arc::new(Mutex::new(Some(2 * 1024 * 1024))),
            log_level: Arc::new(Mutex::new(Some(LevelFilter::Debug))),
            log_handle: Arc::new(Mutex::new(None)),
        }
    }
}

impl LogConfig {
    pub fn global() -> &'static LogConfig {
        static LOGCONFIG: OnceCell<LogConfig> = OnceCell::new();

        LOGCONFIG.get_or_init(|| LogConfig::default())
    }

    pub fn init(&self, log_dir: Option<PathBuf>) -> Result<()> {
        let LogConfig {
            log_file_name,
            limited_file_size,
            log_level,
            ..
        } = Self::default();

        let log_file_name = log_file_name.lock().clone();
        let limited_file_size = limited_file_size.lock().clone().map(|v| v);
        let log_level = log_level.lock().clone().unwrap();

        let config =
            Self::create_log_config(&log_file_name, log_dir.clone(), limited_file_size, log_level.clone());

        if let Some(config) = config {
            let handle = log4rs::init_config(config).unwrap();

            *self.log_dir.lock() = log_dir;
            *self.log_handle.lock() = Some(handle);
        }
        Ok(())
    }

    #[allow(unused)]
    pub fn update_config(
        &self,
        log_file_name: &str,
        log_dir: PathBuf,
        limited_file_size: Option<u64>,
    ) -> Result<()> {
        let handle = Self::global().log_handle.lock().clone();
        if handle.is_none() {
            log::error!("update log config failed, log handle is none, please init first");
            bail!("update log config failed, log handle is none, please init first");
        }

        // check if need to update log config
        let mut need_update = false;
        let LogConfig {
            log_file_name: c_log_file_name,
            log_dir: c_log_dir,
            limited_file_size: c_limited_file_size,
            ..
        } = Self::global();
        if log_file_name != *c_log_file_name.lock() {
            need_update = true;
        }
        if !need_update
            && (c_log_dir.lock().is_none() || log_dir != *c_log_dir.lock().clone().unwrap())
        {
            need_update = true;
        }
        if !need_update && limited_file_size != *c_limited_file_size.lock() {
            need_update = true;
        }
        if !need_update {
            log::debug!("no need to update log config");
            return Ok(());
        }

        let log_level = Self::global().log_level.lock().clone().unwrap();
        let config = Self::create_log_config(
            &log_file_name,
            Some(log_dir.clone()),
            limited_file_size,
            log_level,
        );
        if let Some(config) = config {
            handle.unwrap().set_config(config);

            *self.log_file_name.lock() = log_file_name.to_string();
            *self.log_dir.lock() = Some(log_dir);
            *self.limited_file_size.lock() = limited_file_size;
        }
        Ok(())
    }

    fn create_log_config(
        log_file_name: &str,
        log_dir: Option<PathBuf>,
        limited_size: Option<u64>,
        log_level: LevelFilter,
    ) -> Option<Config> {
        let log_pattern = "{d(%Y-%m-%d %H:%M:%S)} {l} - {m}{n}";
        let encoder = Box::new(PatternEncoder::new(&log_pattern));

        let mut appenders = Vec::new();
        let log_to_file = log_dir.is_some();

        if log_to_file {
            // create log to file appender
            let log_file = log_dir.unwrap().join(log_file_name);
            if log_file.exists() && limited_size.is_some() {
                let metadata = fs::metadata(log_file.clone()).unwrap();
                if metadata.len() >= limited_size.unwrap() {
                    let _ = fs::rename(log_file.clone(), log_file.with_extension("old.log"));
                }
            }
            let tofile = FileAppender::builder()
                .encoder(encoder.clone())
                .build(log_file)
                .unwrap();
            let file_appender = Appender::builder().build("file", Box::new(tofile));
            appenders.push(file_appender);
        }

        // create log to stdout appender
        let stdout = ConsoleAppender::builder().encoder(encoder).build();
        let stdout_appender = Appender::builder().build("stdout", Box::new(stdout));
        appenders.push(stdout_appender);

        let appenders_str = if log_to_file {
            // vec!["file", "stdout"]
            vec!["file"]
        } else {
            vec!["stdout"]
        };

        let app_logger = Logger::builder()
            .appenders(appenders_str.clone())
            .additive(false)
            .build("app", log_level);
        let mihomo_logger = Logger::builder()
            .appenders(appenders_str.clone())
            .additive(false)
            .build("mihomo", log_level);
        let root = Root::builder().appenders(appenders_str).build(log_level);

        log4rs::config::Config::builder()
            .appenders(appenders)
            .logger(app_logger)
            .logger(mihomo_logger)
            .build(root)
            .map_or(None, |v| Some(v))
    }

    #[allow(unused)]
    pub fn update_log_level(&self, log_level: LevelFilter) -> Result<()> {
        let handle = self.log_handle.lock().clone();
        if handle.is_none() {
            bail!("update log level failed, log handle is none");
        }
        let config = Self::create_log_config(
            self.log_file_name.lock().clone().as_str(),
            self.log_dir.lock().clone().map(|v| v),
            self.limited_file_size.lock().clone(),
            log_level,
        );
        if let Some(config) = config {
            handle.unwrap().set_config(config);
            *self.log_level.lock() = Some(log_level);
        } else {
            bail!("Unable to create log config");
        }
        Ok(())
    }
}

#[allow(unused)]
pub fn parse_args() -> Option<PathBuf> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("--log-dir argument is required");
        return None;
    }
    if args.len() > 3 {
        eprintln!("too many arguments, only the --log-dir is allowed");
        return None;
    }
    let arg = &args[1];
    if arg != "--log-dir" {
        eprintln!("only the --log-dir argument is allowed");
        return None;
    }
    let val = &args[2];
    Some(PathBuf::from(val))
}

pub fn log_expect<T, E>(result: Result<T, E>, msg: &str) -> T
where
    E: std::fmt::Display,
{
    result.unwrap_or_else(|err| {
        log::error!("{}: {}", msg, err);
        panic!("{}", msg);
    })
}

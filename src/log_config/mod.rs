use std::{env, fs, path::PathBuf};

// use chrono::Local;
use log::LevelFilter;
use log4rs::{
    append::file::FileAppender,
    config::{Appender, Logger, Root},
    encode::pattern::PatternEncoder,
};

fn get_log_dir() -> PathBuf {
    let log_dir = std::env::var("CLASH_VERGE_SERVICE_LOG_DIR").unwrap_or_else(|e| {
        log::error!("Unable to get log dir: {}", e);
        panic!("Unable to get log dir");
    });
    log_dir.into()
}

fn get_log_file_path(log_file_name: &str) -> PathBuf {
    let log_dir = get_log_dir();
    // let local_time = Local::now().format("%Y-%m-%d-%H%M").to_string();
    let log_file = log_dir.join(log_file_name);
    log_file
}

pub fn init_log_config(log_file_name: &str, limited_size: Option<u64>) {
    let log_file = get_log_file_path(log_file_name);
    if log_file.exists() && limited_size.is_some() {
        let metadata = fs::metadata(log_file.clone()).unwrap();
        if metadata.len() > limited_size.unwrap() {
            let _ = fs::rename(log_file.clone(), log_file.with_extension("old.log"));
        }
    }
    let log_pattern = "{d(%Y-%m-%d %H:%M:%S)} {l} - {m}{n}";
    let encoder = Box::new(PatternEncoder::new(&log_pattern));
    let tofile = FileAppender::builder()
        .encoder(encoder)
        .build(log_file)
        .unwrap();
    let logger = Logger::builder()
        .appenders(["file"])
        .additive(false)
        .build("app", log::LevelFilter::Debug);
    let root_builder = Root::builder()
        .appenders(["file"])
        .build(LevelFilter::Debug);

    let config = log4rs::config::Config::builder()
        .appender(Appender::builder().build("file", Box::new(tofile)))
        .logger(logger)
        .build(root_builder)
        .unwrap();

    let _ = log4rs::init_config(config).unwrap();
}

#[allow(unused)]
pub fn parse_args() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        log::error!("accept only 1 argument: --log-dir, but got error arguments");
        panic!("accept only 1 argument: --log-dir, but got error arguments");
    } else {
        let arg = &args[1];
        if arg != "--log-dir" {
            log::error!("accept only 1 argument: --log-dir, but got {}", arg);
            panic!("accept only 1 argument: --log-dir, but got {}", arg);
        } else {
            let val = &args[2];
            std::env::set_var("CLASH_VERGE_SERVICE_LOG_DIR", val);
        }
    }
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

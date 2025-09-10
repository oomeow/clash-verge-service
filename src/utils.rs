use std::path::PathBuf;

#[allow(unused)]
/// 解析参数
///
/// ### 返回:
///
/// (logdir, serverid)
///     - log-dir: 记录日志的文件夹
///     - server-id: 服务用于 IPC 通信的 ID
pub fn parse_args() -> anyhow::Result<(Option<PathBuf>, Option<String>)> {
    let mut args: Vec<String> = std::env::args().collect();
    let args = args.split_first();
    if let Some((_, elements)) = args {
        let mut iter = elements.chunks(2);
        let mut log_dir = None;
        let mut server_id = None;
        for chunk in iter {
            if chunk.len() != 2 {
                anyhow::bail!("invalid argument format");
            }
            let arg = &chunk[0];
            let val = &chunk[1];
            if arg == "--log-dir" {
                log_dir = Some(PathBuf::from(val));
            } else if arg == "--server-id" {
                server_id = Some(val.to_string());
            } else {
                anyhow::bail!("only the --log-dir and --server-id arguments are allowed");
            }
        }
        Ok((log_dir, server_id))
    } else {
        anyhow::bail!("missing argument");
    }
}

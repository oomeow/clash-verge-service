# Clash Verge Service

Supports multiple platforms Service.

### 命令示例

```shell
# 安装服务, `--server-id` 是指定 IPC 服务的 ID
clash-verge--self-service install [--log-dir 记录日志的目录] --server-id server-test

# 卸载服务
clash-verge-self-service uninstall [--log-dir 记录日志的目录]

# 直接运行 IPC 服务
clash-verge-self-service --server-id server-test
```

### 测试 IPC 服务 API 接口

有关的测试用例都在 `main.rs` 里的测试模块中了, 可自行测试

### IPC 通信加密

服务端和客户端会在连接时通过 X25519 协商临时会话密钥，并使用 XChaCha20-Poly1305 加密每个请求/响应帧。默认不再使用写死密钥或 auth key 文件。

客户端和服务端启动时会从系统 keyring 读取同一个本地长期 IPC secret。服务端不会只凭这个 secret 放行连接：连接进来后先校验本机 IPC peer 身份，身份通过后，客户端才会在加密通道内执行 `ClaimClient`，用 keyring secret 换取短期 `session_token`。

服务端只保存长期 secret 和短期 `session_token` 的哈希值。运行期业务请求只依赖短期 `session_token`，租约有效期默认 15 秒，心跳间隔默认 5 秒；租约未过期前，其他客户端无法接管服务端。`Client` 内部会通过后台任务自动心跳续租，业务请求也会刷新租约，退出前可以调用 `Client::release()` 主动释放。

### examples

```shell
cargo run --example server
cargo run --example client
```

`examples/client.rs` 会在连接后自动 claim 服务端，空闲阶段由 `Client` 后台任务自动心跳续租，结束时调用 `Client::release()` 主动释放租约。

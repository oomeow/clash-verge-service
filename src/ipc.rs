use std::{
    io,
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub async fn connect(server_id: String) -> io::Result<Connection> {
    platform::connect(server_id).await
}

pub fn bind(server_id: String) -> io::Result<Listener> {
    platform::bind(server_id)
}

pub struct Listener {
    inner: platform::Listener,
    path: PathBuf,
}

impl Listener {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn accept(&mut self) -> io::Result<Connection> {
        self.inner.accept().await
    }
}

pub struct Connection {
    inner: platform::Connection,
}

impl Connection {
    #[cfg(unix)]
    pub fn unix_stream(&self) -> &tokio::net::UnixStream {
        &self.inner
    }

    #[cfg(windows)]
    pub fn verify_windows_client_token(&self) -> anyhow::Result<()> {
        self.inner.verify_client_token()
    }
}

impl AsyncRead for Connection {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let this = Pin::into_inner(self);
        Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for Connection {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        let this = Pin::into_inner(self);
        Pin::new(&mut this.inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = Pin::into_inner(self);
        Pin::new(&mut this.inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = Pin::into_inner(self);
        Pin::new(&mut this.inner).poll_shutdown(cx)
    }
}

fn ipc_path(server_id: String) -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(format!(r"\\.\pipe\{}", server_id.replace('/', "\\")))
    }

    #[cfg(not(windows))]
    {
        std::env::temp_dir().join(format!("{server_id}.sock"))
    }
}

#[cfg(unix)]
mod platform {
    use std::{
        fs, io,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
    };

    use tokio::net::{UnixListener, UnixStream};

    use super::{Connection as PublicConnection, Listener as PublicListener, ipc_path};

    pub type Connection = UnixStream;

    pub struct Listener {
        listener: UnixListener,
        path: PathBuf,
    }

    impl Listener {
        pub async fn accept(&mut self) -> io::Result<PublicConnection> {
            let (stream, _) = self.listener.accept().await?;
            Ok(PublicConnection { inner: stream })
        }
    }

    impl Drop for Listener {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }

    pub async fn connect(server_id: String) -> io::Result<PublicConnection> {
        Ok(PublicConnection {
            inner: UnixStream::connect(ipc_path(server_id)).await?,
        })
    }

    pub fn bind(server_id: String) -> io::Result<PublicListener> {
        let path = ipc_path(server_id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if Path::new(&path).exists() {
            fs::remove_file(&path)?;
        }

        let listener = UnixListener::bind(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o666))?;

        Ok(PublicListener {
            inner: Listener {
                listener,
                path: path.clone(),
            },
            path,
        })
    }
}

#[cfg(windows)]
mod platform {
    use std::{
        io,
        os::windows::io::AsRawHandle,
        path::PathBuf,
        pin::Pin,
        task::{Context, Poll},
        time::{Duration, Instant},
    };

    use anyhow::{Context as AnyhowContext, Result, anyhow};
    use tokio::{
        io::{AsyncRead, AsyncWrite, ReadBuf},
        net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_PIPE_BUSY, HANDLE},
        Security::{OpenThreadToken, RevertToSelf, TOKEN_QUERY},
        System::{Pipes::ImpersonateNamedPipeClient, Threading::GetCurrentThread},
    };

    use super::{Connection as PublicConnection, Listener as PublicListener, ipc_path};

    const PIPE_AVAILABILITY_TIMEOUT: Duration = Duration::from_secs(5);

    pub enum Connection {
        Server(NamedPipeServer),
        Client(NamedPipeClient),
    }

    impl Connection {
        pub fn verify_client_token(&self) -> Result<()> {
            let handle = match self {
                Self::Server(pipe) => pipe.as_raw_handle() as HANDLE,
                Self::Client(_) => return Err(anyhow!("cannot verify peer token from named-pipe client end")),
            };

            unsafe {
                if ImpersonateNamedPipeClient(handle) == 0 {
                    return Err(io::Error::last_os_error()).context("failed to impersonate named-pipe client");
                }

                let _revert = RevertGuard;
                let mut token: HANDLE = std::ptr::null_mut();
                if OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 0, &mut token) == 0 {
                    return Err(io::Error::last_os_error()).context("failed to open impersonated client token");
                }

                CloseHandle(token);
            }

            Ok(())
        }
    }

    struct RevertGuard;

    impl Drop for RevertGuard {
        fn drop(&mut self) {
            unsafe {
                RevertToSelf();
            }
        }
    }

    pub struct Listener {
        path: PathBuf,
        next: Option<NamedPipeServer>,
    }

    impl Listener {
        pub async fn accept(&mut self) -> io::Result<PublicConnection> {
            let listener = self
                .next
                .take()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "named-pipe listener is closed"))?;
            listener.connect().await?;
            self.next = Some(create_server(&self.path)?);
            Ok(PublicConnection {
                inner: Connection::Server(listener),
            })
        }
    }

    pub async fn connect(server_id: String) -> io::Result<PublicConnection> {
        let path = ipc_path(server_id);
        let attempt_start = Instant::now();
        let client = loop {
            match ClientOptions::new().read(true).write(true).open(&path) {
                Ok(client) => break client,
                Err(err) if err.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                    if attempt_start.elapsed() < PIPE_AVAILABILITY_TIMEOUT {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    }
                    return Err(err);
                }
                Err(err) => return Err(err),
            }
        };

        Ok(PublicConnection {
            inner: Connection::Client(client),
        })
    }

    pub fn bind(server_id: String) -> io::Result<PublicListener> {
        let path = ipc_path(server_id);
        Ok(PublicListener {
            inner: Listener {
                next: Some(create_server(&path)?),
                path: path.clone(),
            },
            path,
        })
    }

    fn create_server(path: &PathBuf) -> io::Result<NamedPipeServer> {
        ServerOptions::new()
            .reject_remote_clients(true)
            .access_inbound(true)
            .access_outbound(true)
            .in_buffer_size(65536)
            .out_buffer_size(65536)
            .create(path)
    }

    impl AsyncRead for Connection {
        fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
            match Pin::into_inner(self) {
                Self::Server(pipe) => Pin::new(pipe).poll_read(cx, buf),
                Self::Client(pipe) => Pin::new(pipe).poll_read(cx, buf),
            }
        }
    }

    impl AsyncWrite for Connection {
        fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
            match Pin::into_inner(self) {
                Self::Server(pipe) => Pin::new(pipe).poll_write(cx, buf),
                Self::Client(pipe) => Pin::new(pipe).poll_write(cx, buf),
            }
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            match Pin::into_inner(self) {
                Self::Server(pipe) => Pin::new(pipe).poll_flush(cx),
                Self::Client(pipe) => Pin::new(pipe).poll_flush(cx),
            }
        }

        fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            match Pin::into_inner(self) {
                Self::Server(pipe) => Pin::new(pipe).poll_shutdown(cx),
                Self::Client(pipe) => Pin::new(pipe).poll_shutdown(cx),
            }
        }
    }
}

use std::future::Future;

use once_cell::sync::Lazy;
use tokio::{
    runtime::{Builder, Handle, Runtime},
    task::JoinHandle,
};

static TOKIO_RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to create global tokio runtime")
});

fn runtime() -> &'static Runtime {
    &TOKIO_RUNTIME
}

pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    runtime().spawn(future)
}

pub fn block_on<F>(future: F) -> F::Output
where
    F: Future,
{
    match Handle::try_current() {
        Ok(handle) => tokio::task::block_in_place(|| handle.block_on(future)),
        Err(_) => runtime().block_on(future),
    }
}

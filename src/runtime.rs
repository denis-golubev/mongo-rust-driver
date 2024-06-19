mod acknowledged_message;
#[cfg(any(
    feature = "aws-auth",
    feature = "azure-kms",
    feature = "gcp-kms",
    feature = "azure-oidc",
    feature = "gcp-oidc"
))]
mod http;
mod join_handle;
#[cfg(any(feature = "in-use-encryption-unstable", test))]
pub(crate) mod process;
#[cfg(feature = "dns-resolver")]
mod resolver;
pub(crate) mod stream;
mod sync_read_ext;
pub(crate) mod time_utils;
#[cfg(feature = "openssl-tls")]
mod tls_openssl;
#[cfg(feature = "rustls-tls")]
#[cfg_attr(feature = "openssl-tls", allow(unused))]
mod tls_rustls;
mod tls_worker;
mod worker_handle;

#[cfg(not(target_arch = "wasm32"))]
use std::net::SocketAddr;

use std::{future::Future, time::Duration};
use worker::console_log;

#[cfg(feature = "dns-resolver")]
pub(crate) use self::resolver::AsyncResolver;
pub(crate) use self::{
    acknowledged_message::{AcknowledgedMessage, AcknowledgmentReceiver, AcknowledgmentSender},
    join_handle::AsyncJoinHandle,
    stream::AsyncStream,
    sync_read_ext::SyncLittleEndianRead,
    worker_handle::{WorkerHandle, WorkerHandleListener},
};

#[cfg(not(target_arch = "wasm32"))]
use crate::options::ServerAddress;

use crate::error::Result;
#[cfg(any(
    feature = "aws-auth",
    feature = "azure-kms",
    feature = "gcp-kms",
    feature = "azure-oidc",
    feature = "gcp-oidc"
))]
pub(crate) use http::HttpClient;
#[cfg(feature = "openssl-tls")]
use tls_openssl as tls;
#[cfg(all(feature = "rustls-tls", not(feature = "openssl-tls")))]
use tls_rustls as tls;
// #[cfg(not(any(feature = "rustls-tls", feature = "openssl-tls")))]
// compile_error!("At least one of the features 'rustls-tls' or 'openssl-tls' must be enabled.");

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use tls::TlsConfig;

#[cfg(target_arch = "wasm32")]
pub(crate) use tls_worker::TlsConfig;

/// Spawn a task in the background to run a future.
///
/// This must be called from an async block
/// or function running on a runtime.
pub(crate) fn spawn<F, O>(fut: F) -> AsyncJoinHandle<O>
where
    F: Future<Output = O> + Send + 'static,
    O: Send + 'static,
{
    AsyncJoinHandle::spawn(fut)
}

/// Await on a future for a maximum amount of time before returning an error.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn timeout<F: Future>(timeout: Duration, future: F) -> Result<F::Output> {
    tokio::time::timeout(timeout, future)
        .await
        .map_err(|_| std::io::ErrorKind::TimedOut.into())
}

// TODO: no timeout for now.
#[cfg(target_arch = "wasm32")]
pub(crate) async fn timeout<F: Future>(timeout: Duration, future: F) -> Result<F::Output> {
    console_log!("in runtime::timeout");

    let r = Ok(future.await);

    console_log!("after awaiting 'time-outed' future");

    r
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn resolve_address(
    address: &ServerAddress,
) -> Result<impl Iterator<Item = SocketAddr>> {
    let socket_addrs = tokio::net::lookup_host(format!("{}", address)).await?;
    Ok(socket_addrs)
}

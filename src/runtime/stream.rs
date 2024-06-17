#[cfg(not(target_arch = "wasm32"))]
use std::net::SocketAddr;

use std::{
    ops::DerefMut,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use tokio::io::AsyncWrite;
#[cfg(not(target_arch = "wasm32"))]
use tokio::net::TcpStream;
use worker::{ConnectionBuilder, SecureTransport, Socket};

#[cfg(not(target_arch = "wasm32"))]
use crate::runtime;

use crate::{
    error::{ErrorKind, Result},
    options::ServerAddress
};

#[cfg(not(target_arch = "wasm32"))]
use super::{
    tls::{tls_connect, TlsStream},
    TlsConfig,
};
#[cfg(target_arch = "wasm32")]
use super::tls_worker::TlsConfig;

pub(crate) const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const KEEPALIVE_TIME: Duration = Duration::from_secs(120);

/// An async stream possibly using TLS.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub(crate) enum AsyncStream {
    Null,

    /// A basic TCP connection to the server.
    #[cfg(not(target_arch = "wasm32"))]
    Tcp(TcpStream),

    /// A TLS connection over TCP.
    #[cfg(not(target_arch = "wasm32"))]
    Tls(TlsStream),

    #[cfg(target_arch = "wasm32")]
    Connection(SocketWithDebug),

    /// A Unix domain socket connection.
    #[cfg(unix)]
    Unix(tokio::net::UnixStream),
}

pub struct SocketWithDebug(Socket);

impl std::fmt::Debug for SocketWithDebug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Cloudflare Worker Socket")
    }
}

impl std::ops::Deref for SocketWithDebug {
    type Target = Socket;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsyncStream {
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn connect(
        address: ServerAddress,
        tls_cfg: Option<&TlsConfig>,
    ) -> Result<Self> {
        match &address {
            ServerAddress::Tcp { host, .. } => {
                let inner = tcp_connect(&address).await?;

                // If there are TLS options, wrap the inner stream in an AsyncTlsStream.
                match tls_cfg {
                    Some(cfg) => {
                        let with_debug = SocketWithDebug(tls_connect(host, inner, cfg).await?);
                        Ok(AsyncStream::Tls(with_debug))
                    }
                    None => Ok(AsyncStream::Tcp(inner)),
                }
            }
            #[cfg(unix)]
            ServerAddress::Unix { path } => Ok(AsyncStream::Unix(
                tokio::net::UnixStream::connect(path.as_path()).await?,
            )),
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) async fn connect(
        address: ServerAddress,
        tls_cfg: Option<&TlsConfig>,
    ) -> Result<Self> {
        match &address {
            ServerAddress::Tcp { .. } => {
                let tls_cfg = tls_cfg
                    .map(|cfg| cfg.clone())
                    .unwrap_or_default();

                let mut socket = tcp_connect(&address, &tls_cfg).await?;

                // It looks like the original implementation is directly
                // connecting via TLS if available.
                if let SecureTransport::StartTls = tls_cfg.secure_transport.0 {
                    socket = socket.start_tls();
                }

                Ok(AsyncStream::Connection(SocketWithDebug(socket)))
            },
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
async fn tcp_try_connect(address: &SocketAddr) -> Result<TcpStream> {
    let stream = TcpStream::connect(address).await?;
    stream.set_nodelay(true)?;

    let socket = socket2::Socket::from(stream.into_std()?);
    let conf = socket2::TcpKeepalive::new().with_time(KEEPALIVE_TIME);
    socket.set_tcp_keepalive(&conf)?;
    let std_stream = std::net::TcpStream::from(socket);
    Ok(TcpStream::from_std(std_stream)?)
}

#[cfg(not(target_arch = "wasm32"))]
async fn tcp_connect(address: &ServerAddress) -> Result<TcpStream> {
    let mut socket_addrs: Vec<_> = runtime::resolve_address(address).await?.collect();

    if socket_addrs.is_empty() {
        return Err(ErrorKind::DnsResolve {
            message: format!("No DNS results for domain {}", address),
        }
        .into());
    }

    // After considering various approaches, we decided to do what other drivers do, namely try
    // each of the addresses in sequence with a preference for IPv4.
    socket_addrs.sort_by_key(|addr| if addr.is_ipv4() { 0 } else { 1 });

    let mut connect_error = None;

    for address in &socket_addrs {
        connect_error = match tcp_try_connect(address).await {
            Ok(stream) => return Ok(stream),
            Err(err) => Some(err),
        };
    }

    Err(connect_error.unwrap_or_else(|| {
        ErrorKind::Internal {
            message: "connecting to all DNS results failed but no error reported".to_string(),
        }
        .into()
    }))
}

#[cfg(target_arch = "wasm32")]
async fn tcp_connect(address: &ServerAddress, tls_config: &TlsConfig) -> Result<Socket> {
    match address {
        ServerAddress::Tcp { host, port } => {
            // Let Cloudflare handle the DNS resolution => thus we can directly connect to the IP address
            // in contrast to the non-worker implementation.
            let builder =
                ConnectionBuilder::new().secure_transport(tls_config.secure_transport.clone().0);
            let port = port.unwrap_or(27017);
            let socket = builder.connect(host, port)
                .map_err(|e| ErrorKind::Internal {
                    message: format!("Failed to connect to {}:{} due to {}", host, port, e),
                })?;
            Ok(socket)
        },
    }
}

// TODO: Cloudflare Worker's Socket already implements AsyncRead and AsyncWrite...

impl tokio::io::AsyncRead for AsyncStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.deref_mut() {
            Self::Null => Poll::Ready(Ok(())),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Tcp(ref mut inner) => tokio::io::AsyncRead::poll_read(Pin::new(inner), cx, buf),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Tls(ref mut inner) => tokio::io::AsyncRead::poll_read(Pin::new(inner), cx, buf),
            #[cfg(target_arch = "wasm32")]
            Self::Connection(SocketWithDebug(ref mut inner)) =>
                tokio::io::AsyncRead::poll_read(Pin::new(inner), cx, buf),
            #[cfg(unix)]
            Self::Unix(ref mut inner) => tokio::io::AsyncRead::poll_read(Pin::new(inner), cx, buf),
        }
    }
}

impl AsyncWrite for AsyncStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.deref_mut() {
            Self::Null => Poll::Ready(Ok(0)),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Tcp(ref mut inner) => AsyncWrite::poll_write(Pin::new(inner), cx, buf),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Tls(ref mut inner) => Pin::new(inner).poll_write(cx, buf),
            #[cfg(target_arch = "wasm32")]
            Self::Connection(SocketWithDebug(ref mut inner)) =>
                tokio::io::AsyncWrite::poll_write(Pin::new(inner), cx, buf),
            #[cfg(unix)]
            Self::Unix(ref mut inner) => AsyncWrite::poll_write(Pin::new(inner), cx, buf),
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.deref_mut() {
            Self::Null => Poll::Ready(Ok(())),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Tcp(ref mut inner) => AsyncWrite::poll_flush(Pin::new(inner), cx),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Tls(ref mut inner) => Pin::new(inner).poll_flush(cx),
            #[cfg(target_arch = "wasm32")]
            Self::Connection(SocketWithDebug(ref mut inner)) =>
                tokio::io::AsyncWrite::poll_flush(Pin::new(inner), cx),
            #[cfg(unix)]
            Self::Unix(ref mut inner) => AsyncWrite::poll_flush(Pin::new(inner), cx),
        }
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.deref_mut() {
            Self::Null => Poll::Ready(Ok(())),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Tcp(ref mut inner) => Pin::new(inner).poll_shutdown(cx),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Tls(ref mut inner) => Pin::new(inner).poll_shutdown(cx),
            #[cfg(target_arch = "wasm32")]
            Self::Connection(SocketWithDebug(ref mut inner)) =>
                Pin::new(inner).poll_shutdown(cx),
            #[cfg(unix)]
            Self::Unix(ref mut inner) => Pin::new(inner).poll_shutdown(cx),
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[futures_io::IoSlice<'_>],
    ) -> Poll<std::result::Result<usize, std::io::Error>> {
        match self.get_mut() {
            Self::Null => Poll::Ready(Ok(0)),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Tcp(ref mut inner) => Pin::new(inner).poll_write_vectored(cx, bufs),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Tls(ref mut inner) => Pin::new(inner).poll_write_vectored(cx, bufs),
            #[cfg(target_arch = "wasm32")]
            Self::Connection(SocketWithDebug(ref mut inner)) =>
                Pin::new(inner).poll_write_vectored(cx, bufs),
            #[cfg(unix)]
            Self::Unix(ref mut inner) => Pin::new(inner).poll_write_vectored(cx, bufs),
        }
    }

    fn is_write_vectored(&self) -> bool {
        match self {
            Self::Null => false,
            #[cfg(not(target_arch = "wasm32"))]
            Self::Tcp(ref inner) => inner.is_write_vectored(),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Tls(ref inner) => inner.is_write_vectored(),
            #[cfg(target_arch = "wasm32")]
            Self::Connection(SocketWithDebug(ref inner)) =>
                inner.is_write_vectored(),
            #[cfg(unix)]
            Self::Unix(ref inner) => inner.is_write_vectored(),
        }
    }
}

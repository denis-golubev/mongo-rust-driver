use worker::SecureTransport;
use crate::options::TlsOptions;
use crate::error::Result;

#[derive(Clone, Default)]
pub(crate) struct TlsConfig {
    // SecureTransport does not implement Clone, using a separate type.
    pub secure_transport: CloneableSecureTransport,
}

impl TlsConfig {
    pub fn new(_tls_options: TlsOptions) -> Result<Self> {
        // This is only created when we actually have TLS => Thus just enable it.

        Ok(Self {
            secure_transport: CloneableSecureTransport(SecureTransport::On),
        })
    }
}

pub struct CloneableSecureTransport(pub SecureTransport);

impl Clone for CloneableSecureTransport {
    fn clone(&self) -> Self {
        CloneableSecureTransport(match self.0 {
            SecureTransport::Off => SecureTransport::Off,
            SecureTransport::On => SecureTransport::On,
            SecureTransport::StartTls => SecureTransport::StartTls,
        })
    }
}

impl Default for CloneableSecureTransport {
    fn default() -> Self {
        CloneableSecureTransport(SecureTransport::Off)
    }
}

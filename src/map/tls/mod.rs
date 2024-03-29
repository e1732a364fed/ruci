/*!
Defines Map s for tls server and client

uses rustls 0.22
 */
mod load;

pub mod client;
pub mod server;

#[cfg(test)]
mod test;

/// for benchmark
pub mod test2;

use async_trait::async_trait;
use bytes::BytesMut;
use rustls::pki_types::{Der, TrustAnchor};
use std::{fmt, sync::Arc};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tracing::debug;

use crate::{
    map,
    net::{self, helpers::EarlyDataWrapper},
    Name,
};
use std::path::PathBuf;

use super::{MapResult, ProxyBehavior};

pub fn default_rcs() -> rustls::RootCertStore {
    let mut root_certs = rustls::RootCertStore::empty();
    root_certs.extend(
        webpki_roots::TLS_SERVER_ROOTS
            .0
            .iter()
            .map(|ta| TrustAnchor {
                subject: ta.subject.into(),
                subject_public_key_info: ta.spki.into(),
                name_constraints: ta.name_constraints.map(Der::from),
            }),
    );
    root_certs
}

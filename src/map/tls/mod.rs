/*!
Defines [`map::Map`]s for tls server and client.

uses rustls 0.22
 */
pub mod load;

pub mod client;
pub mod mitm;
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

pub fn extract_host_from_client_hello(hello: &[u8]) -> Option<String> {
    // 检查是否是 Client Hello (handshake type 1)
    if hello.len() < 5 || hello[0] != 0x16 || hello[5] != 0x01 {
        return None;
    }

    let mut pos = 43; // Skip fixed headers

    // Skip session ID if present
    if pos < hello.len() {
        let session_id_len = hello[pos] as usize;
        pos += 1 + session_id_len;
    }

    // Skip cipher suites
    if pos + 2 < hello.len() {
        let cipher_len = ((hello[pos] as usize) << 8) | hello[pos + 1] as usize;
        pos += 2 + cipher_len;
    }

    // Skip compression methods
    if pos < hello.len() {
        let comp_len = hello[pos] as usize;
        pos += 1 + comp_len;
    }

    // Parse extensions
    if pos + 2 < hello.len() {
        let extensions_len = ((hello[pos] as usize) << 8) | hello[pos + 1] as usize;
        pos += 2;

        let extensions_end = pos + extensions_len;
        while pos + 4 < extensions_end {
            let ext_type = ((hello[pos] as u16) << 8) | hello[pos + 1] as u16;
            let ext_len = ((hello[pos + 2] as usize) << 8) | hello[pos + 3] as usize;
            pos += 4;

            // SNI extension type is 0
            if ext_type == 0 && pos + ext_len <= hello.len() {
                // Parse SNI
                pos += 2; // Skip server name list length
                let name_type = hello[pos];
                pos += 1;

                if name_type == 0 {
                    // host_name type
                    let name_len = ((hello[pos] as usize) << 8) | hello[pos + 1] as usize;
                    pos += 2;

                    if pos + name_len <= hello.len() {
                        return String::from_utf8(hello[pos..pos + name_len].to_vec()).ok();
                    }
                }
            }

            pos += ext_len;
        }
    }

    None
}

pub fn extract_alpn_from_clinet_hello(hello: &[u8]) -> Option<Vec<String>> {
    // 检查是否是 Client Hello (handshake type 1)
    if hello.len() < 5 || hello[0] != 0x16 || hello[5] != 0x01 {
        return None;
    }

    let mut pos = 43; // Skip fixed headers

    // Skip session ID
    if pos < hello.len() {
        let session_id_len = hello[pos] as usize;
        pos += 1 + session_id_len;
    }

    // Skip cipher suites
    if pos + 2 < hello.len() {
        let cipher_len = ((hello[pos] as usize) << 8) | hello[pos + 1] as usize;
        pos += 2 + cipher_len;
    }

    // Skip compression methods
    if pos < hello.len() {
        let comp_len = hello[pos] as usize;
        pos += 1 + comp_len;
    }

    // Parse extensions
    if pos + 2 < hello.len() {
        let extensions_len = ((hello[pos] as usize) << 8) | hello[pos + 1] as usize;
        pos += 2;

        let extensions_end = pos + extensions_len;
        while pos + 4 < extensions_end {
            let ext_type = ((hello[pos] as u16) << 8) | hello[pos + 1] as u16;
            let ext_len = ((hello[pos + 2] as usize) << 8) | hello[pos + 3] as usize;
            pos += 4;

            // ALPN extension type is 16
            if ext_type == 16 && pos + ext_len <= hello.len() {
                let mut protocols = Vec::new();

                // Read protocol list length
                let list_length = ((hello[pos] as usize) << 8) | hello[pos + 1] as usize;
                pos += 2;

                let list_end = pos + list_length;
                while pos < list_end {
                    let proto_len = hello[pos] as usize;
                    pos += 1;

                    if pos + proto_len <= list_end {
                        if let Ok(proto) = String::from_utf8(hello[pos..pos + proto_len].to_vec()) {
                            protocols.push(proto);
                        }
                        pos += proto_len;
                    }
                }

                return Some(protocols);
            }

            pos += ext_len;
        }
    }

    None
}

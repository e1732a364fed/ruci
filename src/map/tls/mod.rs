/*
使用 async_tls(其使用了 rustls)
 */
mod load;

pub mod client;
pub mod server;

#[cfg(test)]
mod test;

use async_trait::async_trait;
use bytes::BytesMut;
use log::debug;
use std::{fmt, io, sync::Arc};
use tokio::io::AsyncWriteExt;
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::{
    map,
    net::{self, helpers::EarlyDataWrapper},
    Name,
};
use std::path::PathBuf;

use super::{MapResult, ProxyBehavior};

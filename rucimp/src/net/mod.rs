#[cfg(all(feature = "sockopt", target_os = "linux"))]
pub mod so2;

#[cfg(all(feature = "sockopt", target_os = "linux"))]
pub mod so_opts;

use http::{HeaderValue, Request};
use ruci::net::http::CommonConfig;

use lazy_static::lazy_static;
lazy_static! {
    pub static ref EMPTY_HV: HeaderValue = HeaderValue::from_static("");
}

/// protocol: like ws://
pub fn build_request_from(c: &CommonConfig, protocol: &str) -> Request<()> {
    let mut request = Request::builder()
        .method(c.method.as_deref().unwrap_or("GET"))
        .header("Host", c.host.as_str())
        .uri(protocol.to_string() + c.host.as_str() + &c.path);

    if let Some(h) = &c.headers {
        for (k, v) in h.iter() {
            if k != "Host" {
                request = request.header(k.as_str(), v.as_str());
            }
        }
    }

    request.body(()).unwrap()
}

use thiserror::Error;

#[derive(Error, Debug)]
pub enum HttpMatchError<'a> {
    #[error("invalid host (expected {expected:?}, found {found:?})")]
    InvalidHost { expected: &'a str, found: &'a str },

    #[error("invalid path (expected {expected:?}, found {found:?})")]
    InvalidPath { expected: &'a str, found: &'a str },

    #[error("invalid header (expected {expected:?}, found {found:?})")]
    InvalidHeader { expected: &'a str, found: &'a str },

    #[error("invalid content-type (expected {expected:?}, found {found:?})")]
    InvalidContentType { expected: &'a str, found: &'a str },
}

pub fn match_request_http_header<'a, T: 'a>(
    c: &'a CommonConfig,
    r: &'a Request<T>,
) -> Result<(), HttpMatchError<'a>> {
    let a = r.uri().authority();
    let given_host = if let Some(a) = a { a.as_str() } else { "" };

    if c.host != given_host {
        return Err(HttpMatchError::InvalidHost {
            expected: &c.host,
            found: given_host,
        });
    }

    let given_path = r.uri().path();
    if c.path != given_path {
        return Err(HttpMatchError::InvalidPath {
            expected: &c.path,
            found: given_path,
        });
    }

    Ok(())
}

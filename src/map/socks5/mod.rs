/*!

Implements the following rfcs:

https://www.ietf.org/rfc/rfc1928.txt

USER/PASSWORD authentication rfc:

https://datatracker.ietf.org/doc/html/rfc1929

*/

pub mod client;
pub mod server;
#[cfg(test)]
mod test;

use crate::{
    map,
    net::{self},
    user::PlainText,
};
use anyhow::anyhow;
use bytes::{Buf, BufMut, BytesMut};

// socks5 version number.
pub const VERSION5: u8 = 0x05;

pub const AUTH_NONE: u8 = 0;
pub const AUTH_PASSWORD: u8 = 2;
pub const AUTH_NO_ACCEPTABLE: u8 = 0xff;

pub const CMD_CONNECT: u8 = 1;
pub const CMD_BIND: u8 = 2;
pub const CMD_UDPASSOCIATE: u8 = 3;

pub const ATYP_IP4: u8 = 1;
pub const ATYP_DOMAIN: u8 = 3;
pub const ATYP_IP6: u8 = 4;

pub const SUCCESS: u8 = 0;
pub const RSV: u8 = 0;
pub const USERPASS_SUBNEGOTIATION_VERSION: u8 = 1;

use lazy_static::lazy_static;

lazy_static! {
    pub static ref COMMMON_TCP_HANDSHAKE_REPLY: [u8; 10] = {
        [
            VERSION5, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ]
    };
}

//todo: 支持 fragment
pub fn decode_udp_diagram(buf: &mut BytesMut) -> anyhow::Result<net::Addr> {
    if buf.len() < 11 {
        return Err(anyhow!("udp diagram lenth wrong, {}", buf.len()));
    }
    let first2bytes = buf.get_u16();
    if first2bytes != 0 {
        return Err(anyhow!("udp diagram first2bytes wrong, {}", first2bytes));
    }
    let _frag = buf.get_u8();

    net::helpers::socks5_bytes_to_addr(buf)
}

//todo: 支持 fragment
pub fn encode_udp_diagram(ad: net::Addr, buf: &mut BytesMut) {
    buf.put_u16(0);
    buf.put_u8(0);

    net::helpers::addr_to_socks5_bytes(&ad, buf);
}

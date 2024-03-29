/*!
Implements <https://trojan-gfw.github.io/trojan/protocol> .
 */
use std::{fmt, mem};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha224};

use crate::user;

use super::{Data, DataFlags};

pub mod client;
pub mod server;
pub mod udp;

#[cfg(test)]
mod test;

pub const ATYP_IP4: u8 = 1;
pub const ATYP_DOMAIN: u8 = 3;
pub const ATYP_IP6: u8 = 4;

pub const CMD_CONNECT: u8 = 1;
pub const CMD_UDPASSOCIATE: u8 = 3;
pub const CMD_MUX: u8 = 0x7f; //trojan-gfw 那个文档里并没有提及Mux, 这个定义作者似乎没有在任何文档中提及, 而这个值是在trojan-go的源代码文件中找到的.

pub const CRLF: u16 = (0x0du16 << 8) + 0x0au16;
pub const CR: u8 = 0x0d;
pub const LF: u8 = 0x0a;

const PASS_LEN: usize = 56;

//https://stackoverflow.com/questions/27650312/show-u8-slice-in-hex-representation
pub struct HexSlice<'a>(&'a [u8]);

impl<'a> HexSlice<'a> {
    fn new<T>(data: &'a T) -> HexSlice<'a>
    where
        T: ?Sized + AsRef<[u8]> + 'a,
    {
        HexSlice(data.as_ref())
    }
}

// 实际上trojan协议文档写的不严谨, 它只说了用hex, 没说用大写还是小写. 它代码实现用的是小写.
impl fmt::Display for HexSlice<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

pub trait HexDisplayExt {
    fn hex_display(&self) -> HexSlice<'_>;
}

impl<T> HexDisplayExt for T
where
    T: ?Sized + AsRef<[u8]>,
{
    fn hex_display(&self) -> HexSlice<'_> {
        HexSlice::new(self)
    }
}

pub fn sha224_hex_string_lower_case(pass: &str) -> String {
    let mut hasher = Sha224::new();
    hasher.update(pass);
    let bs = &hasher.finalize()[..];
    format!("{}", bs.hex_display())
}

#[derive(Debug, PartialEq, Eq, Clone, Default, Serialize, Deserialize)]
pub struct User {
    pub plain_text_pass: String, //store the original password
    pub hex: String,             //len = 56

    astr: String,
}

impl User {
    pub fn new(plain_text: &str) -> Self {
        let hex = sha224_hex_string_lower_case(plain_text);
        User {
            plain_text_pass: plain_text.to_string(),
            hex: hex.clone(),
            astr: format!("trojan:{}", hex),
        }
    }
}

#[typetag::serde]
impl Data for User {
    fn get_user(&self) -> Option<Box<dyn user::User>> {
        let ub = Box::new(self.clone());
        Some(ub)
    }

    fn take_user(&mut self) -> Option<Box<dyn user::User>> {
        let ub = Box::new(mem::take(self));
        Some(ub)
    }
    fn get_flags(&self) -> DataFlags {
        DataFlags::User
    }
}

#[typetag::serde]
impl crate::user::UserTrait for User {
    fn identity_str(&self) -> String {
        self.hex.clone()
    }

    fn identity_bytes(&self) -> &[u8] {
        self.hex.as_bytes()
    }

    fn auth_str(&self) -> String {
        self.astr.clone()
    }

    fn auth_bytes(&self) -> &[u8] {
        self.hex.as_bytes()
    }
}

use std::mem;

use super::*;

use bitflags::bitflags;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub  struct DataFlags: u16 {
        const None = 0;
        const Unsigned = 0;
        const Integer = 0;
        const Float = 0b10000000;
        const Signed = 0b01000000;

        const Bool = 0b00000001;
        const Byte = 0b00000011;
        const Word = 0b00000111;
        const DWord = 0b00001111;
        const QWord = 0b00011111;

        const RAddr = 0b1000000000000000;
        const LAddr = 0b0100000000000000;
        const User = 0b0010000000000000;
        const CID = 0b0001000000000000;

        const RLAddr = Self::RAddr.bits() | Self::LAddr.bits();
    }
}

/// Mapper 的 maps 返回的 MapResult 中的静态数据类型
#[typetag::serde(tag = "type")]
pub trait Data: Debug + Send + Sync + DynClone {
    fn get_flags(&self) -> DataFlags {
        DataFlags::None
    }

    fn get_raddr(&self) -> Option<net::Addr> {
        None
    }
    fn take_raddr(&mut self) -> Option<net::Addr> {
        None
    }
    fn get_laddr(&self) -> Option<net::Addr> {
        None
    }
    fn get_user(&self) -> Option<Box<dyn User>> {
        None
    }
    fn take_user(&mut self) -> Option<Box<dyn User>> {
        None
    }

    fn get_extra_data(&self) -> Option<Vec<u8>> {
        None
    }

    fn get_u8(&self) -> Option<u8> {
        None
    }
}
dyn_clone::clone_trait_object!(Data);

#[typetag::serde]
impl Data for PlainText {
    fn get_user(&self) -> Option<Box<dyn User>> {
        let ub = Box::new(self.clone());
        Some(ub)
    }

    fn take_user(&mut self) -> Option<Box<dyn User>> {
        let ub = Box::new(mem::take(self));
        Some(ub)
    }

    fn get_flags(&self) -> DataFlags {
        DataFlags::User
    }
}
#[typetag::serde]
impl Data for u8 {
    fn get_u8(&self) -> Option<u8> {
        Some(*self)
    }
    fn get_flags(&self) -> DataFlags {
        DataFlags::Byte
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RAddr(pub net::Addr);

#[typetag::serde]
impl Data for RAddr {
    fn get_raddr(&self) -> Option<net::Addr> {
        Some(self.0.clone())
    }
    fn take_raddr(&mut self) -> Option<net::Addr> {
        Some(mem::take(&mut self.0))
    }
    fn get_flags(&self) -> DataFlags {
        DataFlags::RAddr
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RLAddr(pub net::Addr, pub net::Addr);

#[typetag::serde]
impl Data for RLAddr {
    fn get_raddr(&self) -> Option<net::Addr> {
        Some(self.0.clone())
    }
    fn get_laddr(&self) -> Option<net::Addr> {
        Some(self.1.clone())
    }
    fn get_flags(&self) -> DataFlags {
        DataFlags::RLAddr
    }
}

use super::*;

use bitflags::bitflags;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub  struct DataFlags: u32 {
        const None = 0b00000000;
        const Bool = 0b00000001;
        const RAddr = 0b00000010;
        const LAddr = 0b00000100;
        const User = 0b00001000;
        const CID = 0b00010000;
        const U8 = 0b00100000;

        const RLAddr = Self::RAddr.bits() | Self::LAddr.bits();
    }
}

/// Mapper 的 maps 返回的 MapResult 中的静态数据类型
#[typetag::serde]
pub trait Data: Debug + Send + Sync + DynClone {
    fn get_flags(&self) -> DataFlags {
        DataFlags::None
    }

    fn get_raddr(&self) -> Option<net::Addr> {
        None
    }
    fn get_laddr(&self) -> Option<net::Addr> {
        None
    }
    fn get_user(&self) -> Option<Box<dyn User>> {
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
        DataFlags::U8
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RLAddr(net::Addr, net::Addr);

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

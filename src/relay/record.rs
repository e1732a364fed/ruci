use super::*;

pub type OptNewInfoSender = Option<tokio::sync::mpsc::Sender<NewConnInfo>>;

/// (ub,db)
pub type OptUpdateInfoSender = Option<tokio::sync::mpsc::Sender<(u64, u64)>>;

#[derive(Clone, Debug)]
pub struct NewConnInfo {
    pub cid: CID,
    pub in_tag: String,
    pub out_tag: String,
    pub target_addr: net::Addr,

    #[cfg(feature = "trace")]
    pub in_trace: Vec<String>,

    #[cfg(feature = "trace")]
    pub out_trace: Vec<String>,
}

impl std::fmt::Display for NewConnInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}, {} -> {}, ta: {}\n",
            self.cid, self.in_tag, self.out_tag, self.target_addr
        )?;
        #[cfg(feature = "trace")]
        {
            write!(
                f,
                "in_trace: {:?}\n , out_trace: {:?}\n",
                self.in_trace, self.out_trace
            )
        }
    }
}

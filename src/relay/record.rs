use super::*;

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

#[async_trait]
pub trait NewInfoRecorder: Send + Sync {
    async fn record(&mut self, state: NewConnInfo);
}

use super::*;

pub type OptNewInfoSender = Option<tokio::sync::mpsc::Sender<NewConnInfo>>;

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
            "{} {} -> {} => {} , ",
            self.cid, self.in_tag, self.out_tag, self.target_addr
        )?;
        #[cfg(not(feature = "trace"))]
        return Ok(());

        #[cfg(feature = "trace")]
        {
            write!(
                f,
                "in_trace: {:?}, out_trace: {:?}",
                self.in_trace, self.out_trace
            )
        }
    }
}

#[test]
fn test() {
    let n = NewConnInfo {
        cid: CID::new_random(),
        in_tag: "intag1".to_string(),
        out_tag: "outt1".to_string(),
        target_addr: net::Addr::from_network_addr_str("127.1.2.3:389").unwrap(),

        #[cfg(feature = "trace")]
        in_trace: Vec::new(),

        #[cfg(feature = "trace")]
        out_trace: Vec::new(),
    };
    println!("{}", n)
}

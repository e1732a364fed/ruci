pub mod record;

use std::{fmt, io};

use bytes::BytesMut;

/// remove first character, and return the trimmed str
pub fn rm_first(value: &str) -> &str {
    let mut chars = value.chars();
    chars.next();
    chars.as_str()
}

/// bytes to Captalized hex string(like 1234FF)
pub struct HexSlice<'a>(pub &'a [u8]);

impl fmt::Display for HexSlice<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let len = self.0.len();
        write!(f, "{:06},", len)?;

        for byte in self.0 {
            write!(f, "{:02X}", byte)?;
        }
        writeln!(f)?;
        Ok(())
    }
}

/// generate an io::ErrorKind::Other
pub fn io_error<T: std::fmt::Display>(message: T) -> io::Error {
    io::Error::new(io::ErrorKind::Other, format!("{}", message))
}

pub fn buf_to_ob(b: BytesMut) -> Option<BytesMut> {
    if b.is_empty() {
        None
    } else {
        Some(b)
    }
}

/*
pub fn run_command(cmd: &str, args: &str) -> anyhow::Result<()> {
    trace!(cmd = cmd, args = ?args, "running command",);

    let r = Command::new(cmd).args(args.split(' ')).output()?;

    if r.status.success() {
        Ok(())
    } else {
        bail!("err output: {:?}", r);
    }
}
/// keep run next command if got error
pub fn sync_run_command_list_no_stop(list: Vec<&str>, no_warn: bool) -> anyhow::Result<()> {
    //debug!("utils: start run_command_list ");
    for cmd in list {
        let mut strs: Vec<_> = cmd.split(' ').collect();
        if strs.is_empty() {
            bail!("got empty command");
        }
        let args = strs.split_off(1);

        trace!(cmd = strs[0], args = ?args, "running command",);

        let r = Command::new(strs[0]).args(args).output();
        match r {
            Ok(o) => {
                if !o.status.success() {
                    if !no_warn {
                        warn!("run command not success, result is {:?}", o);
                    }
                    continue;
                }
            }
            Err(e) => {
                if !no_warn {
                    warn!("run command got err, result is {:?}", e);
                }
                continue;
            }
        }
    }
    //debug!("utils: finish run_command_list ");

    Ok(())
}

/// stop run if got error
pub fn sync_run_command_list_stop(list: Vec<&str>) -> anyhow::Result<()> {
    //debug!("utils: start run_command_list ");
    for cmd in list {
        let mut strs: Vec<_> = cmd.split(' ').collect();
        if strs.is_empty() {
            bail!("got empty command");
        }
        let args = strs.split_off(1);

        trace!(cmd = strs[0], args = ?args, "running command",);

        let r = Command::new(strs[0]).args(args).output();

        match r {
            Ok(o) => {
                if !o.status.success() {
                    bail!("run command not success, result is {:?}", o);
                }
            }
            Err(e) => {
                warn!("run command got err, result is {:?}", e);
                return Err(e.into());
            }
        }
    }
    //debug!("utils: finish run_command_list ");

    Ok(())
}
 */

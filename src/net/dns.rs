use std::{fs, vec};

pub fn get_sys_dns() -> Vec<String> {
    #[cfg(target_os = "linux")]
    {
        let r = fs::read_to_string("/etc/resolv.conf");
        match r {
            Ok(s) => {
                println!("{}", s);
            }
            Err(_) => {}
        }
    }

    return vec![];
}

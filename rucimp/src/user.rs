/*!
Provides some `user` related helper functions.
 */

use ruci::map::trojan;
use tracing::warn;
use user_trait::{PlainText, UserBox};

#[test]
fn test() {
    str_to_userbox("plaintext:u0\n p2");
}

/// Convert string with certain prefix to [`user_trait::UserBox`]
///
/// support plaintext:xxx, trojan:xxx
///
pub fn str_to_userbox(str: &str) -> Option<UserBox> {
    let s = String::from(str);
    let (protocol, desc_str) = match s.split_once(':') {
        Some(r) => r,
        None => return None,
    };

    let pass_type = String::from(protocol).to_lowercase();
    match pass_type.as_str() {
        "plaintext" => {
            let s = String::from(desc_str);
            let pair = s.split_once(char::is_whitespace).unwrap();

            let p = PlainText::new(pair.0.to_string(), pair.1.to_string());
            return Some(UserBox(Box::new(p)));
        }
        "trojan" => {
            let p = trojan::User::new(desc_str);
            return Some(UserBox(Box::new(p)));
        }

        _ => {
            warn!("user format invalid: {desc_str}, you can use like plaintext:u0 p0, or trojan:mypassword")
        }
    }
    None
}

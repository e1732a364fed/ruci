use ruci::{
    map::trojan,
    user::{PlainText, UserBox},
};
use tracing::warn;

#[test]
fn test() {
    str_to_userbox("plaintext:u0\n p2");
}

/// convert string with certain prefix to [`ruci::user::UserBox`]
///
/// support plaintext:xxx, trojan:xxx
///
pub fn str_to_userbox(str: &str) -> Option<UserBox> {
    let s = String::from(str);
    let v: Vec<&str> = s.splitn(2, ':').collect();
    if v.len() != 2 {
        return None;
    }
    let pass_type = String::from(v[0]).to_lowercase();
    match pass_type.as_str() {
        "plaintext" => {
            let s = String::from(v[1]);
            let pair = s.split_once(char::is_whitespace).unwrap();

            let p = PlainText::new(pair.0.to_string(), pair.1.to_string());
            return Some(UserBox(Box::new(p)));
        }
        "trojan" => {
            let p = trojan::User::new(v[1]);
            return Some(UserBox(Box::new(p)));
        }
        _ => {
            warn!("user format invalid: {str}, you can use like plaintext:u0 p0, or trojan:mypassword")
        }
    }
    None
}

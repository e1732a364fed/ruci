use std::collections::HashMap;

use async_trait::async_trait;
use std::sync::Mutex;

/// 用于用户鉴权
pub trait UserTrait {
    fn identity_str(&self) -> String; //每个user唯一，通过比较这个string 即可 判断两个User 是否相等。相当于 user name

    fn identity_bytes(&self) -> &[u8]; //与str类似; 对于程序来说,bytes更方便处理; 可以与str相同，也可以不同.

    fn auth_str(&self) -> String; //AuthStr 可以识别出该用户 并验证该User的真实性。相当于 user name + password

    fn auth_bytes(&self) -> &[u8]; //与 AuthStr 类似; 对于程序来说,bytes更方便处理; 可以与str相同，也可以不同.
}

pub trait User: UserTrait + Clone {}
impl<T: UserTrait + Clone> User for T {}

/// 用户鉴权的实际方法
#[async_trait]
pub trait AsyncUserAuthenticator<T: User> {
    async fn auth_user_by_authstr(&self, authstr: &str) -> Option<T>;
}

/// 简单以字符串存储用户名和密码，实现User
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct UserPass {
    pub user: String,
    pub pass: String,

    astr: String,
}

impl UserPass {
    pub fn new(user: String, pass: String) -> Self {
        let astr = format!("{}\n{}", user, pass);
        UserPass { user, pass, astr }
    }

    ///按whitespace 分割userpass后进行new
    pub fn from(userpass: String) -> Self {
        let ss: Vec<&str> = userpass.splitn(2, char::is_whitespace).collect();
        if ss.len() < 2 {
            UserPass::new(userpass, "".into())
        } else {
            UserPass::new(ss[0].into(), ss[1].into())
        }
    }

    ///只要user不为空就视为有效
    pub fn valid(&self) -> bool {
        !self.user.is_empty()
    }

    /// user不为空 且 pass 不为空
    pub fn strict_valid(&self) -> bool {
        !self.user.is_empty() && !self.pass.is_empty()
    }

    ///auth string slice
    pub fn auth_strs(&self) -> &str {
        self.astr.as_str()
    }
}

impl UserTrait for UserPass {
    fn identity_str(&self) -> String {
        self.user.clone()
    }

    fn identity_bytes(&self) -> &[u8] {
        self.user.as_bytes()
    }

    fn auth_str(&self) -> String {
        self.astr.clone()
    }

    fn auth_bytes(&self) -> &[u8] {
        self.astr.as_bytes()
    }
}

/// 存储User, 实现 AsyncUserAuthenticator
#[derive(Debug)]
pub struct UsersMap<T: User> {
    m: Mutex<InnerUsersmapStruct<T>>,
}

impl<T: User> Clone for UsersMap<T> {
    fn clone(&self) -> Self {
        Self {
            m: Mutex::new(self.m.lock().unwrap().clone()),
        }
    }
}

#[derive(Debug, Clone)]
struct InnerUsersmapStruct<T: User> {
    idmap: HashMap<String, T>, // id map
    amap: HashMap<String, T>,  //auth map
}

impl<T: User> InnerUsersmapStruct<T> {
    fn new() -> Self {
        InnerUsersmapStruct {
            idmap: HashMap::new(),
            amap: HashMap::new(),
        }
    }
}

impl<T: User> Default for UsersMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: User> UsersMap<T> {
    pub fn new() -> Self {
        UsersMap {
            m: Mutex::new(InnerUsersmapStruct::new()),
        }
    }

    pub async fn add_user(&mut self, u: T) {
        let uc = u.clone();
        let mut inner = self.m.lock().unwrap();
        inner.idmap.insert(u.identity_str(), u);
        inner.amap.insert(uc.auth_str(), uc);
    }

    pub async fn len(&self) -> usize {
        self.m.lock().unwrap().idmap.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.m.lock().unwrap().idmap.is_empty()
    }
}

#[async_trait]
impl<T: User + Send> AsyncUserAuthenticator<T> for UsersMap<T> {
    async fn auth_user_by_authstr(&self, authstr: &str) -> Option<T> {
        let inner = self.m.lock().unwrap();
        let s = authstr.to_string();
        inner.amap.get(&s).cloned()
    }
}

#[cfg(test)]
mod test {
    use futures::executor::block_on;
    use std::{collections::HashMap, io};

    use super::UserPass;
    use crate::user::{AsyncUserAuthenticator, UsersMap};

    #[test]
    fn test_hashmap() {
        let mut map = HashMap::new();
        map.insert(1, "a");
        assert_eq!(map.get(&1), Some(&"a"));
        assert_eq!(map.get(&2), None);

        let mut map = HashMap::new();
        let k = 1.to_string();
        map.insert(k.as_str(), "a");
        assert_eq!(map.get("1"), Some(&"a"));
    }

    #[tokio::test]
    async fn test_users_map() -> std::io::Result<()> {
        let up = UserPass::new("u".into(), "p".into());
        //println!("up: {:?}", up);
        let up2 = UserPass::new("u2".into(), "p2".into());

        let mut um: UsersMap<UserPass> = UsersMap::new();
        block_on(um.add_user(up));
        block_on(um.add_user(up2));
        //println!("um: {:?}", um);

        let x = um.auth_user_by_authstr("u").await;
        //println!("x {:?}", x);

        if x != None {
            return Err(io::Error::other("shit,not none"));
        }

        let x = um.auth_user_by_authstr("u2\np2").await;
        //println!("x {:?}", x);

        if x == None {
            Err(io::Error::other("shit,none"))
        } else {
            Ok(())
        }
    }
}

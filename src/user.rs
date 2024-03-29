/*!
Defines basic traits and helper structs for user authentication.
 */

use std::collections::{BTreeSet, HashMap};
use std::fmt::Debug;

use dyn_clone::DynClone;
use serde::{Deserialize, Serialize};
use std::hash::Hash;

/// 用于用户鉴权
#[typetag::serde]
pub trait UserTrait: Debug + Send + Sync {
    /// 每个user唯一, 通过比较这个string 即可 判断两个User 是否相等. 相当于 user name. 用于在非敏感环境显示该用户
    fn identity_str(&self) -> String;

    fn identity_bytes(&self) -> &[u8]; //与str类似; 对于程序来说,bytes更方便处理; 可以与str相同, 也可以不同.

    /// auth_str 可以识别出该用户 并验证该User的真实性. 相当于 user name + password.
    /// 约定, 每一种不同的 UserTrait 实现都要在 auth_str 前部加上 {type}: 这种形式, 如"plaintext:u0 p0" ,
    /// 以用于对不同的 实现 得到的 auth_str 加以区分. 也即 auth_str 须可用于 UserBox 的 Hash
    fn auth_str(&self) -> String;

    fn auth_bytes(&self) -> &[u8]; //与 auth_str 类似; 对于程序来说,bytes更方便处理; 可以与 auth_str 相同, 也可以不同.
}

/// a cloneable [`UserTrait`]
///
/// Dev Notes:
/// 如果User的super trait 是 Clone, 则 [`Box<dyn User>`] 会报错, says
/// can't make into object; 但是用 DynClone 就可以
pub trait User: UserTrait + DynClone {}

impl<T: UserTrait + DynClone> User for T {}
dyn_clone::clone_trait_object!(User);

/// implements Hash
#[derive(Clone)]
pub struct UserBox(pub Box<dyn User>);

impl Debug for UserBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("UserBox").field(&self.0.auth_str()).finish()
    }
}

impl Hash for UserBox {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.auth_str().hash(state);
    }
}

impl PartialOrd for UserBox {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for UserBox {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.auth_str().cmp(&other.0.auth_str())
    }
}

impl PartialEq for UserBox {
    fn eq(&self, other: &Self) -> bool {
        self.0.auth_str() == other.0.auth_str()
    }
}

impl Eq for UserBox {}

/// 实现了 Hash
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UserVec(pub Vec<UserBox>);

impl UserVec {
    /// sort_hash always sort vec before hash, meaning that
    /// the sort_hash for `UserVec([a,b,c])` and   `UserVec([b,a,c])` is the same
    pub fn sort_hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let bt: BTreeSet<&UserBox> = self.0.iter().collect();
        bt.iter().for_each(|b| {
            b.hash(state);
        })
    }
}

impl Hash for UserVec {
    /// hash 为所有 user 的 hash 相加
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.iter().for_each(|b| {
            b.hash(state);
        })
    }
}

/// 用户鉴权的实际方法
//#[async_trait]
pub trait AsyncUserAuthenticator<T: User> {
    fn auth_user_by_authstr(&self, authstr: &str) -> Option<T>;
}

/// 简单以字符串存储用户名和密码, 实现User
#[derive(Debug, Default, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct PlainText {
    pub user: String,
    pub pass: String,

    astr: String,
}

impl PlainText {
    pub fn new(user: String, pass: String) -> Self {
        // 采用 \n, 以支持 用户名或密码中有空格的情况
        let astr = format!("plaintext:{}\n{}", user, pass);
        PlainText { user, pass, astr }
    }

    ///按whitespace 分割userpass后进行new
    pub fn from(userpass: String) -> Self {
        let ss: Vec<&str> = userpass.splitn(2, char::is_whitespace).collect();
        if ss.len() < 2 {
            PlainText::new(userpass, "".into())
        } else {
            PlainText::new(ss[0].into(), ss[1].into())
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

    ///plaintext:{user}\n{pass}, like plaintext:u1\np1
    pub fn auth_str(&self) -> &str {
        self.astr.as_str()
    }
}

#[typetag::serde]
impl UserTrait for PlainText {
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

/// store User, impl AsyncUserAuthenticator
#[derive(Debug, Clone)]
pub struct UsersMap<T: UserTrait + Clone> {
    m: InnerUsersMapStruct<T>,
}

// impl<T: UserTrait + Clone> Clone for UsersMap<T> {
//     fn clone(&self) -> Self {
//         Self {
//             m: Mutex::new(self.m.clone()),
//         }
//     }
// }

#[derive(Debug, Clone)]
struct InnerUsersMapStruct<T: UserTrait + Clone> {
    id_map: HashMap<String, T>, // id map
    a_map: HashMap<String, T>,  //auth map
}

impl<T: UserTrait + Clone> InnerUsersMapStruct<T> {
    fn new() -> Self {
        InnerUsersMapStruct {
            id_map: HashMap::new(),
            a_map: HashMap::new(),
        }
    }
}

impl<T: UserTrait + Clone> Default for UsersMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: UserTrait + Clone> UsersMap<T> {
    pub fn new() -> Self {
        UsersMap {
            m: InnerUsersMapStruct::new(),
            //m: Mutex::new(InnerUsersMapStruct::new()),
        }
    }

    pub fn add_user(&mut self, u: T) {
        let uc = u.clone();
        let inner = &mut self.m;
        inner.id_map.insert(u.identity_str(), u);
        inner.a_map.insert(uc.auth_str(), uc);
    }

    pub fn len(&self) -> usize {
        self.m.id_map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.m.id_map.is_empty()
    }
}

//#[async_trait]
impl<T: UserTrait + Clone> AsyncUserAuthenticator<T> for UsersMap<T> {
    fn auth_user_by_authstr(&self, authstr: &str) -> Option<T> {
        let inner = &self.m;
        let s = authstr.to_string();
        inner.a_map.get(&s).cloned()
    }
}

#[cfg(test)]
mod test {
    use anyhow::*;
    use std::collections::HashMap;

    use super::PlainText;
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
    async fn test_users_map() -> Result<()> {
        let up = PlainText::new("u".into(), "p".into());
        let up2 = PlainText::new("u2".into(), "p2".into());

        let mut um: UsersMap<PlainText> = UsersMap::new();
        um.add_user(up);
        um.add_user(up2);

        let x = um.auth_user_by_authstr("plaintext:u");

        if x != None {
            return Err(anyhow!("shit,not none"));
        }

        let x = um.auth_user_by_authstr("plaintext:u2\np2");

        if x == None {
            Err(anyhow!("shit,none"))
        } else {
            Ok(())
        }
    }
}

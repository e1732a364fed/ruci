/*
mod user defines basic traits and helper structs for user authentication.
 */

use std::collections::{BTreeSet, HashMap};
use std::fmt::Debug;

use async_trait::async_trait;
use dyn_clone::DynClone;
use parking_lot::Mutex;
use std::hash::Hash;

//use crate::{AnyBox, AnyS};

/// 用于用户鉴权
pub trait UserTrait: Debug + Send + Sync {
    /// 每个user唯一, 通过比较这个string 即可 判断两个User 是否相等。相当于 user name. 用于在非敏感环境显示该用户
    fn identity_str(&self) -> String;

    fn identity_bytes(&self) -> &[u8]; //与str类似; 对于程序来说,bytes更方便处理; 可以与str相同, 也可以不同.

    /// auth_str 可以识别出该用户 并验证该User的真实性。相当于 user name + password.
    /// 约定，每一种不同的 UserTrait 实现都要在 auth_str 前部加上 {type}: 这种形式, 如"plaintext:u0 p0" ,
    /// 以用于对不同的 实现 得到的 auth_str 加以区分. 也即 auth_str 须可用于 UserBox 的 Hash
    fn auth_str(&self) -> String;

    fn auth_bytes(&self) -> &[u8]; //与 auth_str 类似; 对于程序来说,bytes更方便处理; 可以与 auth_str 相同, 也可以不同.
}

/// 如果User的supertrait 是 Clone, 则 Box<dyn User> 会报错, says
/// can't make into object; 但是用 DynClone 就可以
pub trait User: UserTrait + DynClone {}
impl<T: UserTrait + DynClone> User for T {}
dyn_clone::clone_trait_object!(User);

/*
/// from &AnyS get `Box<dyn User>`
///
/// # Example
///
/// ```
/// use ruci::*;
/// use ruci::user::*;
/// use parking_lot::Mutex;
/// use std::sync::Arc;
///
/// let u = PlainText::new("u".to_string(), "".to_string());
/// let ub0: Box<dyn User> = Box::new(u);
/// let ub2: AnyArc = Arc::new(Mutex::new(ub0));
/// let anyv = ub2.lock();
/// let y = get_user_from_anydata(&*anyv);
/// assert!(y.is_some());
/// ```
///
pub(crate) fn get_user_from_anydata(anys: &AnyS) -> Option<Box<dyn User>> {
    let a = anys.downcast_ref::<Box<dyn User>>();
    a.map(|u| u.clone())
}

/// from &AnyBox get `Box<dyn User>`
///
/// # Example
///
/// ```
/// use ruci::*;
/// use ruci::user::*;
/// ///
/// let u = PlainText::new("u".to_string(), "".to_string());
/// let ub0: Box<dyn User> = Box::new(u);
/// let ub2:AnyBox = Box::new(ub0);
/// let y = bget_user_from_anydata(&ub2);
/// assert!(y.is_some());
/// ```
///
pub(crate) fn bget_user_from_anydata(anys: &AnyBox) -> Option<Box<dyn User>> {
    let a = anys.downcast_ref::<Box<dyn User>>();
    a.map(|u| u.clone())
}
*/

/// implements Hash
#[derive(Clone)]
pub struct UserBox(pub Box<dyn User>);

impl Debug for UserBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("UserBox")
            .field(&self.0.identity_str())
            .finish()
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserVec(pub Vec<UserBox>);

impl UserVec {
    pub fn new() -> Self {
        UserVec(Vec::new())
    }

    /// sort_hash always sort vec before hash, meaning that
    /// the sort_hash for UserVec([a,b,c]) and   UserVec([b,a,c]) is the same
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
#[async_trait]
pub trait AsyncUserAuthenticator<T: User> {
    async fn auth_user_by_authstr(&self, authstr: &str) -> Option<T>;
}

/// 简单以字符串存储用户名和密码, 实现User
#[derive(Debug, PartialEq, Eq, Clone)]
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
    pub fn auth_strs(&self) -> &str {
        self.astr.as_str()
    }
}

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
#[derive(Debug)]
pub struct UsersMap<T: UserTrait + Clone> {
    m: Mutex<InnerUsersmapStruct<T>>,
}

impl<T: UserTrait + Clone> Clone for UsersMap<T> {
    fn clone(&self) -> Self {
        Self {
            m: Mutex::new(self.m.lock().clone()),
        }
    }
}

#[derive(Debug, Clone)]
struct InnerUsersmapStruct<T: UserTrait + Clone> {
    idmap: HashMap<String, T>, // id map
    amap: HashMap<String, T>,  //auth map
}

impl<T: UserTrait + Clone> InnerUsersmapStruct<T> {
    fn new() -> Self {
        InnerUsersmapStruct {
            idmap: HashMap::new(),
            amap: HashMap::new(),
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
            m: Mutex::new(InnerUsersmapStruct::new()),
        }
    }

    pub async fn add_user(&mut self, u: T) {
        let uc = u.clone();
        let mut inner = self.m.lock();
        inner.idmap.insert(u.identity_str(), u);
        inner.amap.insert(uc.auth_str(), uc);
    }

    pub async fn len(&self) -> usize {
        self.m.lock().idmap.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.m.lock().idmap.is_empty()
    }
}

#[async_trait]
impl<T: UserTrait + Clone> AsyncUserAuthenticator<T> for UsersMap<T> {
    async fn auth_user_by_authstr(&self, authstr: &str) -> Option<T> {
        let inner = self.m.lock();
        let s = authstr.to_string();
        inner.amap.get(&s).cloned()
    }
}

#[cfg(test)]
mod test {
    use anyhow::*;
    use futures::executor::block_on;
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
        block_on(um.add_user(up));
        block_on(um.add_user(up2));

        let x = um.auth_user_by_authstr("plaintext:u").await;

        if x != None {
            return Err(anyhow!("shit,not none"));
        }

        let x = um.auth_user_by_authstr("plaintext:u2\np2").await;

        if x == None {
            Err(anyhow!("shit,none"))
        } else {
            Ok(())
        }
    }
}

use self::dynamic::NextSelector;

use super::*;
use lua::dynamic::Finite;
use mlua::prelude::*;
use mlua::{Lua, LuaSerdeExt, Result, Value};

/// load chain::config::StaticConfig from a lua file which has a
/// "config" global variable
pub fn load_static(lua_text: &str) -> Result<StaticConfig> {
    let lua = Lua::new();

    lua.load(lua_text).eval()?;

    let clt: LuaTable = lua.globals().get("config")?;

    let c: StaticConfig = lua.from_value(Value::Table(clt))?;

    Ok(c)
}

//todo: 写得太乱了，improve code

const GET_DYN_SELECTOR_FOR: &str = "get_dyn_selector_for";

/// test if the lua text is ok for finite dynamic
pub fn try_load_finite_dynamic(lua_text: &str) -> Result<()> {
    let lua = Lua::new();
    lua.load(lua_text).eval()?;

    let lg = lua.globals();

    let _s1: LuaFunction = lg.get(GET_DYN_SELECTOR_FOR)?;

    Ok(())
}

struct SelectorHelper {
    pub inbounds_selector: HashMap<String, LuaNextSelector>,
    pub outbounds_selector: HashMap<String, LuaNextSelector>,
}

pub fn load_finite_dynamic(
    lua_text: String,
) -> Result<(
    StaticConfig,
    Vec<DMIterBox>,
    DMIterBox,
    Arc<HashMap<String, DMIterBox>>,
)> {
    let lua = Lua::new();
    let (sc, sh) = load_finite_dynamic_helper(&lua, lua_text)?;

    let (ibs, fb, obm) = get_dmiter_from_static_config_and_helper(sc.clone(), sh);
    Ok((sc, ibs, fb, obm))
}

fn load_finite_dynamic_helper(
    lua: &Lua,
    lua_text: String,
) -> Result<(StaticConfig, SelectorHelper)> {
    lua.load(&lua_text).eval()?;

    let lg = lua.globals();

    let clt: LuaTable = lg.get("config").context("lua has no config field")?;

    let _: LuaFunction = lg
        .get(GET_DYN_SELECTOR_FOR)
        .context(format!("lua has no {}", GET_DYN_SELECTOR_FOR))?;

    let c: StaticConfig = lua.from_value(Value::Table(clt))?;
    let x: HashMap<String, LuaNextSelector> = c
        .inbounds
        .iter()
        .map(|x| {
            let tag = x.tag.as_ref().unwrap();

            let real_lua = Lua::new();
            real_lua.load(&lua_text).eval::<()>().expect("must be ok");

            let real_getter: LuaFunction = real_lua
                .globals()
                .get(GET_DYN_SELECTOR_FOR)
                .expect("must be ok");

            let x = match real_getter.call::<String, LuaFunction>(tag.to_string()) {
                Ok(rst) => rst,
                Err(err) => {
                    panic!("get get_dyn_selector_for for {tag} err: {}", err);
                }
            };
            (
                tag.to_string(),
                LuaNextSelector(Arc::new(parking_lot::Mutex::new(x.into_owned()))),
            )
        })
        .collect();

    let y: HashMap<String, LuaNextSelector> = c
        .outbounds
        .iter()
        .map(|x| {
            let tag = &x.tag;

            let real_lua = Lua::new();
            real_lua.load(&lua_text).eval::<()>().expect("must be ok");

            let real_getter: LuaFunction = real_lua
                .globals()
                .get(GET_DYN_SELECTOR_FOR)
                .expect("must be ok");

            let x = match real_getter.call::<String, LuaFunction>(tag.to_string()) {
                Ok(rst) => rst,
                Err(err) => {
                    panic!("get get_dyn_selector_for for {tag} err: {}", err);
                }
            };
            (
                tag.to_string(),
                LuaNextSelector(Arc::new(parking_lot::Mutex::new(x.into_owned()))),
            )
        })
        .collect();

    let sh = SelectorHelper {
        inbounds_selector: x,
        outbounds_selector: y,
    };

    Ok((c, sh))
}

/// returns inbounds, first_outbound, outbound_map
fn get_dmiter_from_static_config_and_helper(
    c: StaticConfig,
    mut sh: SelectorHelper,
) -> (Vec<DMIterBox>, DMIterBox, Arc<HashMap<String, DMIterBox>>) {
    let ibs = c.get_inbounds();
    let v: Vec<DMIterBox> = ibs
        .into_iter()
        .map(|v| {
            let tag = v.last().unwrap().get_chain_tag().to_string();
            let inbound: Vec<_> = v.into_iter().map(|o| Arc::new(o)).collect();

            let selector = Box::new(sh.inbounds_selector.remove(&tag).unwrap());

            let x: DMIterBox = Box::new(Finite {
                mb_vec: inbound,
                current_index: -1,
                history: Vec::new(),
                selector,
            });
            x
        })
        .collect();

    let obs = c.get_outbounds();

    let mut first_o: Option<DMIterBox> = None;

    let omap: HashMap<String, DMIterBox> = obs
        .into_iter()
        .map(|outbound| {
            let tag = outbound
                .iter()
                .next()
                .expect("outbound should has at least one mapper ")
                .get_chain_tag();

            let ts = tag.to_string();
            let outbound: Vec<_> = outbound.into_iter().map(|o| Arc::new(o)).collect();

            let selector = Box::new(sh.outbounds_selector.remove(&ts).unwrap());

            let outbound_iter: DMIterBox = Box::new(Finite {
                mb_vec: outbound,
                current_index: -1,
                history: Vec::new(),
                selector,
            });

            if let None = first_o {
                first_o = Some(outbound_iter.clone());
            }

            (ts, outbound_iter)
        })
        .collect();

    (v, first_o.expect("has an outbound"), Arc::new(omap))
}

///(用OwnedFunction 后无生命周期了, 但用了 mlua 的 unstable feature)
///
/// https://github.com/mlua-rs/mlua/pull/148
///
/// https://github.com/mlua-rs/mlua/issues/262
///
#[derive(Debug, Clone)]
pub struct LuaNextSelector(Arc<parking_lot::Mutex<mlua::OwnedFunction>>);

unsafe impl Send for LuaNextSelector {}
unsafe impl Sync for LuaNextSelector {}

impl NextSelector for LuaNextSelector {
    fn next_index(&self, this_index: i64, data: Option<Vec<ruci::map::OptVecData>>) -> Option<i64> {
        let w = OptVecOptVecDataLuaWrapper(data);
        let f = self.0.lock();

        match f.call::<_, i64>((this_index, w)) {
            Ok(rst) => Some(rst),
            Err(err) => {
                warn!("{}", err);
                None
            }
        }
    }
}

/*
//////////////////////////////////////////////////////////////////////////////

         rust -> lua UserData 的包装

//////////////////////////////////////////////////////////////////////////////
*/

use mlua::{UserData, UserDataMethods};

#[repr(transparent)]
pub struct AnyDataLuaWrapper(ruci::map::AnyData);

//todo: 都用clone 太慢了, 加 add_method_mut

impl UserData for AnyDataLuaWrapper {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("get_type", |_, this, ()| Ok(this.0.get_type_str()));

        methods.add_method("get_string", |_, this, ()| match &this.0 {
            AnyData::String(s) => Ok(s.to_owned()),
            _ => Err(LuaError::DeserializeError("can't get string".to_string())),
        });

        methods.add_method("get_u64", |_, this, ()| match &this.0 {
            AnyData::U64(u) => Ok(*u),
            AnyData::AU64(au) => Ok(au.load(std::sync::atomic::Ordering::Relaxed)),
            _ => Err(LuaError::DeserializeError("can't get u64".to_string())),
        });
    }
}

pub struct VecOfAnyDataLuaWrapper(Vec<ruci::map::AnyData>);

impl UserData for VecOfAnyDataLuaWrapper {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("len", |_, this, ()| Ok(this.0.len()));

        methods.add_method("get", |_, this, index: usize| {
            let x = this.0.get(index);
            match x {
                Some(d) => Ok(AnyDataLuaWrapper(d.clone())),
                None => Err(LuaError::DeserializeError("can't get u64".to_string())),
            }
        });
    }
}

pub struct OptVecDataLuaWrapper(OptVecData);

impl UserData for OptVecDataLuaWrapper {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("has_value", |_, this, ()| Ok(this.0.is_some()));

        methods.add_method("get_type", |_, this, ()| match &this.0 {
            Some(vd) => match vd {
                VecAnyData::Data(_) => Ok("data"),
                VecAnyData::Vec(_) => Ok("vec"),
            },
            None => Ok("None"),
        });

        methods.add_method("get_data", |_, this, ()| match &this.0 {
            Some(vd) => match vd {
                VecAnyData::Data(d) => Ok(AnyDataLuaWrapper(d.clone())),
                VecAnyData::Vec(_) => Err(LuaError::DeserializeError("can't get data".to_string())),
            },
            None => Err(LuaError::DeserializeError("can't get data".to_string())),
        });

        methods.add_method("get_vec", |_, this, ()| match &this.0 {
            Some(vd) => match vd {
                VecAnyData::Data(_) => Err(LuaError::DeserializeError("can't get vec".to_string())),
                VecAnyData::Vec(v) => Ok(VecOfAnyDataLuaWrapper(v.clone())),
            },
            None => Err(LuaError::DeserializeError("can't get vec".to_string())),
        });
    }
}

/// 对 dynamic::NextSelector 的 next_index 方法 的 data 参数
/// 的类型的包装
pub struct OptVecOptVecDataLuaWrapper(Option<Vec<OptVecData>>);

impl UserData for OptVecOptVecDataLuaWrapper {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("is_some", |_, this, ()| Ok(this.0.is_some()));

        methods.add_method("len", |_, this, ()| Ok(this.0.as_ref().unwrap().len()));

        methods.add_method("get", |_, this, index: usize| {
            let x = this.0.as_ref().unwrap().get(index);
            match x {
                Some(d) => Ok(OptVecDataLuaWrapper(d.clone())),
                None => Err(LuaError::DeserializeError("can't get u64".to_string())),
            }
        });
    }
}

/*
//////////////////////////////////////////////////////////////////////////////

        end of rust -> lua UserData 的包装

//////////////////////////////////////////////////////////////////////////////
*/

#[allow(unused)]
#[cfg(test)]
mod test {

    #[allow(unused)]
    #[test]
    fn test_transmute() {
        use std::mem;

        // 定义结构体 A 和 B
        #[derive(Debug)]
        struct A {
            i: i32,
        };

        #[repr(transparent)]
        #[derive(Debug)]
        struct B(A);

        // 假设 vec_a 是 Vec<A> 类型的向量
        let vec_a: Vec<A> = vec![A { i: 1 }, A { i: 1 }, A { i: 1 }];

        // 使用 transmute 将 Vec<A> 转换为 Vec<B>
        let vec_b: Vec<B> = unsafe { mem::transmute(vec_a) };
        println!("{:?}", vec_b)
    }

    use std::net::TcpListener;
    use std::sync::atomic::AtomicU64;

    use super::*;
    use mlua::prelude::*;
    use mlua::{Error, Lua, LuaSerdeExt, Result, UserData, Value};

    use super::*;

    #[test]
    fn testin() -> Result<()> {
        let text = r#"
    
        tls = { TLS = {  cert = "test.cert", key = "test.key" } }
        listen = { Listener =  "0.0.0.0:1080"  }
        c = "Counter"
        chain1 = {
            listen,
            { Adder = 3 },
            c,
            tls,
            c,
            { Socks5 = {  userpass = "u0 p0", more = {"u1 p1"} } },
            c,

        }
        len = table.getn(chain1)
        for i=1,5 do 
            chain1[len+1] = tls
            chain1[len+2] = c 
            len = len + 2
        end

        config = {
            inbounds = {
                {chain = chain1, tag = "listen1"}
            },
            outbounds = {}
        }
    "#;

        let c: StaticConfig = load_static(text)?;

        println!("{:#?}", c);
        let first_listen_group = c.inbounds.first().unwrap();
        let last_m = first_listen_group.chain.last().unwrap();
        assert!(matches!(InMapperConfig::Counter, last_m));

        let first_m = first_listen_group.chain.first().unwrap();
        let str = "0.0.0.0:1080".to_string();
        assert!(matches!(first_m, InMapperConfig::Listener(str)));
        let str2 = "0.0.0.0:1".to_string();
        assert!(matches!(
            first_m,
            InMapperConfig::Listener(str2) //won't match inner fields
        ));
        assert!(!matches!(first_m, InMapperConfig::Counter));
        Ok(())
    }

    #[test]
    fn testout() -> Result<()> {
        let lua = Lua::new();
        let globals = lua.globals();

        let text = r#"
    
            tls = { TLS = {  host = "my.com", insecure = true } }
            dialer = { Dialer =  "0.0.0.0:1081"   }
            c = "Counter"
            chain1 = {
                dialer,
                { Adder = 3 },
                c,
                tls,
                c,
                { Socks5 = {  userpass = "u0 p0" , early_data = true } },
                c,
    
            }
            len = table.getn(chain1)
            for i=1,5 do 
                chain1[len+1] = tls
                chain1[len+2] = c 
                len = len + 2
            end
    
            config = {
                inbounds = {},
                outbounds = {
                    {chain = chain1, tag = "dial1"}
                }
            }
        "#;

        let c: StaticConfig = load_static(text)?;

        println!("{:#?}", c);
        let dial = c.outbounds;
        let first_listen_group = dial.first().unwrap();
        let last_m = first_listen_group.chain.last().unwrap();
        assert!(matches!(InMapperConfig::Counter, last_m));

        let first_m = first_listen_group.chain.first().unwrap();
        let str = "0.0.0.0:1080".to_string();
        assert!(matches!(first_m, OutMapperConfig::Dialer(str)));
        let str2 = "0.0.0.0:1".to_string();
        assert!(matches!(
            first_m,
            OutMapperConfig::Dialer(str2) //won't match inner fields
        ));
        assert!(!matches!(first_m, OutMapperConfig::Counter));
        Ok(())
    }

    #[test]
    fn testout2() -> Result<()> {
        let lua = Lua::new();
        let globals = lua.globals();

        let text = r#"
        listen = { Listener =  "0.0.0.0:1080"   }
        chain1 = {
            listen,
            { Socks5 = {   } },
        }
        
        config = {
            inbounds = {
                {chain = chain1, tag = "listen1"}
            },
            outbounds = {
                { 
                    tag="dial1", chain = {
                        { Dialer =  "0.0.0.0:1080"  }
                    } 
                }
            }
        }
        "#;

        let c: StaticConfig = load_static(text)?;

        println!("{:#?}", c);
        let dial = c.outbounds;
        let first_listen_group = dial.first().unwrap();
        let last_m = first_listen_group.chain.last().unwrap();
        assert!(matches!(InMapperConfig::Counter, last_m));

        let first_m = first_listen_group.chain.first().unwrap();
        let str = "0.0.0.0:1080".to_string();
        assert!(matches!(first_m, OutMapperConfig::Dialer(str)));
        let str2 = "0.0.0.0:1".to_string();
        assert!(matches!(
            first_m,
            OutMapperConfig::Dialer(str2) //won't match inner fields
        ));
        assert!(!matches!(first_m, OutMapperConfig::Counter));
        Ok(())
    }

    #[test]
    fn testout3() -> Result<()> {
        let lua = Lua::new();
        let globals = lua.globals();

        let text = r#"

        isac2 = { { Stdio={ fixed_target_addr= "udp://127.0.0.1:20800", pre_defined_early_data = "abc" } } , { Adder = 1 } } 

        out_socks5_c = {{ Socks5 = {} }}

        config = {
            inbounds = { 
                {chain = isac2, tag = "in_stdio_adder_chain"} , 
            } ,
            outbounds = { 
                { tag="d1", chain = out_socks5_c } , 
            },
        
        }
        "#;

        let c: StaticConfig = load_static(text)?;

        println!("{:#?}", c);
        let dial = c.outbounds;
        let first_listen_group = dial.first().unwrap();
        let last_m = first_listen_group.chain.last().unwrap();
        // assert!(matches!(InMapperConfig::Counter, last_m));

        // let first_m = first_listen_group.chain.first().unwrap();
        // let str = "0.0.0.0:1080".to_string();
        // assert!(matches!(first_m, OutMapperConfig::Dialer(str)));
        // let str2 = "0.0.0.0:1".to_string();
        // assert!(matches!(
        //     first_m,
        //     OutMapperConfig::Dialer(str2) //won't match inner fields
        // ));
        // assert!(!matches!(first_m, OutMapperConfig::Counter));
        Ok(())
    }

    #[test]
    fn test_tag_route() -> Result<()> {
        let lua = Lua::new();
        let globals = lua.globals();

        let text = r#"
        listen = { Listener =   "0.0.0.0:1080"   }
        chain1 = {
            listen,
            { Socks5 = {   } },
        }
        
        config = {
            inbounds = {
                {chain = chain1, tag = "listen1"},
                {chain = { Stdio="myfake.com" }, tag = "listen2"},
            },
            outbounds = {
                { 
                    tag="dial1", chain = {
                        { Dialer =  "0.0.0.0:1080"   }
                    }
                },

                { 
                    tag="dial2", chain = {
                        "Direct"
                    }
                }
            },
            tag_route = { { "listen1", "dial1" }, { "listen2", "dial2" }  }
        }
        "#;

        let c: StaticConfig = load_static(text)?;

        println!("{:#?}", c);
        let tr = c.get_tag_route();
        assert!(tr.is_some());
        println!("{:#?}", c.get_tag_route());

        println!("{:#?}", c.get_default_and_outbounds_map());

        Ok(())
    }

    #[test]
    fn test_rule_route() -> Result<()> {
        let lua = Lua::new();
        let globals = lua.globals();

        let text = r#"
        listen = { Listener =   "0.0.0.0:1080"   }
        chain1 = {
            listen,
            { Socks5 = {   } },
        }
        
        config = {
            inbounds = {
                {chain = chain1, tag = "listen1"},
                {chain = { Stdio="myfake.com" }, tag = "listen2"},
            },
            outbounds = {
                { 
                    tag="dial1", chain = {
                        { Dialer =  "0.0.0.0:1080"   }
                    }
                },

                { 
                    tag="dial2", chain = {
                        "Direct"
                    }
                }
            },
            rule_route = { 
                { 
                    out_tag = "dial1", 
                    mode = "WhiteList",
                    in_tags = { "listen1" } ,
                    userset = {
                        { "plaintext:u0 p0", "trojan:mypassword" },
                        { "plaintext:u1 p1", "trojan:password1" },
                    },
                    ta_ip_countries = { "CN", "US" },
                    ta_networks = { "tcp", "udp" },
                    ta_ipv4 = { "192.168.1.0/24" },
                    ta_domain_matcher = {
                        domain_regex = {  "[a-z]+@[a-z]+",
                        "[a-z]+" },
                        domain_set = { "www.baidu.com" },
                    }
                } 
            }
        }
        "#;

        let c: StaticConfig = load_static(text)?;

        println!("{:#?}", c);
        let tr = c.get_rule_route();
        assert!(tr.is_some());
        println!("{:#?}", tr);

        //println!("{:#?}", c.get_default_and_outbounds_map());

        Ok(())
    }

    #[tokio::test]
    async fn test_pass_in_anydata() -> Result<()> {
        use std::rc::Rc;

        let sa = AnyData::String(String::from("mydata"));
        let w = AnyDataLuaWrapper(sa);

        let sa2 = AnyData::U64(321);
        let w2 = AnyDataLuaWrapper(sa2);

        let sa21 = AnyData::AU64(Arc::new(AtomicU64::new(321)));
        let w21 = AnyDataLuaWrapper(sa21);

        let sa22 = AnyData::AU64(Arc::new(AtomicU64::new(321)));

        let va = Some(VecAnyData::Data(sa22));
        let vva = Some(vec![va]);
        let vvaw = OptVecOptVecDataLuaWrapper(vva);

        let lua = Lua::new();
        let lua = Rc::new(lua);

        use mlua::chunk;
        use mlua::Function;

        let handler_fn = lua
            .load(chunk! {
                function(userdata1)
                    print("datai is "..userdata1:get_string())

                end
            })
            .eval::<Function>()
            .expect("cannot create Lua handler");

        let handler_fn2 = lua
            .load(chunk! {
                function(userdata1)
                    print("datai is "..userdata1:get_u64())

                end
            })
            .eval::<Function>()
            .expect("cannot create Lua handler");

        let handler_fn3 = lua
            .load(chunk! {
                function(userdata1)
                    print(userdata1:is_some())
                    print(userdata1:len())

                    x = userdata1:get(0)
                    print(x:has_value())
                    print(x:get_type())
                    y = x:get_data()
                    print(y:get_u64())

                end
            })
            .eval::<Function>()
            .expect("cannot create Lua handler");

        let handler_fn4 = lua
            .load(chunk! {
                function(this_index, ovov)
                    print(ovov:is_some())
                    print(ovov:len())

                    ov = ovov:get(0)
                    print(ov:has_value())
                    print(ov:get_type())
                    d = ov:get_data()
                    print(d:get_u64())

                    return this_index + 1
                end
            })
            .eval::<Function>()
            .expect("cannot create Lua handler");

        let handler = lua
            .create_registry_value(handler_fn)
            .expect("cannot store Lua handler");

        let handler: Function = lua
            .registry_value(&handler)
            .expect("cannot get Lua handler");

        if let Err(err) = handler.call_async::<_, ()>(w).await {
            eprintln!("{}", err);
        }

        if let Err(err) = handler_fn2.call_async::<_, ()>(w2).await {
            eprintln!("{}", err);
        }

        if let Err(err) = handler_fn2.call_async::<_, ()>(w21).await {
            eprintln!("{}", err);
        }

        // if let Err(err) = handler_fn3.call_async::<_, ()>(vvaw).await {
        //     eprintln!("{}", err);
        // }

        match handler_fn4.call::<_, u64>((1, vvaw)) {
            Ok(rst) => println!("{}", rst),
            Err(err) => eprintln!("{}", err),
        }
        Ok(())
    }

    #[tokio::test]
    async fn load_dynamic1() -> Result<()> {
        let lua = Lua::new();
        let lua_text = r"
        function dyn_next_selector(this_index, ovov)
            print(ovov:is_some())
            print(ovov:len())
        
            ov = ovov:get(0)
            print(ov:has_value())
            print(ov:get_type())
            d = ov:get_data()
            print(d:get_u64())
        
            return this_index + 1
        end
        ";

        lua.load(lua_text).eval()?;

        let func: LuaFunction = lua.globals().get("dyn_next_selector")?;

        let sa22 = AnyData::AU64(Arc::new(AtomicU64::new(321)));
        let va = Some(VecAnyData::Data(sa22));
        let vva = Some(vec![va]);
        let vvaw = OptVecOptVecDataLuaWrapper(vva);

        match func.call::<_, u64>((1, vvaw)) {
            Ok(rst) => println!("{}", rst),
            Err(err) => eprintln!("{}", err),
        }

        Ok(())
    }

    #[test]
    fn test_dyn() -> Result<()> {
        let lua = Lua::new();

        use mlua::chunk;
        use mlua::Function;

        let handler_fn = lua
            .load(chunk! {
                function(current_index)



                end
            })
            .eval::<Function>()
            .expect("cannot create Lua handler");

        Ok(())
    }
}

/*
#[allow(unused)]
#[cfg(test)]
mod test1 {

    // example from mlua

    use std::io;
    use std::net::SocketAddr;
    use std::rc::Rc;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::task;

    use mlua::{chunk, Function, Lua, RegistryKey, String as LuaString, UserData, UserDataMethods};

    struct LuaTcpStream(TcpStream);

    impl UserData for LuaTcpStream {
        fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
            methods.add_method("peer_addr", |_, this, ()| {
                Ok(this.0.peer_addr()?.to_string())
            });

            methods.add_async_method_mut("read", |lua, this, size| async move {
                let mut buf = vec![0; size];
                let n = this.0.read(&mut buf).await?;
                buf.truncate(n);
                lua.create_string(&buf)
            });

            methods.add_async_method_mut("write", |_, this, data: LuaString| async move {
                let n = this.0.write(&data.as_bytes()).await?;
                Ok(n)
            });

            methods.add_async_method_mut("close", |_, this, ()| async move {
                this.0.shutdown().await?;
                Ok(())
            });
        }
    }

    pub async fn run_server(lua: Lua, handler: RegistryKey) -> io::Result<()> {
        let addr: SocketAddr = ([127, 0, 0, 1], 3000).into();
        let listener = TcpListener::bind(addr).await.expect("cannot bind addr");

        println!("Listening on {}", addr);

        let lua = Rc::new(lua);
        let handler = Rc::new(handler);
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(res) => res,
                Err(err) if is_transient_error(&err) => continue,
                Err(err) => return Err(err),
            };

            let lua = lua.clone();
            let handler = handler.clone();
            task::spawn_local(async move {
                let handler: Function = lua
                    .registry_value(&handler)
                    .expect("cannot get Lua handler");

                let stream = LuaTcpStream(stream);
                if let Err(err) = handler.call_async::<_, ()>(stream).await {
                    eprintln!("{}", err);
                }
            });
        }
    }

    //#[tokio::main(flavor = "current_thread")]
    #[tokio::test]
    pub async fn main() {
        let lua = Lua::new();

        // Create Lua handler function
        let handler_fn = lua
            .load(chunk! {
                function(stream)
                    local peer_addr = stream:peer_addr()
                    print("connected from "..peer_addr)

                    while true do
                        local data = stream:read(100)
                        data = data:match("^%s*(.-)%s*$") // trim
                        print("["..peer_addr.."] "..data)
                        if data == "bye" then
                            stream:write("bye bye\n")
                            stream:close()
                            return
                        end
                        stream:write("echo: "..data.."\n")
                    end
                end
            })
            .eval::<Function>()
            .expect("cannot create Lua handler");

        // Store it in the Registry
        let handler = lua
            .create_registry_value(handler_fn)
            .expect("cannot store Lua handler");

        task::LocalSet::new()
            .run_until(run_server(lua, handler))
            .await
            .expect("cannot run server")
    }

    fn is_transient_error(e: &io::Error) -> bool {
        e.kind() == io::ErrorKind::ConnectionRefused
            || e.kind() == io::ErrorKind::ConnectionAborted
            || e.kind() == io::ErrorKind::ConnectionReset
    }
}
*/

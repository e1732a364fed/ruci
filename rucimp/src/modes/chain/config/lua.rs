use std::mem;

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

const DYN_SELECTORS_STR: &str = "dyn_selectors";

/// test if the lua text is ok for finite dynamic
pub fn is_finite_dynamic_available(lua_text: &str) -> Result<()> {
    let lua = Lua::new();
    lua.load(lua_text).eval()?;

    let lg = lua.globals();

    let _s1: LuaFunction = lg.get(DYN_SELECTORS_STR)?;

    Ok(())
}

pub fn load_finite_dynamic(
    lua_text: &str,
) -> Result<(
    StaticConfig,
    Vec<DMIterBox>,
    DMIterBox,
    Arc<HashMap<String, DMIterBox>>,
)> {
    let (sc, sm) = load_finite_config_and_selector_map(lua_text)?;

    let (ibs, fb, obm) = get_iobounds_by_config_and_selector_map(sc.clone(), sm);
    Ok((sc, ibs, fb, obm))
}

fn load_finite_config_and_selector_map(
    lua_text: &str,
) -> Result<(StaticConfig, HashMap<String, LuaNextSelector>)> {
    let lua = Lua::new();

    lua.load(lua_text).eval()?;

    let g = lua.globals();

    let config: LuaTable = g.get("config").context("lua has no config field")?;

    let _: LuaFunction = g
        .get(DYN_SELECTORS_STR)
        .context(format!("lua has no {}", DYN_SELECTORS_STR))?;

    let c: StaticConfig = lua.from_value(Value::Table(config))?;
    let mut selector_map: HashMap<String, LuaNextSelector> = c
        .inbounds
        .iter()
        .map(|chain| {
            let tag = chain.tag.as_ref().unwrap();

            (
                tag.to_string(),
                LuaNextSelector::from(&lua_text, &tag).expect("get handler from lua must be ok"),
            )
        })
        .collect();

    c.outbounds.iter().for_each(|chain| {
        let tag = &chain.tag;

        selector_map.insert(
            tag.to_string(),
            LuaNextSelector::from(&lua_text, tag).expect("get handler from lua must be ok"),
        );
    });

    Ok((c, selector_map))
}

/// returns inbounds, first_outbound, outbound_map
fn get_iobounds_by_config_and_selector_map(
    c: StaticConfig,
    mut selector_map: HashMap<String, LuaNextSelector>,
) -> (Vec<DMIterBox>, DMIterBox, Arc<HashMap<String, DMIterBox>>) {
    let ibs = c.get_inbounds();
    let v: Vec<DMIterBox> = ibs
        .into_iter()
        .map(|v| {
            let tag = v.last().unwrap().get_chain_tag().to_string();
            let inbound: Vec<_> = v.into_iter().map(|o| Arc::new(o)).collect();

            let selector = Box::new(selector_map.remove(&tag).unwrap());

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

            let selector = Box::new(selector_map.remove(&ts).unwrap());

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

#[derive(Debug, Clone)]
pub struct LuaNextSelector(Arc<Mutex<LuaOwnedFunction>>);

unsafe impl Send for LuaNextSelector {}
unsafe impl Sync for LuaNextSelector {}

impl LuaNextSelector {
    pub fn from(lua_text: &str, tag: &str) -> anyhow::Result<LuaNextSelector> {
        let lua = Lua::new();
        lua.load(lua_text).eval::<()>()?;

        let selectors: LuaFunction = lua.globals().get(DYN_SELECTORS_STR)?;
        let f = match selectors.call::<&str, LuaFunction>(tag) {
            Ok(rst) => rst,
            Err(err) => {
                panic!("get dyn_selectors for {tag} err: {}", err);
            }
        };

        Ok(LuaNextSelector(Arc::new(Mutex::new(f.into_owned()))))
    }
}

impl NextSelector for LuaNextSelector {
    fn next_index(&self, this_index: i64, data: Option<Vec<ruci::map::Data>>) -> Option<i64> {
        let w = OptVecDataLuaWrapper(data);
        let inner = self.0.lock();

        match inner.call::<_, i64>((this_index, w)) {
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
use parking_lot::Mutex;

#[repr(transparent)]
pub struct AnyDataLuaWrapper(ruci::map::AnyData);

impl UserData for AnyDataLuaWrapper {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("get_type", |_, this, ()| Ok(this.0.get_type_str()));

        methods.add_method("get_string", |_, this, ()| match &this.0 {
            AnyData::String(s) => Ok(s.to_owned()),
            _ => Err(LuaError::DeserializeError("can't get string".to_string())),
        });

        methods.add_method_mut("take_string", |_, this, ()| match &mut this.0 {
            AnyData::String(s) => Ok(mem::take(s)),
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

        methods.add_method_mut("take", |_, this, index: usize| {
            match this.0.get_mut(index) {
                Some(d) => Ok(AnyDataLuaWrapper(mem::take(d))),
                None => Err(LuaError::DeserializeError("can't get u64".to_string())),
            }
        });
    }
}

pub struct DataLuaWrapper(Data);

impl UserData for DataLuaWrapper {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("has_value", |_, this, ()| Ok(!this.0.is_empty()));

        methods.add_method("get_type", |_, this, ()| match &this.0 {
            Data::Data(_) => Ok("data"),
            Data::Vec(_) => Ok("vec"),
        });

        methods.add_method("get_data", |_, this, ()| match &this.0 {
            Data::Data(d) => Ok(AnyDataLuaWrapper(d.clone())),
            Data::Vec(_) => Err(LuaError::DeserializeError("can't get data".to_string())),
        });

        methods.add_method("get_vec", |_, this, ()| match &this.0 {
            Data::Data(_) => Err(LuaError::DeserializeError("can't get vec".to_string())),
            Data::Vec(v) => Ok(VecOfAnyDataLuaWrapper(v.clone())),
        });
    }
}

/// 对 dynamic::NextSelector 的 next_index 方法 的 data 参数
/// 的类型的包装
pub struct OptVecDataLuaWrapper(Option<Vec<Data>>);

impl UserData for OptVecDataLuaWrapper {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("is_some", |_, this, ()| Ok(this.0.is_some()));

        methods.add_method("len", |_, this, ()| Ok(this.0.as_ref().unwrap().len()));

        methods.add_method("get", |_, this, index: usize| {
            let x = this.0.as_ref().unwrap().get(index);
            match x {
                Some(d) => Ok(DataLuaWrapper(d.clone())),
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

        #[derive(Debug)]
        struct A {
            i: i32,
        };

        #[repr(transparent)]
        #[derive(Debug)]
        struct B(A);

        let vec_a: Vec<A> = vec![A { i: 1 }, A { i: 1 }, A { i: 1 }];

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

        let va = Data::Data(sa22);
        let vva = Some(vec![va]);
        let vvaw = OptVecDataLuaWrapper(vva);

        let lua = Lua::new();
        let lua = Rc::new(lua);

        use mlua::chunk;
        use mlua::Function;

        let handler_fn = lua
            .load(chunk! {
                function(userdata1)
                    print("data is "..userdata1:get_string())

                    print("data is "..userdata1:take_string())

                    // take 后原值会为空
                    print("data is "..userdata1:take_string())


                end
            })
            .eval::<Function>()
            .expect("cannot create Lua handler");

        let handler_fn2 = lua
            .load(chunk! {
                function(userdata1)
                    print("data is "..userdata1:get_u64())

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

        let handler: LuaRegistryKey = lua
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
        let va = Data::Data(sa22);
        let vva = Some(vec![va]);
        let vvaw = OptVecDataLuaWrapper(vva);

        match func.call::<_, u64>((1, vvaw)) {
            Ok(rst) => println!("{}", rst),
            Err(err) => eprintln!("{}", err),
        }

        Ok(())
    }

    // #[test]
    // fn test_dyn() -> Result<()> {
    //     let lua = Lua::new();

    //     use mlua::chunk;
    //     use mlua::Function;

    //     let handler_fn = lua
    //         .load(chunk! {
    //             function(current_index)

    //             end
    //         })
    //         .eval::<Function>()
    //         .expect("cannot create Lua handler");

    //     Ok(())
    // }
}

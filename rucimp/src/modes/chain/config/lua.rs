use self::dynamic::NextSelector;

use super::*;
use lua::dynamic::Finite;
use mlua::prelude::*;
use mlua::{Lua, LuaSerdeExt, Value};
use parking_lot::Mutex;
use ruci::map::acc::OVOD;

/// load chain::config::StaticConfig from a lua file which has a
/// "config" global variable
pub fn load_static(lua_text: &str) -> mlua::Result<StaticConfig> {
    let lua = Lua::new();

    lua.load(lua_text).exec()?;

    let ct: LuaTable = lua.globals().get("config")?;

    let c: StaticConfig = lua.from_value(Value::Table(ct))?;

    Ok(c)
}

const DYN_SELECTORS_STR: &str = "dyn_selectors";

/// test if the lua text is ok for finite dynamic
pub fn is_finite_dynamic_available(lua_text: &str) -> mlua::Result<()> {
    let lua = Lua::new();
    lua.load(lua_text).eval()?;

    let lg = lua.globals();

    let _s1: LuaFunction = lg.get(DYN_SELECTORS_STR)?;

    Ok(())
}

pub fn load_finite_dynamic(
    lua_text: &str,
) -> mlua::Result<(
    StaticConfig,
    Vec<DMIterBox>,
    DMIterBox,
    Arc<HashMap<String, DMIterBox>>,
)> {
    let (sc, sm) = load_finite_config_and_selector_map(lua_text)?;

    let (ibs, fb, obm) = get_iobounds_by_config_and_selector_map(sc.clone(), sm);
    Ok((sc, ibs, fb, obm))
}

/// load StaticConfig and generate LuaNextSelector from lua code
/// by tag of each chain
fn load_finite_config_and_selector_map(
    lua_text: &str,
) -> mlua::Result<(StaticConfig, HashMap<String, LuaNextSelector>)> {
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

/// implements dynamic::NextSelector
#[derive(Debug, Clone)]
pub struct LuaNextSelector(Arc<Mutex<(Lua, LuaRegistryKey)>>);

unsafe impl Send for LuaNextSelector {}
unsafe impl Sync for LuaNextSelector {}

impl LuaNextSelector {
    pub fn from(lua_text: &str, tag: &str) -> anyhow::Result<LuaNextSelector> {
        let lua = Lua::new();
        lua.load(lua_text).eval::<()>()?;

        let selectors: LuaFunction = lua.globals().get(DYN_SELECTORS_STR)?;
        let selectors = selectors.into_owned();
        let f = match selectors.call::<&str, LuaFunction>(tag) {
            Ok(rst) => rst,
            Err(err) => {
                panic!("get dyn_selectors for {tag} err: {}", err);
            }
        };
        let key: LuaRegistryKey = lua
            .create_registry_value(f)
            .expect("cannot store Lua handler");

        Ok(LuaNextSelector(Arc::new(Mutex::new((lua, key)))))
    }
}

impl NextSelector for LuaNextSelector {
    fn next_index(&self, this_index: i64, data: OVOD) -> Option<i64> {
        let mg = self.0.lock();
        let lua = &mg.0;

        let f: LuaFunction = lua
            .registry_value(&mg.1)
            .expect("must get selector from lua");

        match f.call::<_, i64>((this_index, lua.to_value(&data))) {
            Ok(rst) => Some(rst),
            Err(err) => {
                warn!("{}", err);
                None
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct LuaNextGenerator {
    lua: Arc<Mutex<(Lua, LuaRegistryKey)>>,
    behavior: ProxyBehavior,
}

unsafe impl Send for LuaNextGenerator {}
unsafe impl Sync for LuaNextGenerator {}

impl dynamic::IndexNextMapperGenerator for LuaNextGenerator {
    fn next_mapper(
        &self,
        this_index: i64,
        cache_len: usize,
        data: OVOD,
    ) -> Option<dynamic::IndexMapperBox> {
        let mg = self.lua.lock();
        let lua = &mg.0;

        let f: LuaFunction = lua
            .registry_value(&mg.1)
            .expect("must get generator from lua");

        let r = f.call::<_, (i64, LuaTable)>((this_index, cache_len, lua.to_value(&data)));

        match r {
            Ok(rst) => {
                if rst.0 < 0 {
                    return None;
                }
                if (rst.0 as usize) < cache_len {
                    return Some((rst.0, None));
                }

                fn get_result<T: for<'de> Deserialize<'de> + ruci::map::ToMapperBox>(
                    lua: &Lua,
                    i: i64,
                    t: LuaTable,
                ) -> Option<dynamic::IndexMapperBox> {
                    let ic: LuaResult<T> = lua.from_value(Value::Table(t));
                    match ic {
                        Ok(ic) => {
                            let mb = ic.to_mapper_box();
                            Some((i, Some(Arc::new(mb))))
                        }
                        Err(e) => {
                            warn!("expect an inmapper config, got error: {e}");
                            None
                        }
                    }
                }

                match self.behavior {
                    ProxyBehavior::UNSPECIFIED => todo!(),
                    ProxyBehavior::ENCODE => get_result::<InMapperConfig>(lua, rst.0, rst.1),
                    ProxyBehavior::DECODE => get_result::<OutMapperConfig>(lua, rst.0, rst.1),
                }
            }
            Err(err) => {
                warn!("getting lua generator failed: {}", err);
                None
            }
        }
    }
}

#[cfg(test)]
#[allow(unused)]
mod test {

    use super::*;
    use mlua::{Error, Lua, LuaSerdeExt};
    use ruci::map;
    use ruci::user::PlainText;
    //https://raw.githubusercontent.com/kikito/inspect.lua/master/inspect.lua

    pub const INSPECT: &str = include_str!("inspect.lua");

    #[test]
    fn testin() -> mlua::Result<()> {
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
    fn testout() -> mlua::Result<()> {
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
    fn testout2() -> mlua::Result<()> {
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
    fn testout3() -> mlua::Result<()> {
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
        println!("{:#?}", last_m);

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
    fn test_tag_route() -> mlua::Result<()> {
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
    fn test_rule_route() -> mlua::Result<()> {
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

    fn get_ovod() -> anyhow::Result<OVOD> {
        let u1 = 3u8;
        let boxed_u1: Box<dyn Data> = Box::new(u1);

        let addr = net::Addr::from_addr_str("tcp", "127.0.0.1:80")?;
        let boxed_a1: Box<dyn Data> = Box::new(map::RAddr(addr));

        let pt = PlainText::new("user".to_string(), "pass".to_string());
        let boxed_pt: Box<dyn Data> = Box::new(pt);

        Ok(Some(vec![
            Some(boxed_u1),
            Some(boxed_a1),
            None,
            Some(boxed_pt),
        ]))
    }
    #[test]
    fn test_serde_json() -> anyhow::Result<()> {
        let ovod = get_ovod()?;
        let json_str = serde_json::to_string_pretty(&ovod).map_err(Error::external)?;
        println!("{}", json_str);
        Ok(())
    }

    #[tokio::test]
    async fn test_pass_in_data() -> anyhow::Result<()> {
        let ovod = get_ovod()?;

        let lua = Lua::new();

        use mlua::chunk;
        use mlua::Function;

        let inspect: LuaTable = lua.load(INSPECT).eval()?;
        lua.globals().set("inspect", inspect)?;

        //rust None in lua will be: userdata: 0x0000000000000000,
        //Other types will be a lua table.

        let f = lua
            .load(chunk! {
                function(vec)
                print( inspect(vec))

                    print("data is ",vec,type(vec))
                    print("data1 is ",vec[1])
                    print("data2 is ",vec[2])
                    print("data3 is ",vec[3], type(vec[3]))

                    print("inspect", inspect(vec[3]))

                    print("data4 is ",vec[4])
                    print("data is ",vec[1]["type"])
                    print("data is ",vec[1]["value"])

                end
            })
            .eval::<Function>()
            .expect("cannot create Lua handler");

        let key: LuaRegistryKey = lua
            .create_registry_value(f)
            .expect("cannot store Lua handler");

        let f: Function = lua.registry_value(&key).expect("cannot get Lua handler");

        if let Err(err) = f.call_async::<_, ()>(lua.to_value(&ovod)?).await {
            eprintln!("{}", err);
        }

        Ok(())
    }

    #[tokio::test]
    async fn load_finite_dynamic1() -> anyhow::Result<()> {
        let lua = Lua::new();
        let lua_text = r#"
           function dyn_next_selector(this_index, ovod)
               print("ovod:",ovod)

               return this_index + 1
           end
           "#;

        lua.load(lua_text).eval()?;

        let func: LuaFunction = lua.globals().get("dyn_next_selector")?;

        let ovod = get_ovod()?;

        match func.call::<_, u64>((1, lua.to_value(&ovod)?)) {
            Ok(rst) => println!("{}", rst),
            Err(err) => eprintln!("{}", err),
        }

        Ok(())
    }

    #[tokio::test]
    async fn load_infinite() -> anyhow::Result<()> {
        let text = r#"
        
dyn_config = {
    inbounds = {{
        tag = "listen1",
        generator = function(this_index, cache_len, data)
            if this_index == -1 then
                return 0, {
                    Listener = "0.0.0.0:10800"
                }

            elseif this_index == 0 then
                return 1, {
                    Socks5 = {}
                }
            else
                return -1, {}

            end
        end
    }, {
        tag = "listen2",
        generator = function(this_index, cache_len, data)
            return -1, {}
        end
    }},

    outbounds = {{
        tag = "dial1",
        generator = function(this_index, cache_len, data)
            if this_index == -1 then
                return "Direct"
            end
        end
    }, {
        tag = "dial2",
        generator = function(this_index, cache_len, data)
            return -1, {}
        end
    }}

}
        "#;

        let lua = Lua::new();
        lua.load(text).eval()?;

        let ct: LuaTable = lua.globals().get("dyn_config")?;
        let inbounds: LuaTable = ct.get("inbounds")?;
        let outbounds: LuaTable = ct.get("outbounds")?;
        inbounds.for_each(|k: usize, v: LuaTable| {
            println!("{k},{:?}", v);
            let tag: String = v.get("tag")?;
            println!("tag, {tag}");

            let g: LuaFunction = v.get("generator")?;
            println!("g, {:?}", g);

            Ok(())
        });

        Ok(())
    }
}

use self::dynamic::NextSelector;

use super::*;
use lua::dynamic::Finite;
use mlua::prelude::*;
use mlua::{Lua, LuaSerdeExt, Value};
use parking_lot::Mutex;
use ruci::map::acc::OVOD;
use ruci::net::CID;

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

pub type LoadFiniteDynamicResult = (
    StaticConfig,
    Vec<DMIterBox>,
    DMIterBox,
    Arc<HashMap<String, DMIterBox>>,
);

pub fn load_finite_dynamic(lua_text: &str) -> mlua::Result<LoadFiniteDynamicResult> {
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
                LuaNextSelector::from(lua_text, tag).expect("get handler from lua must be ok"),
            )
        })
        .collect();

    c.outbounds.iter().for_each(|chain| {
        let tag = &chain.tag;

        selector_map.insert(
            tag.to_string(),
            LuaNextSelector::from(lua_text, tag).expect("get handler from lua must be ok"),
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
            let inbound: Vec<_> = v.into_iter().map(Arc::new).collect();

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
                .first()
                .expect("outbound should has at least one mapper ")
                .get_chain_tag();

            let ts = tag.to_string();
            let outbound: Vec<_> = outbound.into_iter().map(Arc::new).collect();

            let selector = Box::new(selector_map.remove(&ts).unwrap());

            let outbound_iter: DMIterBox = Box::new(Finite {
                mb_vec: outbound,
                current_index: -1,
                history: Vec::new(),
                selector,
            });

            if first_o.is_none() {
                first_o = Some(outbound_iter.clone());
            }

            (ts, outbound_iter)
        })
        .collect();

    (v, first_o.expect("has an outbound"), Arc::new(omap))
}

/// used by load_infinite,
pub type GMAP = HashMap<String, LuaNextGenerator>;

/// get (inbounds generator map, outbounds generator map)
pub fn load_infinite_io(text: &str) -> anyhow::Result<(GMAP, GMAP)> {
    let i = get_infinite_gmap_from(text, ProxyBehavior::DECODE)?;
    let o = get_infinite_gmap_from(text, ProxyBehavior::ENCODE)?;
    Ok((i, o))
}

fn get_infinite_gmap_from(text: &str, behavior: ProxyBehavior) -> anyhow::Result<GMAP> {
    let mut gmap: GMAP = HashMap::new();

    let lua = Lua::new();
    lua.load(text).eval()?;

    let ct: LuaTable = lua.globals().get("dyn_config")?;

    let tkey = match behavior {
        ProxyBehavior::UNSPECIFIED => todo!(),
        ProxyBehavior::DECODE => "inbounds",
        ProxyBehavior::ENCODE => "outbounds",
    };

    let table: LuaTable = ct.get(tkey)?;
    let ibc = table.len()?;

    for i in 1..ibc + 1 {
        let lua = Lua::new();
        lua.load(text).eval()?;

        let (key, tag) = {
            let ct: LuaTable = lua.globals().get("dyn_config")?;
            let table: LuaTable = ct.get(tkey)?;

            let chain: LuaTable = table.get(i)?;
            let tag: String = chain.get("tag")?;
            let g: LuaFunction = chain.get("generator")?;

            let key = lua.create_registry_value(g).expect("ok");

            (key, tag)
        };

        let lng = LuaNextGenerator::new(lua, key, behavior);

        gmap.insert(tag, lng);
    }
    Ok(gmap)
}

/// implements dynamic::NextSelector
#[derive(Debug, Clone)]
pub struct LuaNextSelector(Arc<Mutex<InnerLuaNextSelector>>);

#[derive(Debug)]
struct InnerLuaNextSelector(Lua, LuaRegistryKey);

unsafe impl Send for InnerLuaNextSelector {}
unsafe impl Sync for InnerLuaNextSelector {}

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

        Ok(LuaNextSelector(Arc::new(Mutex::new(InnerLuaNextSelector(
            lua, key,
        )))))
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

/// implements dynamic::IndexNextMapperGenerator
#[derive(Debug, Clone)]
pub struct LuaNextGenerator {
    inner: Arc<Mutex<InnerLuaNextGenerator>>,
}
impl LuaNextGenerator {
    pub fn new(lua: Lua, key: LuaRegistryKey, behavior: ProxyBehavior) -> Self {
        Self {
            inner: Arc::new(Mutex::new(InnerLuaNextGenerator::new(lua, key, behavior))),
        }
    }
}

#[derive(Debug)]
struct InnerLuaNextGenerator {
    lua: Lua,
    key: LuaRegistryKey,
    behavior: ProxyBehavior,
    thread_map: HashMap<CID, LuaOwnedThread>,
    create_thread_func_map: HashMap<CID, LuaOwnedFunction>,
}
unsafe impl Send for InnerLuaNextGenerator {}
unsafe impl Sync for InnerLuaNextGenerator {}

fn get_result<T: for<'de> Deserialize<'de> + ruci::map::ToMapperBox>(
    lua: &Lua,
    i: i64,
    t: Value,
) -> Option<dynamic::IndexMapperBox> {
    let ic: LuaResult<T> = lua.from_value(t);
    match ic {
        Ok(ic) => {
            let mb = ic.to_mapper_box();
            Some((i, Some(Arc::new(mb))))
        }
        Err(e) => {
            warn!("expect an mapper config, got error: {e}");
            None
        }
    }
}

impl InnerLuaNextGenerator {
    pub fn new(lua: Lua, key: LuaRegistryKey, behavior: ProxyBehavior) -> Self {
        Self {
            lua,
            key,
            behavior,
            thread_map: HashMap::new(),
            create_thread_func_map: HashMap::new(),
        }
    }
    fn get_result_by_value(&self, i: i64, t: Value) -> Option<dynamic::IndexMapperBox> {
        match self.behavior {
            ProxyBehavior::UNSPECIFIED => todo!(),
            ProxyBehavior::DECODE => get_result::<InMapperConfig>(&self.lua, i, t),
            ProxyBehavior::ENCODE => get_result::<OutMapperConfig>(&self.lua, i, t),
        }
    }

    /// try get result directly, or use field stream_generator
    /// and new_thread_fn
    fn get_from_result(
        &mut self,
        cid: CID,
        cache_len: usize,
        rst: (i64, OwnedTableOrString),
    ) -> Option<dynamic::IndexMapperBox> {
        let i = rst.0;

        if i < 0 {
            return None;
        }
        if (i as usize) < cache_len {
            return Some((i, None));
        }
        match rst.1 {
            OwnedTableOrString::OT(t) => {
                if let Ok(g) = t.to_ref().get::<_, Value>("stream_generator") {
                    if let Value::Nil = g {
                        // debug!("will get r by table");
                        self.get_result_by_value(i, Value::Table(t.to_ref()))
                    } else {
                        // debug!("will get r by g");

                        let r = self.get_result_by_value(i, g);

                        if let Ok(f) = t.to_ref().get::<_, LuaFunction>("new_thread_fn") {
                            let of = f.into_owned();

                            // debug!("storing thread_fn, {cid}");
                            self.create_thread_func_map.insert(cid, of);
                        }

                        r
                    }
                } else {
                    // debug!("will get r by table");
                    self.get_result_by_value(i, Value::Table(t.to_ref()))
                }
            }
            OwnedTableOrString::OS(s) => {
                // debug!("will get r by str");

                self.get_result_by_value(i, Value::String(s.to_ref()))
            }
        }
    }
}

enum OwnedTableOrString {
    OT(LuaOwnedTable),
    OS(LuaOwnedString),
}

impl dynamic::IndexNextMapperGenerator for LuaNextGenerator {
    fn next_mapper(
        &mut self,
        cid: CID,
        this_index: i64,
        cache_len: usize,
        data: OVOD,
    ) -> Option<dynamic::IndexMapperBox> {
        let mut mg = self.inner.lock();
        //debug!("IndexNextMapperGenerator called {cid} ,{:?}", mg.behavior);

        let mut parent = cid.clone();
        parent.pop();

        if !parent.is_zero() {
            //debug!("has parent {parent}");
            if mg.thread_map.contains_key(&cid) {
                let r = {
                    let t = mg.thread_map.get(&cid).expect("ok");
                    if let LuaThreadStatus::Resumable = t.status() {
                        let r = t.resume::<_, (i64, Value)>((this_index, mg.lua.to_value(&data)));

                        let r = r.ok()?;
                        match r.1 {
                            LuaValue::String(t) => {
                                Some((r.0, OwnedTableOrString::OS(t.into_owned())))
                            }
                            LuaValue::Table(t) => {
                                Some((r.0, OwnedTableOrString::OT(t.into_owned())))
                            }

                            _ => None,
                        }
                    } else {
                        None
                    }
                };

                match r {
                    Some(r) => return mg.get_from_result(cid, cache_len, r),
                    None => {
                        mg.thread_map.remove(&cid);
                        return None;
                    }
                };
            }

            if let Some(f) = mg.create_thread_func_map.get(&parent) {
                //debug!("has create_thread_func, {cid}");
                let (t, r) = {
                    let r = {
                        let l = &mg.lua;
                        let t = l.create_thread(f.to_ref()).ok()?;
                        let r = t.resume::<_, (i64, Value)>((this_index, l.to_value(&data)));

                        let r = r.ok()?;

                        let v = match r.1 {
                            LuaValue::String(t) => OwnedTableOrString::OS(t.into_owned()),
                            LuaValue::Table(t) => {
                                // debug!("thread resume got table");
                                OwnedTableOrString::OT(t.into_owned())
                            }

                            _ => panic!("get lua value not string or table"),
                        };

                        let r = (r.0, v);
                        (t.into_owned(), r)
                    };

                    let new_r = mg.get_from_result(cid.clone(), cache_len, r.1);
                    (r.0, new_r)
                };

                if let LuaThreadStatus::Resumable = t.status() {
                    //debug!("has next, storing");
                    mg.thread_map.insert(cid, t);
                }
                return r;
            }
            // debug!("has parent, but has no thread or create thread func");
        }

        //https://docs.rs/mlua/latest/mlua/struct.Thread.html

        let r = {
            let l = &mg.lua;
            let r = l
                .registry_value::<LuaFunction>(&mg.key)
                .expect("must get generator from lua")
                .call::<_, (i64, Value)>((this_index, l.to_value(&data)));
            let r = r.ok()?;

            let v = match r.1 {
                LuaValue::String(t) => OwnedTableOrString::OS(t.into_owned()),
                LuaValue::Table(t) => OwnedTableOrString::OT(t.into_owned()),

                _ => panic!("get lua value not string or table"),
            };

            (r.0, v)
        };

        mg.get_from_result(cid, cache_len, r)
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
        generator = function(this_index, data)
            if this_index == -1 then
                return 0, {
                    stream_generator = {
                        Listener = "0.0.0.0:10800"
                    },
                    new_thread_fn = function(this_index, data)
                        local newi, new_data = coroutine.yield(1, {
                            Socks5 = {}
                        })
                        return -1, {}
                    end
                }
            end
        end
    }, {
        tag = "listen2",
        generator = function(this_index, data)
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

        let gm = load_infinite_io(text)?;
        println!("{:?}", gm);
        Ok(())
    }
}

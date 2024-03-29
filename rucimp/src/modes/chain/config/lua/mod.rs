/*!
lua defines functions to load static, partial(finite) dynamic and complete(infinite) dynamic chain configs.

 */

#[cfg(test)]
#[allow(unused)]
#[cfg(any(feature = "lua", feature = "lua54"))]
pub mod test;

use self::dynamic::NextSelector;

use super::*;
use lua::dynamic::Finite;
use mlua::prelude::*;
use mlua::{Lua, LuaSerdeExt, Value};
use parking_lot::Mutex;
use ruci::map::fold::OVOD;
use ruci::net::CID;

#[derive(Clone)]
pub struct LuaMapWrapper(Arc<MapBox>);

use mlua::UserData;

impl<'lua> FromLua<'lua> for LuaMapWrapper {
    fn from_lua(value: Value<'lua>, _: &'lua Lua) -> LuaResult<Self> {
        match value {
            Value::UserData(ud) => Ok(ud.take::<Self>()?),
            _ => unreachable!(),
        }
    }
}
use mlua::UserDataMethods;
impl UserData for LuaMapWrapper {
    fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("clone", |_, m, ()| Ok(m.clone()));
    }
}

/// set global func Create_in_map for lua
pub fn set_lua_create_in_map_func(lua: &Lua) -> anyhow::Result<()> {
    let f = lua.create_function(|lua, v: LuaValue| {
        let c = lua.from_value::<InMapConfig>(v)?;
        let m = c.to_map_box();
        let m = LuaMapWrapper(Arc::new(m));
        Ok(m)
    })?;
    lua.globals().set("Create_in_map", f)?;
    Ok(())
}

/// set global func Create_out_map for lua
pub fn set_lua_create_out_map_func(lua: &Lua) -> anyhow::Result<()> {
    let f = lua.create_function(|lua, v: LuaValue| {
        let c = lua.from_value::<OutMapConfig>(v)?;
        let m = c.to_map_box();
        let m = LuaMapWrapper(Arc::new(m));
        Ok(m)
    })?;
    lua.globals().set("Create_out_map", f)?;
    Ok(())
}

/// load chain::config::StaticConfig from a lua file which has a
/// "Config" global variable
pub fn load_static(lua_text: &str) -> mlua::Result<StaticConfig> {
    let lua = Lua::new();

    lua.load(lua_text).exec()?;

    let ct: LuaTable = lua.globals().get("Config")?;

    let c: StaticConfig = lua.from_value(Value::Table(ct))?;

    Ok(c)
}

const DYN_SELECTORS_STR: &str = "Dyn_Selectors";

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

    let (ibs, fb, obm) = get_io_bounds_by_config_and_selector_map(sc.clone(), sm);
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

    let c: LuaTable = g
        .get("Config")
        .with_context(|e| format!("lua has no Config field {}", e))?;

    let _: LuaFunction = g
        .get(DYN_SELECTORS_STR)
        .with_context(|e| format!("lua has no {}, {e}", DYN_SELECTORS_STR))?;

    let c: StaticConfig = lua.from_value(Value::Table(c))?;
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
fn get_io_bounds_by_config_and_selector_map(
    c: StaticConfig,
    mut selector_map: HashMap<String, LuaNextSelector>,
) -> (Vec<DMIterBox>, DMIterBox, Arc<HashMap<String, DMIterBox>>) {
    let ibs = c.get_inbounds();
    let v: Vec<DMIterBox> = ibs
        .into_iter()
        .map(|v| {
            // 这里要求所有的 Map 的 get_chain_tag 均不为空
            let tag = v.last().unwrap().get_chain_tag().to_string();
            let inbound: Vec<_> = v.into_iter().map(Arc::new).collect();

            //tracing::debug!("try remove from selector map: {} {:?}", tag, selector_map);

            let selector = Box::new(selector_map.remove(&tag).unwrap());

            let x: DMIterBox = Box::new(Finite {
                mb_vec: inbound,
                current_index: -1,
                selector,
            });
            x
        })
        .collect();

    let obs = c.get_outbounds();

    let mut first_o: Option<DMIterBox> = None;

    let o_map: HashMap<String, DMIterBox> = obs
        .into_iter()
        .map(|outbound| {
            let tag = outbound
                .first()
                .expect("outbound should has at least one map ")
                .get_chain_tag();

            let ts = tag.to_string();
            let outbound: Vec<_> = outbound.into_iter().map(Arc::new).collect();

            let selector = Box::new(selector_map.remove(&ts).unwrap());

            let outbound_iter: DMIterBox = Box::new(Finite {
                mb_vec: outbound,
                current_index: -1,
                selector,
            });

            if first_o.is_none() {
                first_o = Some(outbound_iter.clone());
            }

            (ts, outbound_iter)
        })
        .collect();

    (v, first_o.expect("has an outbound"), Arc::new(o_map))
}

/// used by load_infinite,
pub type GMap = HashMap<String, LuaNextGenerator>;

const INFINITE_CONFIG_FIELD: &str = "Infinite";

/// get (inbounds generator map, outbounds generator map).
///
/// read INFINITE_CONFIG_FIELD  global variable
pub fn load_infinite_io(text: &str) -> anyhow::Result<(GMap, GMap)> {
    let i = get_infinite_g_map_from(text, ProxyBehavior::DECODE)?;
    let o = get_infinite_g_map_from(text, ProxyBehavior::ENCODE)?;
    Ok((i, o))
}

fn get_infinite_g_map_from(text: &str, behavior: ProxyBehavior) -> anyhow::Result<GMap> {
    let mut g_map: GMap = HashMap::new();

    let lua = Lua::new();
    lua.load(text).eval().context("eval lua failed")?;

    let ct: LuaTable = lua
        .globals()
        .get(INFINITE_CONFIG_FIELD)
        .context("get Infinite failed")?;

    let t_key = match behavior {
        ProxyBehavior::UNSPECIFIED => todo!(),
        ProxyBehavior::DECODE => "inbounds",
        ProxyBehavior::ENCODE => "outbounds",
    };

    let table: LuaTable = ct.get(t_key)?;
    let ibc = table.len()?;

    for i in 1..ibc + 1 {
        let lua = Lua::new();
        lua.load(text).eval()?;

        let (key, tag) = {
            let ct: LuaTable = lua.globals().get(INFINITE_CONFIG_FIELD)?;
            let table: LuaTable = ct.get(t_key)?;

            let chain: LuaTable = table.get(i)?;
            let tag: String = chain.get("tag")?;
            let g: LuaFunction = chain.get("generator")?;

            let key = lua.create_registry_value(g).expect("ok");
            set_lua_create_in_map_func(&lua)?;
            set_lua_create_out_map_func(&lua)?;

            (key, tag)
        };

        let lng = LuaNextGenerator::new(tag.clone(), lua, key, behavior);

        g_map.insert(tag, lng);
    }
    Ok(g_map)
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
                panic!("get Dyn_Selectors for {tag} err: {}", err);
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

/// implements dynamic::IndexNextMapGenerator
#[derive(Debug, Clone)]
pub struct LuaNextGenerator {
    inner: Arc<Mutex<InnerLuaNextGenerator>>,
}
impl LuaNextGenerator {
    pub fn new(tag: String, lua: Lua, key: LuaRegistryKey, behavior: ProxyBehavior) -> Self {
        Self {
            inner: Arc::new(Mutex::new(InnerLuaNextGenerator::new(
                tag, lua, key, behavior,
            ))),
        }
    }
}

#[derive(Debug)]
struct InnerLuaNextGenerator {
    tag: String,

    lua: Lua,
    key: LuaRegistryKey,
    behavior: ProxyBehavior,
    thread_map: HashMap<CID, LuaOwnedThread>,
    create_thread_func_map: HashMap<CID, LuaOwnedFunction>,
}
unsafe impl Send for InnerLuaNextGenerator {}
unsafe impl Sync for InnerLuaNextGenerator {}

impl InnerLuaNextGenerator {
    pub fn new(tag: String, lua: Lua, key: LuaRegistryKey, behavior: ProxyBehavior) -> Self {
        Self {
            tag,
            lua,
            key,
            behavior,
            thread_map: HashMap::new(),
            create_thread_func_map: HashMap::new(),
        }
    }
    fn get_result_by_value(&self, i: i64, t: Value) -> Option<dynamic::IndexMapBox> {
        match self.behavior {
            ProxyBehavior::UNSPECIFIED => todo!(),
            ProxyBehavior::DECODE => self.lua_value_to_oim::<InMapConfig>(i, t),
            ProxyBehavior::ENCODE => self.lua_value_to_oim::<OutMapConfig>(i, t),
        }
    }

    fn lua_value_to_oim<T: for<'de> Deserialize<'de> + ruci::map::ToMapBox>(
        &self,
        i: i64,
        v: Value,
    ) -> Option<dynamic::IndexMapBox> {
        let ic: LuaResult<T> = self.lua.from_value(v);
        match ic {
            Ok(ic) => {
                let mut mb = ic.to_map_box();
                mb.set_chain_tag(&self.tag);
                Some((i, Some(Arc::new(mb))))
            }
            Err(e) => {
                warn!("expect an map, got error: {e}");
                None
            }
        }
    }

    /// try get result directly, or use field stream_generator
    /// and new_thread_fn
    fn get_result(
        &mut self,
        cid: CID,
        rst: (i64, LuaMapRepresentation),
    ) -> Option<dynamic::IndexMapBox> {
        let i = rst.0;

        if i < 0 {
            return None;
        }

        match rst.1 {
            LuaMapRepresentation::OT(t) => {
                if let Ok(g) = t.to_ref().get::<_, Value>("stream_generator") {
                    if let Value::Nil = g {
                        self.get_result_by_value(i, Value::Table(t.to_ref()))
                    } else {
                        let r = self.get_result_by_value(i, g);

                        if let Ok(f) = t.to_ref().get::<_, LuaFunction>("new_thread_fn") {
                            let of = f.into_owned();

                            //debug!(cid = %cid,"storing thread_fn");
                            self.create_thread_func_map.insert(cid, of);
                        }

                        r
                    }
                } else {
                    self.get_result_by_value(i, Value::Table(t.to_ref()))
                }
            }
            LuaMapRepresentation::OS(s) => self.get_result_by_value(i, Value::String(s.to_ref())),
            LuaMapRepresentation::OU(ud) => {
                let m = ud.take::<LuaMapWrapper>().expect("ok");
                Some((i, Some(m.0)))
            }
        }
    }
}

enum LuaMapRepresentation {
    OT(LuaOwnedTable),
    OS(LuaOwnedString),
    OU(LuaOwnedAnyUserData),
}

impl dynamic::IndexNextMapGenerator for LuaNextGenerator {
    fn next_map(
        &mut self,
        cid: CID,
        this_state_index: i64,
        data: OVOD,
    ) -> Option<dynamic::IndexMapBox> {
        let mut mg = self.inner.lock();
        //debug!(cid = %cid,"IndexNextMapGenerator called ,{:?}", mg.behavior);

        let mut parent = cid.clone();
        parent.pop();

        if !parent.is_zero() {
            //debug!("has parent {parent}");
            if mg.thread_map.contains_key(&cid) {
                //debug!(cid = %cid,"has previous thread");

                let r = {
                    let t = mg.thread_map.get(&cid).expect("ok");
                    if let LuaThreadStatus::Resumable = t.status() {
                        let cid_v = mg.lua.to_value(&cid).ok()?;

                        let r = t.resume::<_, (i64, Value)>((
                            cid_v,
                            this_state_index,
                            mg.lua.to_value(&data),
                        ));

                        let r = r.ok()?;
                        match r.1 {
                            LuaValue::String(t) => {
                                Some((r.0, LuaMapRepresentation::OS(t.into_owned())))
                            }
                            LuaValue::Table(t) => {
                                Some((r.0, LuaMapRepresentation::OT(t.into_owned())))
                            }
                            LuaValue::UserData(t) => {
                                Some((r.0, LuaMapRepresentation::OU(t.into_owned())))
                            }

                            _ => None,
                        }
                    } else {
                        None
                    }
                };

                match r {
                    Some(r) => return mg.get_result(cid, r),
                    None => {
                        mg.thread_map.remove(&cid);
                        return None;
                    }
                };
            }

            if let Some(f) = mg.create_thread_func_map.get(&parent) {
                //debug!(cid = %cid,"has create_thread_func");
                let (t, r) = {
                    let r = {
                        let l = &mg.lua;
                        let t = l.create_thread(f.to_ref()).ok()?;

                        let cid_v = mg.lua.to_value(&cid).ok()?;

                        let r = t.resume::<_, (i64, Value)>((
                            cid_v,
                            this_state_index,
                            l.to_value(&data),
                        ));

                        let r = r.ok()?;

                        let v = match r.1 {
                            LuaValue::String(t) => LuaMapRepresentation::OS(t.into_owned()),
                            LuaValue::Table(t) => {
                                // debug!("thread resume got table");
                                LuaMapRepresentation::OT(t.into_owned())
                            }
                            LuaValue::UserData(t) => LuaMapRepresentation::OU(t.into_owned()),
                            _ => panic!("get lua value not string or table"),
                        };

                        let r = (r.0, v);
                        (t.into_owned(), r)
                    };

                    let new_r = mg.get_result(cid.clone(), r.1);
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
            let cid_v = l.to_value(&cid).ok()?;
            let r = l
                .registry_value::<LuaFunction>(&mg.key)
                .expect("must get generator from lua")
                .call::<_, (i64, Value)>((cid_v, this_state_index, l.to_value(&data)));
            let r = r.ok()?;

            let v = match r.1 {
                LuaValue::String(t) => LuaMapRepresentation::OS(t.into_owned()),
                LuaValue::Table(t) => LuaMapRepresentation::OT(t.into_owned()),
                LuaValue::UserData(t) => LuaMapRepresentation::OU(t.into_owned()),

                _ => panic!("get lua value not string or table"),
            };

            (r.0, v)
        };

        mg.get_result(cid, r)
    }
}

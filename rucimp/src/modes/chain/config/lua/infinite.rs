use crate::map::lua::{create_load_file_func, MapWrapper};

/*
Defines functions to load infinite(complete) dynamic chain configs from a lua file.
*/
use super::*;
use parking_lot::Mutex;
use ruci::map::fold::OVOD;
use ruci::net::CID;

const INFINITE_CONFIG_FIELD: &str = "Infinite";
const GENERATOR_FIELD: &str = "generator";

/// used by load_infinite,
pub type GMap = HashMap<String, LuaNextGenerator>;

/// set global func Create_in_map for lua.
///
/// 把 lua中的配置 在 rust 中 初始化为 MapBox, 并包在 LuaMapWrapper 中
pub fn set_lua_create_in_map_func(lua: &Lua) -> anyhow::Result<()> {
    let f = lua.create_function(move |lua, v: LuaValue| {
        let c = lua.from_value::<InMapConfig>(v)?;
        let m: MapBox = c.try_into().map_err(mlua::Error::external)?;
        let m = MapWrapper(Arc::new(m));
        Ok(m)
    })?;
    lua.globals().set("Create_in_map", f)?;
    Ok(())
}

/// set global func Create_out_map for lua.
///
/// 把 lua中的配置 在 rust 中 初始化为 MapBox, 并包在 LuaMapWrapper 中
pub fn set_lua_create_out_map_func(lua: &Lua) -> anyhow::Result<()> {
    let f = lua.create_function(move |lua, v: LuaValue| {
        let c = lua.from_value::<OutMapConfig>(v)?;
        let m: MapBox = c.try_into().map_err(mlua::Error::external)?;
        let m = MapWrapper(Arc::new(m));
        Ok(m)
    })?;
    lua.globals().set("Create_out_map", f)?;
    Ok(())
}

/// get (inbounds generator map, outbounds generator map).
///
/// read INFINITE_CONFIG_FIELD  global variable
pub fn load_infinite_io(
    lua_text: &str,
    file_source: Arc<Option<FileSource>>,
) -> anyhow::Result<(GMap, GMap)> {
    let i = get_g_map_from(lua_text, ProxyBehavior::DECODE, file_source.clone())?;
    let o = get_g_map_from(lua_text, ProxyBehavior::ENCODE, file_source)?;
    Ok((i, o))
}

fn get_g_map_from(
    lua_text: &str,
    behavior: ProxyBehavior,
    file_source: Arc<Option<FileSource>>,
) -> anyhow::Result<GMap> {
    let mut g_map: GMap = HashMap::new();

    let lua = Lua::new();

    if let Some(file_source) = file_source.as_ref() {
        create_load_file_func(&lua, file_source)
    }

    lua.load(lua_text).exec().context("eval lua failed")?;

    let t: LuaTable = lua
        .globals()
        .get(INFINITE_CONFIG_FIELD)
        .context("get Infinite Global field failed")?;

    let t_key = match behavior {
        ProxyBehavior::UNSPECIFIED => todo!(),
        ProxyBehavior::DECODE => "inbounds",
        ProxyBehavior::ENCODE => "outbounds",
    };

    let len = {
        let t: LuaTable = t
            .get(t_key)
            .context(format!("get '{}' field failed", t_key))?;
        t.len()?
    };
    // lua 的 index 是从 1 算起
    for i in 1..len + 1 {
        let lua = Lua::new();
        lua.load(lua_text)
            .exec()
            .context(format!("eval lua for {}'s {} item failed", t_key, i))?;

        set_lua_create_in_map_func(&lua)?;
        set_lua_create_out_map_func(&lua)?;

        let (key, tag) = {
            let t: LuaTable = lua.globals().get(INFINITE_CONFIG_FIELD)?;
            let t: LuaTable = t.get(t_key)?;

            let chain: LuaTable = t.get(i)?;
            let tag: String = chain.get("tag").context("get 'tag' field failed")?;
            let g: LuaFunction = chain
                .get(GENERATOR_FIELD)
                .context("get 'generator' field failed")?;

            let key = lua.create_registry_value(g).expect("ok");

            (key, tag)
        };

        let lng = LuaNextGenerator::new(tag.clone(), lua, key, behavior);

        g_map.insert(tag, lng);
    }
    Ok(g_map)
}

/// implements dynamic::IndexNextMapGenerator
#[derive(Debug, Clone)]
pub struct LuaNextGenerator {
    inner: Arc<Mutex<InnerLuaNextGenerator>>,
}
impl LuaNextGenerator {
    //每个 LuaNextGenerator 都持有自己的 Lua 状态
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
    generator_key: LuaRegistryKey,
    behavior: ProxyBehavior,
    thread_map: HashMap<CID, LuaThread>,
    create_thread_func_map: HashMap<CID, LuaFunction>,
}
unsafe impl Send for InnerLuaNextGenerator {}
unsafe impl Sync for InnerLuaNextGenerator {}

impl InnerLuaNextGenerator {
    pub fn new(tag: String, lua: Lua, key: LuaRegistryKey, behavior: ProxyBehavior) -> Self {
        Self {
            tag,
            lua,
            generator_key: key,
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

    fn lua_value_to_oim<
        T: for<'de> Deserialize<'de> + std::fmt::Debug + TryInto<MapBox, Error = anyhow::Error>,
    >(
        &self,
        i: i64,
        v: Value,
    ) -> Option<dynamic::IndexMapBox> {
        let ic: LuaResult<T> = self.lua.from_value(v);
        match ic {
            Ok(ic) => {
                let mut mb: MapBox = ic.try_into().unwrap();
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
                if let Ok(g) = t.get::<Value>("stream_generator") {
                    if let Value::Nil = g {
                        self.get_result_by_value(i, Value::Table(t))
                    } else {
                        let r = self.get_result_by_value(i, g);

                        if let Ok(f) = t.get::<LuaFunction>("new_thread_fn") {
                            let of = f;

                            //debug!(cid = %cid,"storing thread_fn");
                            self.create_thread_func_map.insert(cid, of);
                        }

                        r
                    }
                } else {
                    self.get_result_by_value(i, Value::Table(t))
                }
            }
            LuaMapRepresentation::OS(s) => self.get_result_by_value(i, Value::String(s)),
            LuaMapRepresentation::OU(ud) => {
                let m = ud.take::<MapWrapper>().expect("ok");
                Some((i, Some(m.0)))
            }
        }
    }
}

enum LuaMapRepresentation {
    OT(LuaTable),
    OS(LuaString),
    OU(LuaAnyUserData),
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

                        let r = t.resume::<(i64, Value)>((
                            cid_v,
                            this_state_index,
                            mg.lua.to_value(&data),
                        ));

                        let r = r.ok()?;
                        match r.1 {
                            LuaValue::String(t) => Some((r.0, LuaMapRepresentation::OS(t))),
                            LuaValue::Table(t) => Some((r.0, LuaMapRepresentation::OT(t))),
                            LuaValue::UserData(t) => Some((r.0, LuaMapRepresentation::OU(t))),

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
                        let t = l.create_thread(f.clone()).ok()?;

                        let cid_v = mg.lua.to_value(&cid).ok()?;

                        let r =
                            t.resume::<(i64, Value)>((cid_v, this_state_index, l.to_value(&data)));

                        let r = r.ok()?;

                        let v = match r.1 {
                            LuaValue::String(t) => LuaMapRepresentation::OS(t),
                            LuaValue::Table(t) => {
                                // debug!("thread resume got table");
                                LuaMapRepresentation::OT(t)
                            }
                            LuaValue::UserData(t) => LuaMapRepresentation::OU(t),
                            _ => panic!("get lua value not string or table"),
                        };

                        let r = (r.0, v);
                        (t, r)
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
                .registry_value::<LuaFunction>(&mg.generator_key)
                .expect("must get generator from lua")
                .call::<(i64, Value)>((cid_v, this_state_index, l.to_value(&data)));
            let r = r.ok()?;

            let v = match r.1 {
                LuaValue::String(t) => LuaMapRepresentation::OS(t),
                LuaValue::Table(t) => LuaMapRepresentation::OT(t),
                LuaValue::UserData(t) => LuaMapRepresentation::OU(t),

                _ => panic!("get lua value not string or table"),
            };

            (r.0, v)
        };

        mg.get_result(cid, r)
    }
}

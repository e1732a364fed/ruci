/*
Defines functions to load finite(partial) dynamic chain configs from a lua file.
*/

use super::*;
use dynamic::Finite;
use parking_lot::Mutex;
use ruci::map::fold::OVOD;

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

impl dynamic::NextSelector for LuaNextSelector {
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

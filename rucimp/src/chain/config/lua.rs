use crate::chain::config::*;
use mlua::prelude::*;
use mlua::{Lua, LuaSerdeExt, Result, Value};

pub fn load(lua_text: &str) -> Result<StaticConfig> {
    let lua = Lua::new();

    lua.load(lua_text).eval()?;

    let clt: LuaTable = lua.globals().get("config")?;

    let c: StaticConfig = lua.from_value(Value::Table(clt))?;

    Ok(c)
}

#[allow(unused)]
#[cfg(test)]
mod test {
    use std::net::TcpListener;

    use crate::chain::config::*;
    use mlua::prelude::*;
    use mlua::{Error, Lua, LuaSerdeExt, Result, UserData, Value};

    use super::*;

    #[test]
    fn testin() -> Result<()> {
        let text = r#"
    
        tls = { TLS = {  cert = "test.cert", key = "test.key" } }
        listen = { Listener = { TcpListener = "0.0.0.0:1080" }  }
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
            listen = {
                {chain = chain1, tag = "listen1"}
            }
        }
    "#;

        let c: StaticConfig = load(text)?;

        println!("{:#?}", c);
        let first_listen_group = c.listen.first().unwrap();
        let last_m = first_listen_group.chain.last().unwrap();
        assert!(matches!(InMapper::Counter, last_m));

        let first_m = first_listen_group.chain.first().unwrap();
        let str = "0.0.0.0:1080".to_string();
        assert!(matches!(
            first_m,
            InMapper::Listener(Listener::TcpListener(str))
        ));
        let str2 = "0.0.0.0:1".to_string();
        assert!(matches!(
            first_m,
            InMapper::Listener(Listener::TcpListener(str2)) //won't match inner fields
        ));
        assert!(false == matches!(first_m, InMapper::Counter));
        Ok(())
    }

    #[test]
    fn testout() -> Result<()> {
        let lua = Lua::new();
        let globals = lua.globals();

        let text = r#"
    
            tls = { TLS = {  host = "my.com", insecure = true } }
            dialer = { Dialer = { TcpDialer = "0.0.0.0:1081" }  }
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
                listen = {},
                dial = {
                    {chain = chain1, tag = "dial1"}
                }
            }
        "#;

        let c: StaticConfig = load(text)?;

        println!("{:#?}", c);
        let dial = c.dial.unwrap();
        let first_listen_group = dial.first().unwrap();
        let last_m = first_listen_group.chain.last().unwrap();
        assert!(matches!(InMapper::Counter, last_m));

        let first_m = first_listen_group.chain.first().unwrap();
        let str = "0.0.0.0:1080".to_string();
        assert!(matches!(first_m, OutMapper::Dialer(Dialer::TcpDialer(str))));
        let str2 = "0.0.0.0:1".to_string();
        assert!(matches!(
            first_m,
            OutMapper::Dialer(Dialer::TcpDialer(str2)) //won't match inner fields
        ));
        assert!(false == matches!(first_m, OutMapper::Counter));
        Ok(())
    }
}

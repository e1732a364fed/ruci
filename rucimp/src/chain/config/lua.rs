#[allow(unused)]
#[cfg(test)]
mod test {
    use std::net::TcpListener;

    use crate::chain::config::*;
    use mlua::prelude::*;
    use mlua::{Error, Lua, LuaSerdeExt, Result, UserData, Value};

    use super::*;

    #[test]
    fn test() -> Result<()> {
        let lua = Lua::new();
        let globals = lua.globals();

        lua.load(
            r#"
    
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
                print(len)
            end
    
            config = {
                listen = {
                    {chain = chain1, tag = "listen1"}
                }
            }
        "#,
        )
        .eval()?;
        let c: LuaTable = lua.globals().get("config")?;

        let c: StaticConfig = lua.from_value(
            Value::Table(c), // lua.load(
                             //     r#"

                             // "#,
                             // )
                             // .eval()?,
        )?;

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
}

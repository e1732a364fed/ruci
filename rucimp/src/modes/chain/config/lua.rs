use super::*;
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

#[allow(unused)]
#[cfg(test)]
mod test {
    use std::net::TcpListener;

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

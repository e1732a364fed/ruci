use super::*;
use mlua::{Error, Lua, LuaSerdeExt};
use ruci::map;
use ruci::user::PlainText;
//https://raw.githubusercontent.com/kikito/inspect.lua/master/inspect.lua

pub const INSPECT: &str = include_str!("../../../../../../resource/inspect.lua");

#[test]
fn test_in() -> mlua::Result<()> {
    let text = r#"
    
        tls = { TLS = {  cert = "test.cert", key = "test.key" } }
        listen = { Listener =  { listen_addr = "0.0.0.0:1080"}  }
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
    assert!(matches!(InMapConfig::Counter, last_m));

    let first_m = first_listen_group.chain.first().unwrap();
    let str = "0.0.0.0:1080".to_string();
    assert!(matches!(
        first_m,
        InMapConfig::Listener {
            listen_addr: str,
            ext: None
        }
    ));
    let str2 = "0.0.0.0:1".to_string();
    assert!(matches!(
        first_m,
        InMapConfig::Listener {
            listen_addr: str2,
            ext: None
        } //won't match inner fields
    ));
    assert!(!matches!(first_m, InMapConfig::Counter));
    Ok(())
}

#[test]
fn test_out() -> mlua::Result<()> {
    let text = r#"
    
            tls = { TLS = {  host = "my.com", insecure = true } }
            dialer = { BindDialer =  {dial_addr = "0.0.0.0:1081" }   }
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
    assert!(matches!(InMapConfig::Counter, last_m));

    let first_m = first_listen_group.chain.first().unwrap();
    let str = "0.0.0.0:1080".to_string();
    assert!(matches!(
        first_m,
        OutMapConfig::BindDialer(DialerConfig {
            bind_addr: None,
            dial_addr: str,
            ext: None
        })
    ));
    let str2 = "0.0.0.0:1".to_string();
    assert!(matches!(
        first_m,
        OutMapConfig::BindDialer(DialerConfig {
            bind_addr: None,
            dial_addr: str2,
            ext: None
        }) //won't match inner fields
    ));
    assert!(!matches!(first_m, OutMapConfig::Counter));
    Ok(())
}

#[test]
fn test_out2() -> mlua::Result<()> {
    let text = r#"
        listen = { Listener =   { listen_addr = "0.0.0.0:1080"}   }
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
                        { BindDialer =  { dial_addr = "0.0.0.0:1080"}  }
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
    assert!(matches!(InMapConfig::Counter, last_m));

    let first_m = first_listen_group.chain.first().unwrap();
    let str = "0.0.0.0:1080".to_string();
    assert!(matches!(
        first_m,
        OutMapConfig::BindDialer(DialerConfig {
            bind_addr: None,
            dial_addr: str,
            ext: None
        })
    ));
    let str2 = "0.0.0.0:1".to_string();
    assert!(matches!(
        first_m,
        OutMapConfig::BindDialer(DialerConfig {
            bind_addr: None,
            dial_addr: str2,
            ext: None
        }) //won't match inner fields
    ));
    assert!(!matches!(first_m, OutMapConfig::Counter));
    Ok(())
}

#[test]
fn test_out3() -> mlua::Result<()> {
    let text = r#"

        ic = { { Stdio={ fixed_target_addr= "udp://127.0.0.1:20800", pre_defined_early_data = "abc" } } , { Adder = 1 } } 

        out_socks5_c = {{ Socks5 = {} }}

        config = {
            inbounds = { 
                {chain = ic, tag = "in_stdio_adder_chain"} , 
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

    // assert!(matches!(InMapConfig::Counter, last_m));

    // let first_m = first_listen_group.chain.first().unwrap();
    // let str = "0.0.0.0:1080".to_string();
    // assert!(matches!(first_m, OutMapConfig::BindDialer(str)));
    // let str2 = "0.0.0.0:1".to_string();
    // assert!(matches!(
    //     first_m,
    //     OutMapConfig::BindDialer(str2) //won't match inner fields
    // ));
    // assert!(!matches!(first_m, OutMapConfig::Counter));
    Ok(())
}

#[test]
fn test_tag_route() -> mlua::Result<()> {
    let text = r#"
        listen = { Listener =    { listen_addr = "0.0.0.0:1080"}  }
        chain1 = {
            listen,
            { Socks5 = {   } },
        }
        
        config = {
            inbounds = {
                {chain = chain1, tag = "listen1"},
                {chain = { Stdio="my_fake.com" }, tag = "listen2"},
            },
            outbounds = {
                { 
                    tag="dial1", chain = {
                        { BindDialer =  { dial_addr = "0.0.0.0:1080"}   }
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
        listen = { Listener =    { listen_addr = "0.0.0.0:1080"}   }
        chain1 = {
            listen,
            { Socks5 = {   } },
        }
        
        config = {
            inbounds = {
                {chain = chain1, tag = "listen1"},
                {chain = { Stdio="my_fake.com" }, tag = "listen2"},
            },
            outbounds = {
                { 
                    tag="dial1", chain = {
                        { BindDialer =  { dial_addr = "0.0.0.0:1080" }   }
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
                        { "plaintext:u0 p0", "trojan:my_password" },
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
        
infinite = {
    inbounds = {{
        tag = "listen1",
        generator = function(this_index, data)
            if this_index == -1 then
                return 0, {
                    stream_generator = {
                        Listener =  { listen_addr = "0.0.0.0:10800"}
                    },
                    new_thread_fn = function(this_index, data)
                        local new_i, new_data = coroutine.yield(1, {
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

#[test]
fn test_userdata() -> anyhow::Result<()> {
    use mlua::{MetaMethod, UserData, UserDataMethods};
    #[derive(Copy, Clone, Debug)]
    struct Vec2(f32, f32);

    // We can implement `FromLua` trait for our `Vec2` to return a copy
    impl<'lua> FromLua<'lua> for Vec2 {
        fn from_lua(value: Value<'lua>, _: &'lua Lua) -> LuaResult<Self> {
            match value {
                Value::UserData(ud) => Ok(*ud.borrow::<Self>()?),
                _ => unreachable!(),
            }
        }
    }

    impl UserData for Vec2 {
        fn add_methods<'lua, M: UserDataMethods<'lua, Self>>(methods: &mut M) {
            methods.add_method("magnitude", |_, vec, ()| {
                let mag_squared = vec.0 * vec.0 + vec.1 * vec.1;
                Ok(mag_squared.sqrt())
            });

            methods.add_meta_function(MetaMethod::Add, |_, (vec1, vec2): (Vec2, Vec2)| {
                Ok(Vec2(vec1.0 + vec2.0, vec1.1 + vec2.1))
            });
        }
    }

    let mut lua = Lua::new();
    let mut globals = lua.globals();
    let vec2_constructor = lua.create_function(|_, (x, y): (f32, f32)| Ok(Vec2(x, y)))?;
    globals.set("new_vec2", vec2_constructor)?;

    assert!(
        (lua.load("(new_vec2(1, 2) + new_vec2(2, 2)):magnitude()")
            .eval::<f32>()?
            - 5.0)
            .abs()
            < f32::EPSILON
    );

    let v = lua
        .load(mlua::chunk! {
            new_vec2(1, 3)
        })
        .eval::<LuaValue>()
        .expect("cannot create Vec2");

    match v {
        LuaValue::UserData(ud) => {
            assert!(ud.is::<Vec2>());

            let v = ud.take::<Vec2>().expect("can take");
            println!("{:?}", v);
        }
        _ => panic!("value type not right"),
    };

    Ok(())
}

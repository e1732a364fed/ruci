print("this is a lua config file")

listen = { Listener = { TcpListener = "0.0.0.0:1080" }  }
chain1 = {
    listen,
    { Socks5 = {   } },
}

config = {
    listen = {
        {chain = chain1, tag = "listen1"}
    },
    dial = {
        { 
            tag="dial1", chain = {
                "Direct"
            } 
        }
    }
}

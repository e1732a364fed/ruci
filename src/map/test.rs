use crate::map::{MapParams, Mapper};
use crate::net::helpers::MockTcpStream;

use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use super::ProxyBehavior;

#[tokio::test]
async fn test_adder1() -> std::io::Result<()> {
    let writev = Arc::new(Mutex::new(Vec::new()));
    let writevc = writev.clone();

    let client_tcps = MockTcpStream {
        read_data: Vec::new(),
        write_data: Vec::new(),
        write_target: Some(writev),
    };

    let a = crate::map::math::Adder { addnum: 2 };
    let r = a
        .maps(
            0,
            ProxyBehavior::UNSPECIFIED,
            MapParams::new(Box::new(client_tcps)),
        )
        .await;

    if let Some(e) = r.e {
        return Err(e);
    }

    let mut r = r.c.unwrap();

    {
        let mut buf = [1u8, 2, 3];
        r.write(&mut buf).await?;
        let mut v = writevc.lock().await;
        println!("it     be {:?}", v);
        assert!(v.eq(&vec![3, 4, 5]));
        v.clear();
    }

    {
        let mut buf = [253u8, 254, 255];
        r.write(&mut buf).await?;
        let v = writevc.lock().await;
        println!("it     be {:?}", v);
        assert!(v.eq(&vec![255, 0, 1]));
    }
    Ok(())
}

#[tokio::test]
async fn test_counter1() -> std::io::Result<()> {
    let writev = Arc::new(Mutex::new(Vec::new()));
    let writevc = writev.clone();

    let client_tcps = MockTcpStream {
        read_data: Vec::new(),
        write_data: Vec::new(),
        write_target: Some(writev),
    };
    use crate::map::counter;

    let a = counter::Counter;
    let r = a
        .maps(
            0,
            ProxyBehavior::UNSPECIFIED,
            MapParams::new(Box::new(client_tcps)),
        )
        .await;

    if let Some(e) = r.e {
        return Err(e);
    }

    let d = r.d.unwrap();

    match d {
        crate::map::AnyData::B(mut d) => {
            if let Some(cd) = d.downcast_mut::<counter::CounterData>() {
                let mut inital_data = [1u8, 2, 3];
                r.c.unwrap().write(&mut inital_data).await?;

                let v = writevc.lock().await;

                println!("it     be {:?}", v);
                assert_eq!(v.len(), inital_data.len());

                println!(
                    "Successfully downcasted to CounterConn, {}, {}",
                    cd.ub.fetch_add(0, std::sync::atomic::Ordering::Relaxed),
                    cd.db.fetch_add(0, std::sync::atomic::Ordering::Relaxed)
                );
                Ok(())
            } else {
                panic!("failed downcasted to CounterConn, ")
            }
        }
        _ => panic!("need AsyncAnyData::B"),
    }
}

use crate::map::acc::MIterBox;
use crate::map::math::{AddDirection, Adder};
use crate::map::network::Direct;
use crate::map::{MapParams, Mapper, CID};
use crate::net::helpers::MockTcpStream;

use parking_lot::Mutex;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{MapperBox, ProxyBehavior};

#[tokio::test]
async fn test_adder_r() -> anyhow::Result<()> {
    let writev = Arc::new(Mutex::new(Vec::new()));
    let writevc = writev.clone();

    let client_tcps = MockTcpStream {
        read_data: Vec::new(),
        write_data: Vec::new(),
        write_target: Some(writev),
    };

    let mut a = Adder::default();
    a.addnum = 2;
    a.direction = AddDirection::Write;

    let r = a
        .maps(
            CID::default(),
            ProxyBehavior::UNSPECIFIED,
            MapParams::new(Box::new(client_tcps)),
        )
        .await;

    if let Some(e) = r.e {
        return Err(e);
    }

    let r = r.c;
    let mut r = r.try_unwrap_tcp()?;
    {
        let mut buf = [1u8, 2, 3];
        r.write(&mut buf).await?;
        let mut v = writevc.lock();
        println!("it     be {:?}", v);
        assert!(v.eq(&vec![3, 4, 5]));
        v.clear();
    }

    {
        let mut buf = [253u8, 254, 255];
        r.write(&mut buf).await?;
        let v = writevc.lock();
        println!("it     be {:?}", v);
        assert!(v.eq(&vec![255, 0, 1]));
    }
    Ok(())
}

#[tokio::test]
async fn test_adder_w() -> anyhow::Result<()> {
    let client_tcps = MockTcpStream {
        read_data: vec![1, 2, 3],
        write_data: Vec::new(),
        write_target: None,
    };

    let mut a = Adder::default();
    a.addnum = 2;
    a.direction = AddDirection::Read;

    let r = a
        .maps(
            CID::default(),
            ProxyBehavior::UNSPECIFIED,
            MapParams::new(Box::new(client_tcps)),
        )
        .await;

    if let Some(e) = r.e {
        return Err(e);
    }

    let r = r.c;
    let mut r = r.try_unwrap_tcp()?;
    {
        let mut buf = [0u8; 3];
        r.read(&mut buf).await?;

        println!("it     be {:?}", buf);
        assert_eq!(buf, [3u8, 4, 5]);
    }

    Ok(())
}

#[tokio::test]
async fn test_counter1() -> anyhow::Result<()> {
    let writev = Arc::new(Mutex::new(Vec::new()));
    let writevc = writev.clone();

    let client_tcps = MockTcpStream {
        read_data: Vec::new(),
        write_data: Vec::new(),
        write_target: Some(writev),
    };
    use crate::map::counter;

    let a = counter::Counter::default();
    let r = a
        .maps(
            CID::default(),
            ProxyBehavior::UNSPECIFIED,
            MapParams::new(Box::new(client_tcps)),
        )
        .await;

    if let Some(e) = r.e {
        return Err(e);
    }

    match r.d {
        crate::map::VecAnyData::Data(_) => {
            panic!("counter should return a vec instead of pure data")
        }
        crate::map::VecAnyData::Vec(mut v) => {
            if v.len() != 2 {
                panic!("counter should return 2 data, got {}", v.len())
            } else {
                let d1 = v.pop().expect("vec has 1 data ");
                let d2 = v.pop().expect("vec has 2 data ");

                match d1 {
                    crate::map::AnyData::AU64(db) => match d2 {
                        crate::map::AnyData::AU64(ub) => {
                            let mut inital_data = [1u8, 2, 3];
                            r.c.try_unwrap_tcp()?.write(&mut inital_data).await?;

                            let v = writevc.lock();

                            println!("it     be {:?}", v);
                            assert_eq!(v.len(), inital_data.len());

                            println!(
                                "Successfully downcasted to CounterConn, {}, {}",
                                ub.load(std::sync::atomic::Ordering::Relaxed),
                                db.load(std::sync::atomic::Ordering::Relaxed)
                            );
                            return Ok(());
                        }

                        _ => panic!("counter got data2 other than AtomicU64"),
                    },

                    _ => panic!("counter got data1 other than AtomicU64"),
                }
            }
        }
    }

    // match d {
    //     crate::map::AnyData::B(mut d) => {
    //         if let Some(cd) = d.downcast_mut::<counter::CounterData>() {
    //             let mut inital_data = [1u8, 2, 3];
    //             r.c.try_unwrap_tcp()?.write(&mut inital_data).await?;

    //             let v = writevc.lock();

    //             println!("it     be {:?}", v);
    //             assert_eq!(v.len(), inital_data.len());

    //             println!(
    //                 "Successfully downcasted to CounterConn, {}, {}",
    //                 cd.ub.load(std::sync::atomic::Ordering::Relaxed),
    //                 cd.db.load(std::sync::atomic::Ordering::Relaxed)
    //             );
    //             Ok(())
    //         } else {
    //             panic!("failed downcasted to CounterConn, ")
    //         }
    //     }
    //     _ => panic!("need AsyncAnyData::B"),
    // }
}

#[test]
fn test_clone_box_and_iter() {
    let a = Direct::default();
    let abase_c = a.clone();

    let a: MapperBox = Box::new(a);
    let a = Arc::new(a);

    let mut ac: MapperBox = Box::new(abase_c);
    ac.set_chain_tag("tag2");

    let ac = Arc::new(ac);

    println!("{:?} {:?}", a, ac);

    assert_ne!(ac.get_chain_tag(), a.get_chain_tag());

    let v = vec![a, ac];
    let mut m: MIterBox = Box::new(v.into_iter());
    println!("{:?}", &m);

    m.next();

    let mc2 = m.clone();
    println!("{:?}", mc2);

    assert_eq!(mc2.count(), 1);

    // cloning an iter is cheap
}

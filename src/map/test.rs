use crate::map::acc2::MIterBox;
use crate::map::math::{AddDirection, Adder};
use crate::map::network::Direct;
use crate::map::{MapParams, Mapper, CID};
use crate::net::helpers::MockTcpStream;

use parking_lot::Mutex;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{MapperBox, ProxyBehavior};

#[tokio::test]
async fn test_adder_r() -> std::io::Result<()> {
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
async fn test_adder_w() -> std::io::Result<()> {
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
async fn test_counter1() -> std::io::Result<()> {
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

    let d = r.d.expect("counter returns an extra data");

    match d {
        crate::map::AnyData::B(mut d) => {
            if let Some(cd) = d.downcast_mut::<counter::CounterData>() {
                let mut inital_data = [1u8, 2, 3];
                r.c.try_unwrap_tcp()?.write(&mut inital_data).await?;

                let v = writevc.lock();

                println!("it     be {:?}", v);
                assert_eq!(v.len(), inital_data.len());

                println!(
                    "Successfully downcasted to CounterConn, {}, {}",
                    cd.ub.load(std::sync::atomic::Ordering::Relaxed),
                    cd.db.load(std::sync::atomic::Ordering::Relaxed)
                );
                Ok(())
            } else {
                panic!("failed downcasted to CounterConn, ")
            }
        }
        _ => panic!("need AsyncAnyData::B"),
    }
}

#[test]
fn test_clone_box_and_iter() {
    let a = Direct::default();
    let a: MapperBox = Box::new(a);

    let mut ac = a.clone();
    ac.set_chain_tag("tag2");

    println!("{:?} {:?}", a, ac);

    assert_ne!(ac.get_chain_tag(), a.get_chain_tag());

    let v = vec![Arc::new(a), Arc::new(ac)];
    let mut m: MIterBox = Box::new(v.into_iter());
    println!("{:?}", &m);

    m.next();

    let mc2 = m.clone();
    println!("{:?}", mc2);

    assert_eq!(mc2.count(), 1);

    // cloning an iter is cheap
}
#[test]
fn test_miter_and_drop() {
    let a = Adder::default();

    let a: MapperBox = Box::new(a);

    let b = Adder::default();

    let v = vec![Arc::new(a), Arc::new(Box::new(b))];
    let m: MIterBox = Box::new(v.into_iter());
    println!("{:?}", m);
    assert!(m.count() == 2);
}

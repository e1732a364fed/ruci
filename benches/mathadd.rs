use std::time::Instant;

use criterion::{criterion_group, criterion_main, Criterion};
use futures::executor::block_on;
use parking_lot::Mutex;
use ruci::{
    map::{math::*, *},
    net::{helpers::MockTcpStream2, CID},
};
use tokio::io::AsyncWriteExt;

async fn test_adder_r(_l: usize) -> std::io::Result<()> {
    let mut x = VEC2.lock();
    let x = &mut *x;
    let x = unsafe { std::mem::transmute::<&mut Vec<u8>, &'static mut Vec<u8>>(x) };

    let mut x2 = VEC3.lock();
    let x2 = &mut *x2;
    let x2 = unsafe { std::mem::transmute::<&mut Vec<u8>, &'static mut Vec<u8>>(x2) };

    let client_tcps = MockTcpStream2 {
        read_data: &mut *x,
        write_data: &mut *x2,
        write_target: None,
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
        r.write(&mut VEC1.lock()).await?;
    }

    Ok(())
}
use lazy_static::lazy_static;
lazy_static! {
    pub static ref VEC1: Mutex<Vec<u8>> = {
        let mut x = Vec::with_capacity(1024);
        x.resize(1024, 1);
        Mutex::new(x)
    };
    pub static ref VEC2: Mutex<Vec<u8>> = {
        let mut x = Vec::with_capacity(1024);
        x.resize(1024, 2);
        Mutex::new(x)
    };
    pub static ref VEC3: Mutex<Vec<u8>> = {
        let mut x = Vec::with_capacity(1024);
        x.resize(1024, 3);
        Mutex::new(x)
    };
}

fn ma(c: &mut Criterion) {
    let l = 1024;
    c.bench_function("mathadd", move |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _i in 0..iters {
                let _ = block_on(test_adder_r(l));
            }
            start.elapsed()
        })
    });
}

criterion_group!(benches, ma);
criterion_main!(benches);
use std::time::Instant;

use criterion::{criterion_group, criterion_main, Criterion};
use ruci::map::tls;

fn t(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut d = rt.block_on(tls::test2::test_init(1)).unwrap();

    c.bench_function("tls 1", move |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _i in 0..iters {
                let _ = rt.block_on(tls::test2::test_write(&mut d));
            }
            start.elapsed()
        })
    });
}

fn t2(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut d = rt.block_on(tls::test2::test_init(2)).unwrap();

    c.bench_function("tls 2", move |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _i in 0..iters {
                let _ = rt.block_on(tls::test2::test_write(&mut d));
            }
            start.elapsed()
        })
    });
}

fn t3(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut d = rt.block_on(tls::test2::test_init(3)).unwrap();

    c.bench_function("tls 3", move |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _i in 0..iters {
                let _ = rt.block_on(tls::test2::test_write(&mut d));
            }
            start.elapsed()
        })
    });
}

criterion_group!(benches, t, t2, t3);
criterion_main!(benches);

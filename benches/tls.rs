use std::time::Instant;

use criterion::{criterion_group, criterion_main, Criterion};
use ruci::map::tls;

fn t(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut d = rt.block_on(tls::test2::test_init()).unwrap();

    c.bench_function("tls", move |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _i in 0..iters {
                let _ = rt.block_on(tls::test2::test_write(&mut d));
            }
            start.elapsed()
        })
    });
}

criterion_group!(benches, t);
criterion_main!(benches);

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

fn t4(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut d = rt.block_on(tls::test2::test_init(4)).unwrap();

    c.bench_function("tls 4", move |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _i in 0..iters {
                let _ = rt.block_on(tls::test2::test_write(&mut d));
            }
            start.elapsed()
        })
    });
}

fn t5(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut d = rt.block_on(tls::test2::test_init(5)).unwrap();

    c.bench_function("tls 5", move |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _i in 0..iters {
                let _ = rt.block_on(tls::test2::test_write(&mut d));
            }
            start.elapsed()
        })
    });
}

fn t6(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut d = rt.block_on(tls::test2::test_init(6)).unwrap();

    c.bench_function("tls 6", move |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _i in 0..iters {
                let _ = rt.block_on(tls::test2::test_write(&mut d));
            }
            start.elapsed()
        })
    });
}

fn t7(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut d = rt.block_on(tls::test2::test_init(7)).unwrap();

    c.bench_function("tls 7", move |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _i in 0..iters {
                let _ = rt.block_on(tls::test2::test_write(&mut d));
            }
            start.elapsed()
        })
    });
}

fn t8(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut d = rt.block_on(tls::test2::test_init(8)).unwrap();

    c.bench_function("tls 8", move |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _i in 0..iters {
                let _ = rt.block_on(tls::test2::test_write(&mut d));
            }
            start.elapsed()
        })
    });
}

fn t9(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut d = rt.block_on(tls::test2::test_init(9)).unwrap();

    c.bench_function("tls 9", move |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _i in 0..iters {
                let _ = rt.block_on(tls::test2::test_write(&mut d));
            }
            start.elapsed()
        })
    });
}

fn t10(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let mut d = rt.block_on(tls::test2::test_init(10)).unwrap();

    c.bench_function("tls 10", move |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _i in 0..iters {
                let _ = rt.block_on(tls::test2::test_write(&mut d));
            }
            start.elapsed()
        })
    });
}

criterion_group!(benches, t, t2, t3, t4, t5, t6, t7, t8, t9, t10);
criterion_main!(benches);

use criterion::{criterion_group, criterion_main, Criterion};
use rucimp::map::steganography::spe1::QaData;

fn bench_bytes_to_questions(c: &mut Criterion) {
    let qa = QaData::new_simple();
    let data = vec![0u8; 1024];

    c.bench_function("bytes_to_questions", |b| {
        b.iter(|| qa.bytes_to_questions_text(&data))
    });
}

fn bench_questions_to_bytes(c: &mut Criterion) {
    let qa = QaData::new_simple();
    let questions = qa.bytes_to_questions_text(&vec![0u8; 1024]);

    c.bench_function("questions_to_bytes", |b| {
        b.iter(|| qa.questions_to_bytes(&questions, true, true))
    });
}

criterion_group!(
    spe1_benches,
    bench_bytes_to_questions,
    bench_questions_to_bytes
);
criterion_main!(spe1_benches);

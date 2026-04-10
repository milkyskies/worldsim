//! Benchmarks for MindGraph query / mutation hot paths (#197).
//!
//! Run with: `cargo bench --bench mind_graph`.

use bevy::prelude::Entity;
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

use worldsim::agent::mind::knowledge::{Concept, MindGraph, Node, Predicate, Triple, Value};

/// Build a MindGraph with `n` triples spread across ~n/3 entities. Each entity
/// owns Contains, LocatedAt, IsA so every query pattern has data to walk.
fn populated_graph(n: usize) -> MindGraph {
    let mut mind = MindGraph::default();
    let entity_count = (n / 3).max(1);
    for i in 0..entity_count {
        let entity = Entity::from_bits(1000 + i as u64);
        mind.add(Triple::new(
            Node::Entity(entity),
            Predicate::Contains,
            Value::Item(Concept::Apple, (i % 10) as u32),
        ));
        mind.add(Triple::new(
            Node::Entity(entity),
            Predicate::LocatedAt,
            Value::Tile((i as i32, (i * 3) as i32)),
        ));
        mind.add(Triple::new(
            Node::Entity(entity),
            Predicate::IsA,
            Value::Concept(Concept::AppleTree),
        ));
    }
    mind
}

fn bench_query_by_subject(c: &mut Criterion) {
    let mut group = c.benchmark_group("mind_graph/query_by_subject");
    for size in [100, 500, 1000] {
        let mind = populated_graph(size);
        // Pick a subject that exists in the middle of the store so linear scan
        // and index paths are both exercised fairly.
        let target = Node::Entity(Entity::from_bits(1000 + (size / 6) as u64));
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                let results = mind.query(black_box(Some(&target)), None, None);
                black_box(results);
            });
        });
    }
    group.finish();
}

fn bench_query_by_subject_predicate(c: &mut Criterion) {
    let mut group = c.benchmark_group("mind_graph/query_by_subject_predicate");
    for size in [100, 500, 1000] {
        let mind = populated_graph(size);
        let target = Node::Entity(Entity::from_bits(1000 + (size / 6) as u64));
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                let results = mind.query(
                    black_box(Some(&target)),
                    black_box(Some(Predicate::Contains)),
                    None,
                );
                black_box(results);
            });
        });
    }
    group.finish();
}

fn bench_query_by_predicate(c: &mut Criterion) {
    let mut group = c.benchmark_group("mind_graph/query_by_predicate");
    for size in [100, 500, 1000] {
        let mind = populated_graph(size);
        group.throughput(Throughput::Elements(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                let results = mind.query(None, black_box(Some(Predicate::LocatedAt)), None);
                black_box(results);
            });
        });
    }
    group.finish();
}

fn bench_assert(c: &mut Criterion) {
    let mut group = c.benchmark_group("mind_graph/assert");
    for size in [100, 500, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter_batched(
                || populated_graph(size),
                |mut mind| {
                    // Non-functional assert into a populated graph.
                    mind.assert(Triple::new(
                        Node::Entity(Entity::from_bits(9_999_999)),
                        Predicate::IsA,
                        Value::Concept(Concept::Food),
                    ));
                    black_box(mind);
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

fn bench_remove(c: &mut Criterion) {
    let mut group = c.benchmark_group("mind_graph/remove");
    for size in [100, 500, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter_batched(
                || {
                    let mind = populated_graph(size);
                    let entity_count = (size / 3).max(1);
                    let pick = entity_count / 2;
                    let target = Entity::from_bits(1000 + pick as u64);
                    (mind, target, pick as i32)
                },
                |(mut mind, target, pick)| {
                    mind.remove(
                        &Node::Entity(target),
                        Predicate::LocatedAt,
                        &Value::Tile((pick, pick * 3)),
                    );
                    black_box(mind);
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_query_by_subject,
    bench_query_by_subject_predicate,
    bench_query_by_predicate,
    bench_assert,
    bench_remove,
);
criterion_main!(benches);

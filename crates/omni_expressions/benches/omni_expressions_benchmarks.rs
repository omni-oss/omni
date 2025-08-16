use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use omni_expressions::Context;
use std::{collections::HashMap, hint::black_box};

#[inline(always)]
fn parse(
    expr: &str,
) -> Result<omni_expressions::Evaluator, omni_expressions::Error> {
    omni_expressions::parse(expr)
}

#[inline(always)]
fn evaluate(
    evaluator: &omni_expressions::Evaluator,
    ctx: &Context<'_>,
) -> bool {
    evaluator.coerce_to_bool(ctx).expect("should have value")
}

fn simple_expression(n: usize) -> String {
    let mut exprs = Vec::with_capacity(n);

    exprs.extend(std::iter::repeat_n("1 + 1 == 2", n));

    exprs.join("&&")
}

fn simple_context() -> Context<'static> {
    Context::default()
}

fn variables_expression(n: usize) -> String {
    let mut exprs = Vec::with_capacity(n);
    for i in 0..n {
        exprs.push(format!("a_{i} == {i} && b_{i} == \"test{i}\""));
    }

    exprs.join("&&")
}

fn variables_context(n: usize) -> Context<'static> {
    let mut ctx = Context::default();

    for i in 0..n {
        ctx.add_variable(format!("a_{i}"), i)
            .expect("Failed to add variable");
        ctx.add_variable(format!("b_{i}"), format!("test{i}"))
            .expect("Failed to add variable");
    }

    ctx
}

fn functions_context(n: usize) -> Context<'static> {
    let mut ctx = Context::default();

    for i in 0..n {
        ctx.add_variable(format!("a_{i}"), format!("test{i}"))
            .expect("Failed to add variable");
        ctx.add_variable(format!("b_{i}"), [1, 2, 3])
            .expect("Failed to add variable");
        let m = HashMap::from([("a", 1), ("b", 2), ("c", 3)]);
        ctx.add_variable(format!("c_{i}"), m)
            .expect("Failed to add variable");
    }

    ctx
}

fn functions_expression(n: usize) -> String {
    let mut exprs = Vec::with_capacity(n);
    for i in 0..n {
        exprs.push(format!(
                "a_{i}.startsWith(\"test\") && b_{i}.contains(1) && size(b_{i}) == 3 && size(c_{i}) == 3",
        ));
    }

    exprs.join("&&")
}

const DATA_SIZES: [usize; 6] = [1, 10, 100, 1000, 5000, 10000];

fn parse_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");

    for n in DATA_SIZES {
        group.bench_with_input(
            BenchmarkId::new("simple", n),
            &(&simple_expression(n),),
            |b, (n,)| {
                b.iter(|| parse(black_box(n)).expect("should have evaluator"))
            },
        );

        group.bench_with_input(
            BenchmarkId::new("variables", n),
            &(&variables_expression(n),),
            |b, (n,)| {
                b.iter(|| parse(black_box(n)).expect("should have evaluator"))
            },
        );

        group.bench_with_input(
            BenchmarkId::new("functions", n),
            &(&functions_expression(n),),
            |b, (n,)| {
                b.iter(|| parse(black_box(n)).expect("should have evaluator"))
            },
        );
    }

    group.finish();
}

fn evaluate_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("evaluate");

    for n in DATA_SIZES {
        let simple =
            parse(&simple_expression(n)).expect("should have evaluator");

        group.bench_with_input(
            BenchmarkId::new("simple", n),
            &(simple, simple_context()),
            |b, (evaluator, ctx)| {
                b.iter(|| evaluate(black_box(evaluator), black_box(ctx)))
            },
        );

        let variables =
            parse(&variables_expression(n)).expect("should have evaluator");

        group.bench_with_input(
            BenchmarkId::new("variables", n),
            &(variables, variables_context(n)),
            |b, (evaluator, ctx)| {
                b.iter(|| evaluate(black_box(evaluator), black_box(ctx)))
            },
        );

        let functions =
            parse(&functions_expression(n)).expect("should have evaluator");

        group.bench_with_input(
            BenchmarkId::new("functions", n),
            &(functions, functions_context(n)),
            |b, (evaluator, ctx)| {
                b.iter(|| evaluate(black_box(evaluator), black_box(ctx)))
            },
        );
    }

    group.finish();
}

fn criterion_config() -> Criterion {
    Criterion::default()
}

criterion_group!(
    name = evaluate_bench;
    config = criterion_config();
    targets = evaluate_benchmark
);

criterion_group!(
    name = parse_bench;
    config = criterion_config();
    targets = parse_benchmark
);

criterion_main!(parse_bench, evaluate_bench);

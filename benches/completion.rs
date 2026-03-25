use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::path::Path;
use tabra::engine::{matcher, parser, resolver};
use tabra::spec::loader::SpecIndex;
use tabra::spec::types::FilterStrategy;

fn load_git_spec() -> SpecIndex {
    let specs_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("specs");
    let mut index = SpecIndex::new();
    let git_path = specs_dir.join("git.json");
    let content = std::fs::read_to_string(&git_path).expect("git.json not found in specs/");
    let spec: tabra::spec::types::Spec =
        serde_json::from_str(&content).expect("failed to parse git.json");
    index.insert("git".to_string(), spec);
    index
}

fn bench_tokenize(c: &mut Criterion) {
    c.bench_function("tokenize_simple", |b| {
        b.iter(|| parser::tokenize(black_box("git commit -m 'hello world'"), 27))
    });

    c.bench_function("tokenize_long", |b| {
        b.iter(|| {
            parser::tokenize(
                black_box("docker run --rm -it --name mycontainer -v /host:/container -p 8080:80 -e FOO=bar ubuntu:latest bash -c 'echo hello'"),
                110,
            )
        })
    });
}

fn bench_parse(c: &mut Criterion) {
    let index = load_git_spec();
    let spec = index.get("git").unwrap();

    c.bench_function("parse_git_commit", |b| {
        b.iter(|| parser::parse(black_box(spec), "git commit -m 'msg'", 19))
    });

    c.bench_function("parse_git_checkout", |b| {
        b.iter(|| parser::parse(black_box(spec), "git checkout ", 13))
    });

    c.bench_function("parse_git_empty", |b| {
        b.iter(|| parser::parse(black_box(spec), "git ", 4))
    });
}

fn bench_resolve(c: &mut Criterion) {
    let index = load_git_spec();
    let spec = index.get("git").unwrap();

    c.bench_function("resolve_git_subcommands", |b| {
        let ctx = parser::parse(spec, "git ", 4);
        b.iter(|| resolver::resolve(black_box(spec), black_box(&ctx), "/tmp"))
    });

    c.bench_function("resolve_git_commit_options", |b| {
        let ctx = parser::parse(spec, "git commit --", 13);
        b.iter(|| resolver::resolve(black_box(spec), black_box(&ctx), "/tmp"))
    });
}

fn bench_match(c: &mut Criterion) {
    let index = load_git_spec();
    let spec = index.get("git").unwrap();

    // Collect all subcommand suggestions
    let ctx = parser::parse(spec, "git ", 4);
    let candidates = resolver::resolve(spec, &ctx, "/tmp");

    c.bench_function("match_empty_query", |b| {
        b.iter(|| {
            matcher::match_suggestions(
                black_box(""),
                black_box(&candidates),
                FilterStrategy::Prefix,
            )
        })
    });

    c.bench_function("match_short_query_ch", |b| {
        b.iter(|| {
            matcher::match_suggestions(
                black_box("ch"),
                black_box(&candidates),
                FilterStrategy::Fuzzy,
            )
        })
    });

    c.bench_function("match_medium_query_check", |b| {
        b.iter(|| {
            matcher::match_suggestions(
                black_box("check"),
                black_box(&candidates),
                FilterStrategy::Fuzzy,
            )
        })
    });
}

fn bench_end_to_end(c: &mut Criterion) {
    let index = load_git_spec();
    let spec = index.get("git").unwrap();

    c.bench_function("e2e_git_space", |b| {
        b.iter(|| {
            let ctx = parser::parse(spec, "git ", 4);
            let candidates = resolver::resolve(spec, &ctx, "/tmp");
            matcher::match_suggestions("", &candidates, ctx.filter_strategy)
        })
    });

    c.bench_function("e2e_git_ch", |b| {
        b.iter(|| {
            let ctx = parser::parse(spec, "git ch", 6);
            let candidates = resolver::resolve(spec, &ctx, "/tmp");
            matcher::match_suggestions("ch", &candidates, ctx.filter_strategy)
        })
    });

    c.bench_function("e2e_git_commit_dash", |b| {
        b.iter(|| {
            let ctx = parser::parse(spec, "git commit --", 13);
            let candidates = resolver::resolve(spec, &ctx, "/tmp");
            matcher::match_suggestions("--", &candidates, ctx.filter_strategy)
        })
    });
}

criterion_group!(
    benches,
    bench_tokenize,
    bench_parse,
    bench_resolve,
    bench_match,
    bench_end_to_end
);
criterion_main!(benches);

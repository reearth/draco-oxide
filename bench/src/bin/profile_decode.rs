//! Sampling profiler for the draco-oxide decoder: encodes a mesh once, decodes
//! it in a loop under pprof, and prints the hottest functions plus a flamegraph.
//!
//! Usage: profile-decode <mesh.obj> [seconds] [flamegraph.svg]

use std::collections::HashMap;
use std::time::{Duration, Instant};

use draco_oxide::core::types::ConfigType;
use draco_oxide::encode::{encode, Config};
use draco_oxide::io::obj::load_obj;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .expect("usage: profile-decode <mesh.obj> [seconds] [flamegraph.svg]");
    let seconds: f64 = args.next().map(|s| s.parse().unwrap()).unwrap_or(5.0);
    let svg_path = args.next();

    let mesh = load_obj(&path).expect("load obj");
    let mut buffer = Vec::new();
    encode(mesh, &mut buffer, Config::default()).expect("encode");
    eprintln!(
        "encoded {} bytes; decoding for {seconds}s ...",
        buffer.len()
    );

    let guard = pprof::ProfilerGuardBuilder::default()
        .frequency(997)
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()
        .expect("start profiler");

    let start = Instant::now();
    let mut iters = 0u64;
    while start.elapsed() < Duration::from_secs_f64(seconds) {
        let reader = draco_oxide::core::bit_coder::SliceReader::new(&buffer);
        let decoded = draco_oxide::decode::decode(reader).expect("decode");
        std::hint::black_box(&decoded);
        iters += 1;
    }
    let elapsed = start.elapsed();

    let report = guard.report().build().expect("build report");
    eprintln!(
        "{iters} decodes in {:.2}s ({:.2} ms/decode)",
        elapsed.as_secs_f64(),
        elapsed.as_secs_f64() * 1e3 / iters as f64
    );

    // Fold samples into per-function self and inclusive counts. A stack's leaf
    // symbol gets the self count; every distinct symbol on the stack gets the
    // inclusive count once.
    let mut self_counts: HashMap<String, isize> = HashMap::new();
    let mut incl_counts: HashMap<String, isize> = HashMap::new();
    let mut total: isize = 0;
    for (frames, count) in report.data.iter() {
        total += count;
        let names: Vec<String> = frames
            .frames
            .iter()
            .flat_map(|frame| frame.iter().map(|s| s.name()))
            .collect();
        if let Some(leaf) = names.first() {
            *self_counts.entry(leaf.clone()).or_default() += count;
        }
        let mut seen = std::collections::HashSet::new();
        for name in &names {
            if seen.insert(name.clone()) {
                *incl_counts.entry(name.clone()).or_default() += count;
            }
        }
    }

    let mut rows: Vec<(&String, isize)> = self_counts.iter().map(|(k, &v)| (k, v)).collect();
    rows.sort_by_key(|&(_, v)| std::cmp::Reverse(v));
    println!("\n== top self time (total {total} samples) ==");
    for (name, count) in rows.iter().take(30) {
        let incl = incl_counts.get(*name).copied().unwrap_or(0);
        println!(
            "{:6.2}% self {:6.2}% incl  {}",
            *count as f64 * 100.0 / total as f64,
            incl as f64 * 100.0 / total as f64,
            name
        );
    }

    let mut incl_rows: Vec<(&String, isize)> = incl_counts.iter().map(|(k, &v)| (k, v)).collect();
    incl_rows.sort_by_key(|&(_, v)| std::cmp::Reverse(v));
    println!("\n== top inclusive time ==");
    for (name, count) in incl_rows.iter().take(30) {
        println!(
            "{:6.2}% incl  {}",
            *count as f64 * 100.0 / total as f64,
            name
        );
    }

    if let Some(svg) = svg_path {
        let file = std::fs::File::create(&svg).expect("create svg");
        report.flamegraph(file).expect("write flamegraph");
        eprintln!("flamegraph written to {svg}");
    }
}

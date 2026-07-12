//! Candidate quality and latency benchmark for the seed engine.
//!
//! Run with `cargo test -p novatype-core --test benchmark -- --nocapture`.

use novatype_core::Engine;
use std::time::Instant;

/// (pinyin input, expected top-1 candidate)
const TOP1_CASES: &[(&str, &str)] = &[
    ("nihao", "你好"),
    ("women", "我们"),
    ("zhongguo", "中国"),
    ("zhongguoren", "中国人"),
    ("shurufa", "输入法"),
    ("xuexi", "学习"),
    ("zhineng", "智能"),
    ("xinxing", "新星"),
];

#[test]
fn top1_accuracy_is_full_on_seed_lexicon() {
    let engine = Engine::new();
    let mut failures = Vec::new();

    for (input, expected) in TOP1_CASES {
        let candidates = engine.suggest(input, 5);
        let top1 = candidates.first().map(|candidate| candidate.text.clone());
        if top1.as_deref() != Some(*expected) {
            failures.push(format!("{input}: expected {expected}, got {top1:?}"));
        }
    }

    assert!(failures.is_empty(), "top-1 failures: {failures:?}");
}

#[test]
fn latency_stays_under_budget() {
    let engine = Engine::new();

    // Warm up.
    for (input, _) in TOP1_CASES {
        let _ = engine.suggest(input, 9);
    }

    let started = Instant::now();
    let rounds = 100;
    for _ in 0..rounds {
        for (input, _) in TOP1_CASES {
            let _ = engine.suggest(input, 9);
        }
    }
    let elapsed = started.elapsed();
    let per_query = elapsed / (rounds * u32::try_from(TOP1_CASES.len()).expect("case count"));

    println!("average suggest latency: {per_query:?}");
    assert!(
        per_query.as_millis() < 5,
        "per-query latency {per_query:?} exceeds 5ms budget"
    );
}

use std::{hint::black_box, time::Instant};

use aura_context_fixtures::p06_performance_preparation_input;
use aura_context_runtime::{
    ContextEstimatorRegistry, ContextPreparationCompilationOutcome, ContextPreparationCompiler,
    HistoryEventKind, HistoryEventUnit, plan_recent_history,
};
use aura_domain_common::{EventId, SessionSequence};
use serde_json::{Value, json};

const COMPILER_ITERATIONS: usize = 9;
const PLANNER_ITERATIONS: usize = 101;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let compile_256k = measure_context_compile(262_144, 180_000)?;
    let compile_1m = measure_context_compile(1_048_576, 720_000)?;
    let planner = measure_compaction_planner()?;
    let report = json!({
        "schema_version": 1,
        "profile": "release",
        "provider_requests": 0,
        "measurements": [
            measurement_json(
                "context_compile_256k",
                COMPILER_ITERATIONS,
                300.0,
                compile_256k,
                json!({
                    "runtime_effective_window_tokens": 262_144,
                    "current_user_utf8_bytes": 180_000,
                    "estimator": "utf8_byte_upper_bound_v1"
                }),
            ),
            measurement_json(
                "context_compile_1m",
                COMPILER_ITERATIONS,
                1_000.0,
                compile_1m,
                json!({
                    "runtime_effective_window_tokens": 1_048_576,
                    "current_user_utf8_bytes": 720_000,
                    "estimator": "utf8_byte_upper_bound_v1"
                }),
            ),
            measurement_json(
                "compaction_plan_2000_events",
                PLANNER_ITERATIONS,
                200.0,
                planner,
                json!({"history_events": 2_000, "event_tokens": 400}),
            )
        ]
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn measure_context_compile(
    window_tokens: u64,
    current_user_utf8_bytes: usize,
) -> Result<Vec<f64>, Box<dyn std::error::Error>> {
    let fixture = p06_performance_preparation_input(window_tokens, current_user_utf8_bytes)?;
    let estimators = ContextEstimatorRegistry::default().counting_unmeasured_by_bytes();
    let compiler = ContextPreparationCompiler::new(&estimators);
    let warmup = compiler.compile(fixture.clone())?;
    ensure_prepared(&warmup)?;

    let mut samples = Vec::with_capacity(COMPILER_ITERATIONS);
    for _ in 0..COMPILER_ITERATIONS {
        let input = fixture.clone();
        let started = Instant::now();
        let outcome = compiler.compile(input)?;
        let elapsed = started.elapsed();
        ensure_prepared(&outcome)?;
        black_box(outcome);
        samples.push(elapsed.as_secs_f64() * 1_000.0);
    }
    Ok(samples)
}

fn measure_compaction_planner() -> Result<Vec<f64>, Box<dyn std::error::Error>> {
    let events = (1_u64..=2_000)
        .map(|sequence| HistoryEventUnit {
            event_id: EventId::new(),
            sequence: SessionSequence::new(sequence),
            turn_segment_id: format!("segment-{sequence}"),
            history_atomic_group_ref: None,
            atomic_group_id: None,
            kind: HistoryEventKind::AssistantMessage,
            estimated_tokens: 400,
            sealed: true,
        })
        .collect::<Vec<_>>();
    let warmup = plan_recent_history(1_000_000, 850_000, 200_000, &events)?;
    if warmup.compact_event_ids.is_empty() || warmup.retained_event_ids.is_empty() {
        return Err("planner performance fixture did not exercise both partitions".into());
    }

    let mut samples = Vec::with_capacity(PLANNER_ITERATIONS);
    for _ in 0..PLANNER_ITERATIONS {
        let started = Instant::now();
        let plan = plan_recent_history(1_000_000, 850_000, 200_000, &events)?;
        let elapsed = started.elapsed();
        black_box(plan);
        samples.push(elapsed.as_secs_f64() * 1_000.0);
    }
    Ok(samples)
}

fn ensure_prepared(
    outcome: &ContextPreparationCompilationOutcome,
) -> Result<(), Box<dyn std::error::Error>> {
    if matches!(outcome, ContextPreparationCompilationOutcome::Prepared(_)) {
        Ok(())
    } else {
        Err("performance Context input did not produce a prepared snapshot".into())
    }
}

fn measurement_json(
    name: &str,
    iterations: usize,
    target_p95_ms: f64,
    mut samples: Vec<f64>,
    fixture: Value,
) -> Value {
    samples.sort_by(f64::total_cmp);
    let p95_index = ((samples.len() * 95).div_ceil(100)).saturating_sub(1);
    let median = samples[samples.len() / 2];
    let p95 = samples[p95_index];
    json!({
        "name": name,
        "iterations": iterations,
        "target_p95_ms": target_p95_ms,
        "min_ms": round_millis(samples[0]),
        "median_ms": round_millis(median),
        "p95_ms": round_millis(p95),
        "max_ms": round_millis(samples[samples.len() - 1]),
        "passed": p95 <= target_p95_ms,
        "fixture": fixture
    })
}

fn round_millis(value: f64) -> f64 {
    (value * 1_000.0).round() / 1_000.0
}

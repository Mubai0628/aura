use super::support::*;
use super::*;
use std::time::{Duration, Instant};

// This includes policy, resource queueing, Attempt/Result persistence and the tool
// return. It excludes Provider inference and UI polling. Electron checks measure
// a fresh Core process separately; this fixture shares the test process's runtime cache.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s08_real_core_shell_return_times_include_authority_and_durable_results() {
    let started = Instant::now();
    let mut f = Fixture::with_exposure(
        PermissionMode::WorkspaceReadWrite,
        &["shell.cmd", "shell.powershell"],
    )
    .await;
    tokio::time::sleep(Duration::from_secs(10).saturating_sub(started.elapsed())).await;
    f.service()
        .commands_required()
        .wait_until_ready()
        .await
        .expect("background runtime verification is complete");
    let mut report = Vec::new();
    for tool in ["shell.powershell", "shell.cmd"] {
        let mut samples = Vec::new();
        for ordinal in 0..21 {
            let batch = f.admit(&[(tool, arguments("echo core-timing", false))]);
            let id = batch.batch.calls[0].tool_call_id;
            let batch_id = batch.batch.tool_batch_id;
            let start = Instant::now();
            let group = f.execute(batch).await;
            let millis = start.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(f.output(id)["exit_code"], 0, "real process and durable tool result");
            samples.push(millis);
            f.continue_after_tools(batch_id, &group);
            if ordinal == 0 {
                println!("S08_CORE_FIRST_AFTER_10_SECONDS {tool} {millis:.3}ms");
            }
        }
        let first = samples.remove(0);
        samples.sort_by(f64::total_cmp);
        let p50 = samples[9];
        let p95 = samples[18];
        let max = samples[19];
        report.push(json!({"tool":tool,"first_after_10_seconds_ms":first,"warm_samples_ms":samples,"p50_ms":p50,"p95_ms":p95,"max_ms":max}));
        assert!(
            p50 <= 1500.0 && p95 <= 3000.0,
            "actual Core return performance for {tool}: p50={p50:.3}ms p95={p95:.3}ms max={max:.3}ms"
        );
    }
    println!(
        "S08_CORE_TIMING {}",
        serde_json::to_string(&report).expect("public timing facts only")
    );
}

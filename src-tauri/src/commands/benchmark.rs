//! "Test this machine": times each engine on the CPU and the GPU and reports
//! which is faster, so the Engine tab can recommend settings. The measuring is
//! in `voxctrl_inference::bench`; this is the command surface around it —
//! the plan, a run that reports progress, and cancelling it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use voxctrl_inference::bench::{self, BenchResult, BenchStep, Skipped};

use crate::state::AppState;

/// Set by `cancel_benchmark`, read between phases of a run.
static CANCEL: AtomicBool = AtomicBool::new(false);
/// Held for the length of a run so two cannot overlap — they would time each
/// other.
static RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Serialize)]
pub struct BenchmarkPlan {
    pub steps: Vec<BenchStep>,
    /// Engines left out, with the reason.
    pub skipped: Vec<Skipped>,
    /// Sum of the steps' estimates, in seconds.
    pub est_secs: u32,
}

#[tauri::command]
pub async fn benchmark_plan(state: State<'_, Arc<AppState>>) -> Result<BenchmarkPlan, String> {
    let cfg = state.config.lock().await.data.clone();
    let (steps, skipped) = tauri::async_runtime::spawn_blocking(move || bench::plan_with_skips(&cfg))
        .await
        .map_err(|e| e.to_string())?;
    let est_secs = steps.iter().map(|s| s.est_secs).sum();
    Ok(BenchmarkPlan { steps, skipped, est_secs })
}

/// Sent as each phase of each step begins, and once more when a step ends.
#[derive(Clone, Serialize)]
struct Progress {
    /// Zero-based step being measured.
    step: usize,
    total_steps: usize,
    label: String,
    /// What the step is doing right now.
    phase: &'static str,
    /// 0.0–1.0 over the whole run.
    fraction: f64,
}

#[derive(Serialize)]
pub struct BenchmarkOutcome {
    pub results: Vec<BenchResult>,
    /// `(engine, "cpu" | "gpu")` for each engine with both halves measured.
    pub recommended: Vec<(String, String)>,
    pub cancelled: bool,
}

const PHASE_NAMES: [&str; bench::PHASES_PER_STEP] = [
    "Loading the model",
    "Warming up",
    "Timing run 1 of 3",
    "Timing run 2 of 3",
    "Timing run 3 of 3",
];

#[tauri::command]
pub async fn run_benchmark(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<BenchmarkOutcome, String> {
    if state.is_recording() || state.is_processing() {
        return Err("Finish the current dictation before running the benchmark.".into());
    }
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("A benchmark is already running.".into());
    }
    CANCEL.store(false, Ordering::SeqCst);
    let cfg = state.config.lock().await.data.clone();

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let steps = bench::plan(&cfg);
        let total = steps.len();
        let mut results = Vec::with_capacity(total);
        let mut cancelled = false;
        for (i, step) in steps.iter().enumerate() {
            if CANCEL.load(Ordering::SeqCst) {
                cancelled = true;
                break;
            }
            let mut report = |phase: usize| {
                let fraction =
                    (i as f64 + phase as f64 / bench::PHASES_PER_STEP as f64) / total as f64;
                let _ = app.emit(
                    "benchmark-progress",
                    Progress {
                        step: i,
                        total_steps: total,
                        label: step.label.clone(),
                        phase: PHASE_NAMES[phase.min(PHASE_NAMES.len() - 1)],
                        fraction,
                    },
                );
            };
            results.push(bench::run_step(step, &cfg, &mut report));
        }
        let _ = app.emit(
            "benchmark-progress",
            Progress {
                step: total,
                total_steps: total,
                label: String::new(),
                phase: "Done",
                fraction: if cancelled { 0.0 } else { 1.0 },
            },
        );
        let recommended = bench::recommend(&results)
            .into_iter()
            .map(|(e, d)| (e.to_string(), d.to_string()))
            .collect();
        BenchmarkOutcome { results, recommended, cancelled }
    })
    .await;

    RUNNING.store(false, Ordering::SeqCst);
    outcome.map_err(|e| e.to_string())
}

/// Stops the run at the next step boundary; the step in flight finishes first,
/// since a model load cannot be interrupted part-way.
#[tauri::command]
pub fn cancel_benchmark() {
    CANCEL.store(true, Ordering::SeqCst);
}

//! Measures which device — CPU or GPU — answers fastest on this machine, per
//! engine, so the Engine tab can recommend settings instead of guessing.
//!
//! The answer is not predictable from the hardware: with the same RTX 4090,
//! S1-mini ran ~6× faster on the GPU while Moonshine and Parakeet ran 5–7×
//! slower (their ONNX graphs are split between GPU and CPU and bounce data
//! across the bus on every decoding step). So it is measured, not assumed.
//!
//! A run is a list of [`BenchStep`]s — one per engine per device — each timed
//! the same way: load, one untimed warm-up (which pays the GPU's one-off shader
//! compilation, so it is not charged to the device), then the median of three
//! timed runs on a short bundled speech clip.

use std::time::Instant;

use anyhow::{bail, Context, Result};
use serde::Serialize;
use voxctrl_config::AppConfig;

use crate::backend::{TranscribeRequest, TranscriptionBackend};

/// A few seconds of synthetic speech, 16 kHz mono 16-bit. Long enough for the
/// per-step costs that decide the winner to show up, short enough to keep a
/// full run to about a minute.
const CLIP_WAV: &[u8] = include_bytes!("../assets/bench_speech.wav");

/// What S1-mini is asked to clean.
const CLEANUP_TEXT: &str =
    "um so uh please send the report by friday afternoon and uh let me know if the numbers look right";

/// Timed runs per step, after the warm-up. The median of an odd count.
const TIMED_RUNS: usize = 3;

/// Sub-steps each step reports progress through: load, warm-up, each timed run.
pub const PHASES_PER_STEP: usize = 2 + TIMED_RUNS;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BenchStep {
    /// `"whisper"`, `"moonshine"`, `"parakeet"` or `"s1_mini"`.
    pub engine: &'static str,
    /// `"cpu"` or `"gpu"`.
    pub device: &'static str,
    /// The model measured, e.g. `base` — the one the user has configured.
    pub model: String,
    /// What the UI shows while the step runs.
    pub label: String,
    /// Rough seconds the step takes, for the up-front "about N seconds".
    pub est_secs: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct BenchResult {
    pub engine: &'static str,
    pub device: &'static str,
    pub model: String,
    pub load_ms: Option<f64>,
    /// Median of the timed runs, in milliseconds.
    pub median_ms: Option<f64>,
    pub error: Option<String>,
}

fn engine_title(engine: &str) -> &'static str {
    match engine {
        "whisper" => "Whisper.cpp",
        "moonshine" => "Moonshine",
        "parakeet" => "Parakeet",
        _ => "S1-mini",
    }
}

/// Rough seconds for one step, from runs on a desktop with an RTX 4090 (a cold
/// start with no shader cache, which is the slow case). Only the up-front
/// estimate uses it; the UI re-derives the time left from the pace of the steps
/// already done.
fn estimate_secs(engine: &str, device: &str, model: &str) -> u32 {
    let gpu = device == "gpu";
    match engine {
        "whisper" => {
            let cpu = match model {
                m if m.starts_with("tiny") => 2,
                m if m.starts_with("base") => 3,
                m if m.starts_with("small") => 9,
                m if m.starts_with("medium") => 30,
                _ => 60,
            };
            // The GPU time is mostly moving the model into VRAM.
            let load = match model {
                m if m.starts_with("tiny") || m.starts_with("base") => 3,
                m if m.starts_with("small") => 4,
                m if m.starts_with("medium") => 7,
                _ => 12,
            };
            if gpu { load } else { cpu }
        }
        "moonshine" => if gpu { 5 } else { 2 },
        "parakeet" => if gpu { 8 } else { 3 },
        _ => if gpu { 3 } else { 2 },
    }
}

/// An engine the run leaves out, and why — shown to the user so a missing
/// engine reads as "download its model" rather than as a bug.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Skipped {
    pub engine: &'static str,
    pub reason: String,
}

/// The steps a run would take on this build with this configuration, and the
/// engines it leaves out.
///
/// An engine is measured only when there is a choice to make — the build has a
/// GPU path for it — and the model the user has configured is on disk. It gets
/// both devices so they are timed back to back.
pub fn plan_with_skips(cfg: &AppConfig) -> (Vec<BenchStep>, Vec<Skipped>) {
    let mut steps = Vec::new();
    let mut skipped = Vec::new();
    let mut add = |engine: &'static str, model: String| {
        for device in ["cpu", "gpu"] {
            steps.push(BenchStep {
                engine,
                device,
                est_secs: estimate_secs(engine, device, &model),
                label: format!(
                    "{} ({}) on the {}",
                    engine_title(engine),
                    model,
                    if device == "gpu" { "GPU" } else { "CPU" }
                ),
                model: model.clone(),
            });
        }
    };
    let mut skip = |engine: &'static str, reason: String| skipped.push(Skipped { engine, reason });

    let w = &cfg.engine.whisper_cpp;
    if crate::whisper_gpu_backend().is_none() {
        skip("whisper", "this build has no GPU path for it".into());
    } else if !crate::whisper_cpp::is_model_downloaded(&w.model_size, &w.model_dir) {
        skip("whisper", format!("the selected model ({}) is not downloaded", w.model_size));
    } else {
        add("whisper", w.model_size.clone());
    }

    #[cfg(feature = "moonshine")]
    {
        let size = &cfg.engine.moonshine.model_size;
        if crate::moonshine_gpu_backend().is_none() {
            skip("moonshine", "this build has no GPU path for it".into());
        } else if !crate::moonshine::is_model_downloaded(size, "") {
            skip("moonshine", format!("the selected model ({size}) is not downloaded"));
        } else {
            add("moonshine", size.clone());
        }
    }
    #[cfg(not(feature = "moonshine"))]
    skip("moonshine", "this build does not include it".into());

    #[cfg(feature = "parakeet")]
    {
        let size = &cfg.engine.parakeet.model_size;
        if crate::parakeet_gpu_backend().is_none() {
            skip("parakeet", "this build has no GPU path for it".into());
        } else if !crate::parakeet::is_model_downloaded(size, "") {
            skip("parakeet", format!("the selected model ({size}) is not downloaded"));
        } else {
            add("parakeet", size.clone());
        }
    }
    #[cfg(not(feature = "parakeet"))]
    skip("parakeet", "this build does not include it".into());

    if crate::s1_mini::sidecar_gpu_backend().is_none() {
        skip("s1_mini", "this build has no GPU path for it".into());
    } else if !crate::s1_mini::is_s1_mini_downloaded(None) {
        skip("s1_mini", "its model is not downloaded".into());
    } else {
        add("s1_mini", "q4_k_m".into());
    }
    (steps, skipped)
}

/// The steps alone, for the run itself.
pub fn plan(cfg: &AppConfig) -> Vec<BenchStep> {
    plan_with_skips(cfg).0
}

/// Decode the bundled clip to f32 samples.
fn clip_samples() -> Result<Vec<f32>> {
    let wav = CLIP_WAV;
    if wav.len() < 12 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        bail!("bundled benchmark clip is not a WAV file");
    }
    let mut pos = 12;
    let mut ok_format = false;
    while pos + 8 <= wav.len() {
        let id = &wav[pos..pos + 4];
        let len = u32::from_le_bytes(wav[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = pos + 8;
        if id == b"fmt " {
            let channels = u16::from_le_bytes(wav[body + 2..body + 4].try_into().unwrap());
            let rate = u32::from_le_bytes(wav[body + 4..body + 8].try_into().unwrap());
            let bits = u16::from_le_bytes(wav[body + 14..body + 16].try_into().unwrap());
            ok_format = channels == 1 && rate == 16_000 && bits == 16;
        } else if id == b"data" {
            if !ok_format {
                bail!("bundled benchmark clip must be 16 kHz mono 16-bit");
            }
            let end = (body + len).min(wav.len());
            return Ok(wav[body..end]
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
                .collect());
        }
        pos = body + len + (len & 1);
    }
    bail!("bundled benchmark clip has no data chunk")
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// Time `run` the standard way: a warm-up, then [`TIMED_RUNS`] timed calls.
/// `progress(phase)` is called as each of the [`PHASES_PER_STEP`] phases starts.
fn time_runs(mut run: impl FnMut() -> Result<()>, progress: &mut dyn FnMut(usize)) -> Result<f64> {
    progress(1);
    run().context("warm-up run")?;
    let mut times = Vec::with_capacity(TIMED_RUNS);
    for i in 0..TIMED_RUNS {
        progress(2 + i);
        let t = Instant::now();
        run().context("timed run")?;
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    Ok(median(times))
}

/// Run one step. Never panics the run on an engine's failure: the error is
/// recorded in the result so the other engines are still measured.
///
/// `progress(phase)` reports which of the [`PHASES_PER_STEP`] phases is
/// starting (0 = loading the model).
pub fn run_step(step: &BenchStep, cfg: &AppConfig, progress: &mut dyn FnMut(usize)) -> BenchResult {
    let mut result = BenchResult {
        engine: step.engine,
        device: step.device,
        model: step.model.clone(),
        load_ms: None,
        median_ms: None,
        error: None,
    };
    match measure(step, cfg, progress) {
        Ok((load_ms, median_ms)) => {
            result.load_ms = Some(load_ms);
            result.median_ms = Some(median_ms);
        }
        Err(e) => result.error = Some(format!("{e:#}")),
    }
    result
}

fn measure(step: &BenchStep, cfg: &AppConfig, progress: &mut dyn FnMut(usize)) -> Result<(f64, f64)> {
    let gpu = step.device == "gpu";
    progress(0);

    if step.engine == "s1_mini" {
        let t = Instant::now();
        // The sidecar loads on first use, and reloads when the device changes.
        crate::s1_mini::bench_clean(CLEANUP_TEXT, gpu).context("load S1-mini")?;
        let load_ms = t.elapsed().as_secs_f64() * 1000.0;
        let median_ms = time_runs(|| crate::s1_mini::bench_clean(CLEANUP_TEXT, gpu), progress)?;
        return Ok((load_ms, median_ms));
    }

    let audio = clip_samples()?;
    let req = TranscribeRequest {
        audio,
        language: None,
        word_timestamps: false,
        initial_prompt: None,
    };
    let device = if gpu { "auto" } else { "cpu" };

    let mut backend: Box<dyn TranscriptionBackend> = match step.engine {
        "whisper" => {
            let mut c = cfg.engine.whisper_cpp.clone();
            c.device = device.into();
            Box::new(crate::whisper_cpp::WhisperCppBackend::new(c))
        }
        #[cfg(feature = "moonshine")]
        "moonshine" => {
            let mut c = cfg.engine.moonshine.clone();
            c.device = device.into();
            Box::new(crate::moonshine::MoonshineBackend::new(c))
        }
        #[cfg(feature = "parakeet")]
        "parakeet" => {
            let mut c = cfg.engine.parakeet.clone();
            c.device = device.into();
            Box::new(crate::parakeet::ParakeetBackend::new(c))
        }
        other => bail!("{other} is not available in this build"),
    };

    let t = Instant::now();
    backend.load().context("load model")?;
    let load_ms = t.elapsed().as_secs_f64() * 1000.0;
    let outcome = time_runs(
        || backend.transcribe(&req).map(|_| ()),
        progress,
    );
    backend.unload();
    Ok((load_ms, outcome?))
}

/// The device each engine should use, from a finished run: the GPU only when it
/// is clearly faster (by more than 10%), because a tie is better spent on the
/// CPU — it needs no driver, no VRAM, and no shader warm-up. An engine with a
/// failed or missing half is left out rather than guessed at.
pub fn recommend(results: &[BenchResult]) -> Vec<(&'static str, &'static str)> {
    let mut out = Vec::new();
    for engine in ["whisper", "moonshine", "parakeet", "s1_mini"] {
        let time_for = |device: &str| {
            results
                .iter()
                .find(|r| r.engine == engine && r.device == device)
                .and_then(|r| r.median_ms)
        };
        if let (Some(cpu), Some(gpu)) = (time_for("cpu"), time_for("gpu")) {
            out.push((engine, if gpu < cpu * 0.9 { "gpu" } else { "cpu" }));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(engine: &'static str, device: &'static str, ms: Option<f64>) -> BenchResult {
        BenchResult { engine, device, model: "m".into(), load_ms: None, median_ms: ms, error: None }
    }

    #[test]
    fn the_bundled_clip_decodes_to_a_few_seconds_of_audio() {
        let samples = clip_samples().unwrap();
        let secs = samples.len() as f32 / 16_000.0;
        assert!((2.0..10.0).contains(&secs), "clip is {secs}s");
        assert!(samples.iter().any(|s| s.abs() > 0.05), "clip is silent");
    }

    #[test]
    fn the_gpu_is_recommended_only_when_clearly_faster() {
        let results = vec![
            r("s1_mini", "cpu", Some(300.0)),
            r("s1_mini", "gpu", Some(60.0)),
            r("moonshine", "cpu", Some(70.0)),
            r("moonshine", "gpu", Some(340.0)),
            // Within 10%: a tie goes to the CPU.
            r("parakeet", "cpu", Some(200.0)),
            r("parakeet", "gpu", Some(190.0)),
        ];
        let rec = recommend(&results);
        assert!(rec.contains(&("s1_mini", "gpu")));
        assert!(rec.contains(&("moonshine", "cpu")));
        assert!(rec.contains(&("parakeet", "cpu")));
    }

    #[test]
    fn an_engine_with_a_failed_half_gets_no_recommendation() {
        let results = vec![r("whisper", "cpu", Some(100.0)), r("whisper", "gpu", None)];
        assert!(recommend(&results).is_empty());
    }

    #[test]
    fn the_median_is_the_middle_run() {
        assert_eq!(median(vec![5.0, 1.0, 3.0]), 3.0);
    }
}

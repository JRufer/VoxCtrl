//! The process-wide ONNX Runtime environment.
//!
//! From `ort` 2.0.0-rc.14, `Session::builder` takes an [`Environment`], and
//! ONNX Runtime allows only one per process: `ort::init().build()` fails the
//! second time it is called, even after the first environment is dropped.
//! Moonshine and Parakeet (voxctrl-inference) and Inflect-Micro
//! (voxctrl-tts) all build sessions in the same process, so they cannot each
//! create their own. This crate creates it once, on first use, and keeps it
//! alive for the life of the process.
//!
//! Execution providers are not set here: each engine registers its own on its
//! session builder, so the GPU choice stays with the engine that knows whether
//! the user asked for it.

use std::sync::{Mutex, OnceLock};

pub use ort::environment::Environment;

static ENV: OnceLock<Environment> = OnceLock::new();
static INIT: Mutex<()> = Mutex::new(());

/// The shared environment, created on first call.
///
/// Errors only if ONNX Runtime itself fails to create the environment. A
/// failure is not cached, so a later call tries again.
pub fn environment() -> Result<&'static Environment, ort::Error> {
    if let Some(env) = ENV.get() {
        return Ok(env);
    }
    // `OnceLock::get_or_try_init` is not stable, and two threads racing
    // through `ort::init().build()` would make the loser fail with "only one
    // environment is allowed per process". Serialise creation instead.
    let _guard = INIT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(env) = ENV.get() {
        return Ok(env);
    }
    let env = ort::init().with_name("voxctrl").build()?;
    Ok(ENV.get_or_init(|| env))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every caller gets the same environment, including callers that race to
    /// create it: a second `ort::init().build()` would fail outright.
    #[test]
    fn one_environment_for_every_caller() {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| environment().expect("create ONNX Runtime environment") as *const _ as usize)
            })
            .collect();
        let first = environment().expect("create ONNX Runtime environment") as *const _ as usize;
        for h in handles {
            assert_eq!(h.join().unwrap(), first);
        }
    }
}

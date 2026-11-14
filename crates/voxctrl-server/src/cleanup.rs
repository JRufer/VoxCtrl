//! Dictation cleanup on the server: the S1-mini model a client would
//! otherwise run itself.
//!
//! A client that hands its transcription to this server can hand cleanup over
//! too, so it loads no language model of its own. The client still decides
//! *whether* a dictation is cleaned and in which style (per hotkey, per app);
//! the server only runs the model.

use anyhow::{Context, Result};
use tracing::info;
use voxctrl_inference::s1_mini;

/// Something that can tidy a transcript. A trait so the HTTP layer can be
/// tested without a model on disk.
pub trait Cleaner: Send + Sync {
    /// Clean `text` in the given style. Blocking: call it off the async runtime.
    fn clean(&self, text: &str, styling: &str) -> String;
    /// A name for the model, shown to clients and in the log.
    fn model(&self) -> &str;
}

/// S1-mini, through the llama.cpp sidecar when it is installed beside the
/// server and the slower in-process engine when it is not.
pub struct S1MiniCleaner {
    gpu: bool,
}

impl S1MiniCleaner {
    /// Download the model if it is not on disk yet and load it, so the first
    /// request does not pay for either.
    pub async fn prepare(gpu: bool) -> Result<Self> {
        if !s1_mini::is_s1_mini_downloaded(None) {
            info!("Downloading the S1-mini cleanup model (about 470 MB) to {}", s1_mini::s1_mini_dir(None).display());
            s1_mini::download_s1_mini_assets(None).await.context(
                "Could not download the S1-mini cleanup model. Check this machine's internet access, \
                 or turn cleanup off (cleanup.enabled = false).",
            )?;
        }
        if s1_mini::sidecar_available() {
            tokio::task::spawn_blocking(move || s1_mini::preload(None, gpu))
                .await
                .context("Loading the cleanup model stopped unexpectedly")?;
        } else {
            info!(
                "voxctrl-llm-sidecar was not found beside the server, so cleanup runs on the slower \
                 built-in CPU engine"
            );
        }
        Ok(Self { gpu })
    }
}

impl Cleaner for S1MiniCleaner {
    fn clean(&self, text: &str, styling: &str) -> String {
        s1_mini::clean_dictation(text, styling, None, self.gpu)
    }

    fn model(&self) -> &str {
        "s1-mini"
    }
}

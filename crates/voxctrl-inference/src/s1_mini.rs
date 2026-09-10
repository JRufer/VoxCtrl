//! S1-mini GGUF dictation cleanup processor.
//!
//! Uses Candle to run Superwhisper's s1-mini-q4_k_m.gguf model locally
//! in pure Rust with zero external process dependencies and zero C symbol collisions.

use std::fs::File;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{bail, Context, Result};
use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use tokenizers::Tokenizer;
use tracing::{debug, info, warn};

const S1_MINI_GGUF_URL: &str =
    "https://huggingface.co/superwhisper/s1-mini-GGUF/resolve/main/s1-mini-q4_k_m.gguf";
const S1_MINI_TOKENIZER_URL: &str =
    "https://huggingface.co/superwhisper/s1-mini/resolve/main/tokenizer.json";

const MODEL_FILENAME: &str = "s1-mini-q4_k_m.gguf";
const TOKENIZER_FILENAME: &str = "tokenizer.json";

/// System prompt required by S1-mini.
pub const S1_MINI_SYSTEM_PROMPT: &str =
    "You are a text normalizer for speech-to-text transcripts. The input begins with a control line specifying the styling, structure, and context settings; clean the transcript to match those settings and output only the cleaned text.";

/// Default styling if not specified.
pub const DEFAULT_STYLING: &str = "semi-formal";

pub fn s1_mini_dir(custom_dir: Option<&str>) -> PathBuf {
    match custom_dir {
        Some(dir) if !dir.trim().is_empty() => crate::util::expand_tilde(dir),
        _ => crate::util::models_base_dir().join("s1-mini"),
    }
}

pub fn is_s1_mini_downloaded(custom_dir: Option<&str>) -> bool {
    let dir = s1_mini_dir(custom_dir);
    let model_file = dir.join(MODEL_FILENAME);
    let tokenizer_file = dir.join(TOKENIZER_FILENAME);

    let model_ok = match std::fs::metadata(&model_file) {
        Ok(m) => m.len() > 10_000_000, // Expected ~462 MB
        Err(_) => false,
    };
    let tokenizer_ok = match std::fs::metadata(&tokenizer_file) {
        Ok(m) => m.len() > 100_000, // Expected ~11 MB
        Err(_) => false,
    };

    model_ok && tokenizer_ok
}

static DOWNLOAD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub async fn download_s1_mini_assets(custom_dir: Option<&str>) -> Result<()> {
    let _guard = DOWNLOAD_LOCK.lock().await;

    if is_s1_mini_downloaded(custom_dir) {
        return Ok(());
    }

    let dir = s1_mini_dir(custom_dir);
    tokio::fs::create_dir_all(&dir)
        .await
        .context("create s1-mini directory")?;

    let model_path = dir.join(MODEL_FILENAME);
    if !model_path.exists() || std::fs::metadata(&model_path).map(|m| m.len()).unwrap_or(0) < 10_000_000 {
        info!("Downloading S1-mini model from {}", S1_MINI_GGUF_URL);
        let resp = reqwest::get(S1_MINI_GGUF_URL).await?.error_for_status()?;
        let bytes = resp.bytes().await?;
        tokio::fs::write(&model_path, bytes)
            .await
            .context("write s1-mini model file")?;
        info!("Downloaded S1-mini GGUF to {}", model_path.display());
    }

    let tokenizer_path = dir.join(TOKENIZER_FILENAME);
    if !tokenizer_path.exists() || std::fs::metadata(&tokenizer_path).map(|m| m.len()).unwrap_or(0) < 100_000 {
        info!("Downloading S1-mini tokenizer from {}", S1_MINI_TOKENIZER_URL);
        let resp = reqwest::get(S1_MINI_TOKENIZER_URL).await?.error_for_status()?;
        let bytes = resp.bytes().await?;
        tokio::fs::write(&tokenizer_path, bytes)
            .await
            .context("write s1-mini tokenizer file")?;
        info!("Downloaded S1-mini tokenizer to {}", tokenizer_path.display());
    }

    Ok(())
}

struct ModelVariant {
    inner: candle_transformers::models::quantized_qwen3::ModelWeights,
}

impl ModelVariant {
    fn forward(&mut self, input: &Tensor, offset: usize) -> candle_core::Result<Tensor> {
        self.inner.forward(input, offset)
    }

    fn clear_kv_cache(&mut self) {
        self.inner.clear_kv_cache();
    }
}

pub struct S1MiniEngine {
    model: ModelVariant,
    tokenizer: Tokenizer,
    device: Device,
    eos_token_id: u32,
    im_end_token_id: Option<u32>,
}

impl S1MiniEngine {
    pub fn load(custom_dir: Option<&str>) -> Result<Self> {
        let dir = s1_mini_dir(custom_dir);
        let model_path = dir.join(MODEL_FILENAME);
        let tokenizer_path = dir.join(TOKENIZER_FILENAME);

        if !model_path.exists() || !tokenizer_path.exists() {
            bail!(
                "S1-mini files missing in {}. Please download S1-mini in Settings → Engine.",
                dir.display()
            );
        }

        info!("Loading S1-mini tokenizer from {}", tokenizer_path.display());
        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {e}"))?;

        let eos_token_id = tokenizer
            .token_to_id("<|endoftext|>")
            .unwrap_or(151643);
        let im_end_token_id = tokenizer.token_to_id("<|im_end|>");

        info!("Loading S1-mini GGUF model from {}", model_path.display());
        let mut file = File::open(&model_path).context("open s1-mini gguf file")?;
        let content = gguf_file::Content::read(&mut file).context("read gguf content")?;

        let arch = match content.metadata.get("general.architecture") {
            Some(v) => v.to_string().map(|s| s.as_str()).unwrap_or("qwen3"),
            None => "qwen3",
        };

        let device = Device::Cpu;

        debug!("Loading S1-mini using quantized_qwen3 (architecture: {arch})");
        let m = candle_transformers::models::quantized_qwen3::ModelWeights::from_gguf(
            content, &mut file, &device,
        )
        .context("load quantized_qwen3 weights")?;
        let model = ModelVariant { inner: m };

        info!("S1-mini engine loaded successfully");
        Ok(Self {
            model,
            tokenizer,
            device,
            eos_token_id,
            im_end_token_id,
        })
    }

    /// Construct prompt according to S1-mini chat template.
    pub fn build_prompt(text: &str, styling: &str) -> String {
        let style = if styling.is_empty() {
            DEFAULT_STYLING
        } else {
            styling
        };

        format!(
            "<|im_start|>system\n{}\n<|im_end|>\n<|im_start|>user\n[Styling: {}] [Structure: prose] [Context: general]\n{}\n<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n",
            S1_MINI_SYSTEM_PROMPT, style, text.trim()
        )
    }

    /// Process a text transcript and return normalized text.
    pub fn process(&mut self, text: &str, styling: &str) -> Result<String> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Ok(String::new());
        }

        let prompt_str = Self::build_prompt(trimmed, styling);
        let encoding = self
            .tokenizer
            .encode(prompt_str.as_str(), true)
            .map_err(|e| anyhow::anyhow!("Tokenize error: {e}"))?;

        let prompt_tokens = encoding.get_ids().to_vec();
        if prompt_tokens.is_empty() {
            return Ok(trimmed.to_string());
        }

        self.model.clear_kv_cache();

        let t0 = Instant::now();
        let prompt_len = prompt_tokens.len();

        // Feed entire prompt in forward pass with offset 0
        let input_tensor = Tensor::new(&prompt_tokens[..], &self.device)?.unsqueeze(0)?;
        let logits = self.model.forward(&input_tensor, 0)?;

        let mut next_token = logits
            .squeeze(0)?
            .argmax(candle_core::D::Minus1)?
            .to_scalar::<u32>()?;

        let mut generated_tokens = Vec::new();
        let max_tokens = (prompt_len * 2).clamp(64, 1024);

        for step in 0..max_tokens {
            if next_token == self.eos_token_id {
                break;
            }
            if let Some(im_end) = self.im_end_token_id {
                if next_token == im_end {
                    break;
                }
            }

            generated_tokens.push(next_token);

            let offset = prompt_len + step;
            let token_tensor = Tensor::new(&[next_token], &self.device)?.unsqueeze(0)?;
            let step_logits = self.model.forward(&token_tensor, offset)?;
            next_token = step_logits
                .squeeze(0)?
                .argmax(candle_core::D::Minus1)?
                .to_scalar::<u32>()?;
        }

        self.model.clear_kv_cache();

        let elapsed_ms = t0.elapsed().as_millis();
        let decoded = self
            .tokenizer
            .decode(&generated_tokens, true)
            .map_err(|e| anyhow::anyhow!("Decode error: {e}"))?;

        let cleaned = decoded.trim().to_string();
        debug!(
            in_len = trimmed.len(),
            out_len = cleaned.len(),
            tokens = generated_tokens.len(),
            took_ms = elapsed_ms,
            "S1-mini cleaned text"
        );

        Ok(cleaned)
    }
}

// Global cached engine instance
static GLOBAL_ENGINE: std::sync::OnceLock<Arc<Mutex<Option<S1MiniEngine>>>> =
    std::sync::OnceLock::new();

fn global_engine_cell() -> &'static Arc<Mutex<Option<S1MiniEngine>>> {
    GLOBAL_ENGINE.get_or_init(|| Arc::new(Mutex::new(None)))
}

/// Run text cleanup through S1-mini, lazily loading the model if needed.
/// Falls back to the original text on any error.
pub fn clean_dictation(text: &str, styling: &str, custom_dir: Option<&str>) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return text.to_string();
    }

    if !is_s1_mini_downloaded(custom_dir) {
        debug!("S1-mini requested but model files are not downloaded");
        return text.to_string();
    }

    let cell = global_engine_cell();
    let mut guard = match cell.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };

    if guard.is_none() {
        match S1MiniEngine::load(custom_dir) {
            Ok(engine) => {
                *guard = Some(engine);
            }
            Err(e) => {
                warn!("Failed to load S1-mini engine: {e:#}");
                return text.to_string();
            }
        }
    }

    if let Some(engine) = guard.as_mut() {
        match engine.process(text, styling) {
            Ok(cleaned) => {
                if cleaned.is_empty() && !text.trim().is_empty() {
                    // S1-mini returned empty (e.g. input was only fillers/noise); return empty string
                    String::new()
                } else {
                    cleaned
                }
            }
            Err(e) => {
                warn!("S1-mini processing error: {e:#}");
                text.to_string()
            }
        }
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s1_mini_prompt_formatting() {
        let prompt = S1MiniEngine::build_prompt("VoxCtrl is a great app.", "semi-formal");
        assert!(prompt.contains("<|im_start|>system"));
        assert!(prompt.contains(S1_MINI_SYSTEM_PROMPT));
        assert!(prompt.contains("[Styling: semi-formal]"));
        assert!(prompt.contains("VoxCtrl is a great app."));
        assert!(prompt.contains("<think>\n\n</think>"));
    }

    #[test]
    fn test_s1_mini_dir_default() {
        let dir = s1_mini_dir(None);
        assert!(dir.ends_with("s1-mini"));
    }
}

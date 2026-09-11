use std::io::{self, BufRead, Write};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::time::Instant;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::token::data_array::LlamaTokenDataArray;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct Request {
    #[serde(default)]
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    id: Option<serde_json::Value>,
    method: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct LoadParams {
    model_path: String,
}

#[derive(Debug, Deserialize)]
struct CleanParams {
    raw_text: String,
    #[serde(default)]
    styling: Option<String>,
    #[serde(default)]
    context: Option<String>,
    #[serde(default)]
    model_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct ResponseSuccess<T> {
    jsonrpc: &'static str,
    id: serde_json::Value,
    result: T,
}

#[derive(Debug, Serialize)]
struct ResponseError {
    jsonrpc: &'static str,
    id: serde_json::Value,
    error: ErrorBody,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: i32,
    message: String,
}

#[derive(Debug, Serialize)]
struct PingResult {
    status: &'static str,
    backend: String,
    gpu_enabled: bool,
    model_loaded: bool,
}

#[derive(Debug, Serialize)]
struct LoadResult {
    status: &'static str,
    model_path: String,
    device: String,
    gpu_layers: u32,
}

#[derive(Debug, Serialize)]
struct CleanResult {
    cleaned_text: String,
    elapsed_ms: u64,
    device: String,
}

struct LoadedState {
    model_path: PathBuf,
    model: LlamaModel,
    gpu_layers: u32,
    device_desc: String,
}

fn load_model(
    backend: &LlamaBackend,
    path: &Path,
) -> Result<(LlamaModel, u32, String), Box<dyn std::error::Error>> {
    if !path.exists() {
        return Err(format!("Model file not found: {}", path.display()).into());
    }

    #[cfg(feature = "vulkan")]
    {
        tracing::info!("Attempting to load model with Vulkan GPU offload: {}", path.display());
        let params = LlamaModelParams::default().with_n_gpu_layers(99);
        match LlamaModel::load_from_file(backend, path, &params) {
            Ok(model) => {
                tracing::info!("Successfully loaded model with Vulkan GPU acceleration");
                return Ok((model, 99, "vulkan".to_string()));
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to load model on Vulkan GPU ({}); falling back to CPU",
                    e
                );
            }
        }
    }

    tracing::info!("Loading model on CPU: {}", path.display());
    let params = LlamaModelParams::default().with_n_gpu_layers(0);
    let model = LlamaModel::load_from_file(backend, path, &params)?;
    Ok((model, 0, "cpu".to_string()))
}

fn clean_text(
    backend: &LlamaBackend,
    state: &LoadedState,
    raw_text: &str,
    styling: &str,
    context: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let prompt = format!(
        "<|im_start|>system\nYou are a text normalizer for speech-to-text transcripts. The input begins with a control line specifying the styling, structure, and context settings; clean the transcript to match those settings and output only the cleaned text.<|im_end|>\n<|im_start|>user\n[Styling: {}] [Structure: prose] [Context: {}]\n{}<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n",
        styling, context, raw_text
    );

    let tokens = state.model.str_to_token(&prompt, AddBos::Never)?;
    if tokens.is_empty() {
        return Ok(raw_text.to_string());
    }

    let n_ctx = ((tokens.len() + 512) as u32).max(1024);
    let ctx_params = LlamaContextParams::default()
        .with_n_ctx(NonZeroU32::new(n_ctx))
        .with_n_batch(512);

    let mut ctx = state.model.new_context(backend, ctx_params)?;
    let mut batch = LlamaBatch::new(tokens.len().max(512), 1);

    for (i, token) in tokens.iter().enumerate() {
        let is_last = i == tokens.len() - 1;
        batch.add(*token, i as i32, &[0], is_last)?;
    }
    ctx.decode(&mut batch)?;

    let mut output = String::new();
    let mut curr_pos = tokens.len() as i32;
    let mut decoder = encoding_rs::UTF_8.new_decoder();

    let max_gen_tokens = (tokens.len() * 2).clamp(128, 1024);
    for _ in 0..max_gen_tokens {
        let candidates = ctx.candidates_ith(batch.n_tokens() - 1);
        let mut candidates_arr = LlamaTokenDataArray::from_iter(candidates, false);
        let token = candidates_arr.sample_token_greedy();

        if state.model.is_eog_token(token) {
            break;
        }

        let piece = state.model.token_to_piece(token, &mut decoder, false, None)?;
        if piece.contains("<|im_end|>") || piece.contains("<|endoftext|>") {
            break;
        }
        output.push_str(&piece);

        batch.clear();
        batch.add(token, curr_pos, &[0], true)?;
        curr_pos += 1;
        ctx.decode(&mut batch)?;
    }

    let cleaned = strip_artifacts(&output);
    if cleaned.is_empty() {
        Ok(raw_text.to_string())
    } else {
        Ok(cleaned)
    }
}

fn strip_artifacts(text: &str) -> String {
    let mut res = text.trim();
    if let Some(pos) = res.find("</think>") {
        res = res[pos + 8..].trim();
    }
    res = res.trim_end_matches("<|im_end|>").trim();
    res.to_string()
}

fn send_response<T: Serialize>(stdout: &mut io::StdoutLock, id: serde_json::Value, result: T) {
    let resp = ResponseSuccess {
        jsonrpc: "2.0",
        id,
        result,
    };
    if let Ok(json) = serde_json::to_string(&resp) {
        let _ = writeln!(stdout, "{}", json);
        let _ = stdout.flush();
    }
}

fn send_error(stdout: &mut io::StdoutLock, id: serde_json::Value, code: i32, message: &str) {
    let resp = ResponseError {
        jsonrpc: "2.0",
        id,
        error: ErrorBody {
            code,
            message: message.to_string(),
        },
    };
    if let Ok(json) = serde_json::to_string(&resp) {
        let _ = writeln!(stdout, "{}", json);
        let _ = stdout.flush();
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tracing::info!("Starting VoxCtrl LLM sidecar...");

    let backend = match LlamaBackend::init() {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Failed to initialize LlamaBackend: {e}");
            eprintln!("Failed to initialize LlamaBackend: {e}");
            std::process::exit(1);
        }
    };

    let mut state: Option<LoadedState> = None;
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let req: Request = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                send_error(
                    &mut stdout_lock,
                    serde_json::Value::Null,
                    -32700,
                    &format!("Parse error: {e}"),
                );
                continue;
            }
        };

        let id = req.id.unwrap_or(serde_json::Value::Null);

        match req.method.as_str() {
            "ping" => {
                let gpu_enabled = state
                    .as_ref()
                    .map(|s| s.gpu_layers > 0)
                    .unwrap_or(cfg!(feature = "vulkan"));
                let backend_name = if cfg!(feature = "vulkan") {
                    "vulkan".to_string()
                } else {
                    "cpu".to_string()
                };
                send_response(
                    &mut stdout_lock,
                    id,
                    PingResult {
                        status: "ok",
                        backend: backend_name,
                        gpu_enabled,
                        model_loaded: state.is_some(),
                    },
                );
            }
            "load" => {
                let params: LoadParams = match req.params {
                    Some(p) => match serde_json::from_value(p) {
                        Ok(lp) => lp,
                        Err(e) => {
                            send_error(&mut stdout_lock, id, -32602, &format!("Invalid params: {e}"));
                            continue;
                        }
                    },
                    None => {
                        send_error(&mut stdout_lock, id, -32602, "Missing params");
                        continue;
                    }
                };

                let path = PathBuf::from(&params.model_path);
                if let Some(s) = &state {
                    if s.model_path == path {
                        send_response(
                            &mut stdout_lock,
                            id,
                            LoadResult {
                                status: "ok",
                                model_path: params.model_path,
                                device: s.device_desc.clone(),
                                gpu_layers: s.gpu_layers,
                            },
                        );
                        continue;
                    }
                }

                match load_model(&backend, &path) {
                    Ok((model, gpu_layers, device_desc)) => {
                        state = Some(LoadedState {
                            model_path: path,
                            model,
                            gpu_layers,
                            device_desc: device_desc.clone(),
                        });
                        send_response(
                            &mut stdout_lock,
                            id,
                            LoadResult {
                                status: "ok",
                                model_path: params.model_path,
                                device: device_desc,
                                gpu_layers,
                            },
                        );
                    }
                    Err(e) => {
                        send_error(
                            &mut stdout_lock,
                            id,
                            -32603,
                            &format!("Failed to load model: {e}"),
                        );
                    }
                }
            }
            "clean" => {
                let params: CleanParams = match req.params {
                    Some(p) => match serde_json::from_value(p) {
                        Ok(cp) => cp,
                        Err(e) => {
                            send_error(&mut stdout_lock, id, -32602, &format!("Invalid params: {e}"));
                            continue;
                        }
                    },
                    None => {
                        send_error(&mut stdout_lock, id, -32602, "Missing params");
                        continue;
                    }
                };

                // If a model path was passed and model not yet loaded, load it
                if state.is_none() {
                    if let Some(mp) = &params.model_path {
                        let path = PathBuf::from(mp);
                        match load_model(&backend, &path) {
                            Ok((model, gpu_layers, device_desc)) => {
                                state = Some(LoadedState {
                                    model_path: path,
                                    model,
                                    gpu_layers,
                                    device_desc,
                                });
                            }
                            Err(e) => {
                                send_error(
                                    &mut stdout_lock,
                                    id,
                                    -32603,
                                    &format!("Failed to auto-load model: {e}"),
                                );
                                continue;
                            }
                        }
                    } else {
                        send_error(
                            &mut stdout_lock,
                            id,
                            -32603,
                            "No model loaded and model_path parameter not specified",
                        );
                        continue;
                    }
                }

                let current_state = match &state {
                    Some(s) => s,
                    None => {
                        send_error(&mut stdout_lock, id, -32603, "Model not loaded");
                        continue;
                    }
                };

                let t0 = Instant::now();
                let styling = params.styling.as_deref().unwrap_or("casual");
                let context = params.context.as_deref().unwrap_or("general");

                match clean_text(
                    &backend,
                    current_state,
                    &params.raw_text,
                    styling,
                    context,
                ) {
                    Ok(cleaned) => {
                        let elapsed_ms = t0.elapsed().as_millis() as u64;
                        send_response(
                            &mut stdout_lock,
                            id,
                            CleanResult {
                                cleaned_text: cleaned,
                                elapsed_ms,
                                device: current_state.device_desc.clone(),
                            },
                        );
                    }
                    Err(e) => {
                        send_error(
                            &mut stdout_lock,
                            id,
                            -32603,
                            &format!("Inference failed: {e}"),
                        );
                    }
                }
            }
            other => {
                send_error(
                    &mut stdout_lock,
                    id,
                    -32601,
                    &format!("Method not found: {other}"),
                );
            }
        }
    }

    tracing::info!("VoxCtrl LLM sidecar exiting.");
}

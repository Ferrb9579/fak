use encoding_rs::UTF_8;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::{LogOptions, send_logs_to_tracing};
use std::io::Write;
use std::num::NonZeroU32;
use tempfile::NamedTempFile;

const MAX_OUTPUT_TOKENS: usize = 32;
const CONTEXT_SIZE: u32 = 1024;

// The model is part of the executable at compile time. llama.cpp's public
// loader accepts a path, so the bytes are materialized into a private
// temporary file while the model is loaded. No model download or Ollama
// installation is required at runtime.
static MODEL_GGUF: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/models/LFM2.5-230M.Q4_K_M.gguf"
));

pub fn fix_command(cmd: &str, preamble: &str) -> Result<String, String> {
    let prompt = format!(
        concat!(
            "<|im_start|>system\n{preamble}<|im_end|>\n",
            "<|im_start|>user\nInput: ggit status<|im_end|>\n",
            "<|im_start|>assistant\ngit status<|im_end|>\n",
            "<|im_start|>user\nInput: crago check<|im_end|>\n",
            "<|im_start|>assistant\ncargo check<|im_end|>\n",
            "<|im_start|>user\nInput: git stats<|im_end|>\n",
            "<|im_start|>assistant\ngit status<|im_end|>\n",
            "<|im_start|>user\nInput: {cmd}<|im_end|>\n",
            "<|im_start|>assistant\n"
        ),
        preamble = preamble,
        cmd = cmd
    );

    // The embedded GGUF is loaded through llama.cpp's mmap-friendly file API.
    // Keeping this handle alive also keeps the mapped file valid for the
    // model's entire lifetime.
    let mut model_file = NamedTempFile::new()
        .map_err(|e| format!("Failed to create a temporary embedded model file: {e}"))?;
    model_file
        .write_all(MODEL_GGUF)
        .map_err(|e| format!("Failed to materialize the embedded GGUF: {e}"))?;

    // llama.cpp emits the common_params_fit_impl diagnostic through its
    // logger. Suppress the native logger entirely; errors still arrive via
    // the safe Rust Result APIs below.
    send_logs_to_tracing(LogOptions::default().with_logs_enabled(false));

    let backend = LlamaBackend::init()
        .map_err(|e| format!("Failed to initialize the embedded llama backend: {e}"))?;
    let model_params = LlamaModelParams::default();
    let model = LlamaModel::load_from_file(&backend, model_file.path(), &model_params)
        .map_err(|e| format!("Failed to load the embedded LFM2 model: {e}"))?;

    let context_params = LlamaContextParams::default().with_n_ctx(Some(
        NonZeroU32::new(CONTEXT_SIZE).expect("CONTEXT_SIZE is non-zero"),
    ));
    let mut context = model
        .new_context(&backend, context_params)
        .map_err(|e| format!("Failed to create the llama context: {e}"))?;

    let prompt_tokens = model
        .str_to_token(&prompt, AddBos::Always)
        .map_err(|e| format!("Failed to tokenize command: {e}"))?;
    if prompt_tokens.is_empty() {
        return Err("The command prompt produced no tokens".to_string());
    }

    // Submit the whole prompt in one batch. Only its final position needs
    // logits, while llama.cpp retains the KV state for subsequent decoding.
    let prompt_len = prompt_tokens.len();
    let last_prompt_position = i32::try_from(prompt_len - 1)
        .map_err(|_| "The command prompt is too long for llama.cpp".to_string())?;
    let mut batch = LlamaBatch::new(prompt_len.max(512), 1);
    for (position, token) in prompt_tokens.into_iter().enumerate() {
        let position = i32::try_from(position)
            .map_err(|_| "The command prompt is too long for llama.cpp".to_string())?;
        batch
            .add(token, position, &[0], position == last_prompt_position)
            .map_err(|e| format!("Failed to create the llama prompt batch: {e}"))?;
    }
    context
        .decode(&mut batch)
        .map_err(|e| format!("llama.cpp prompt evaluation failed: {e}"))?;

    let mut sampler = LlamaSampler::greedy();
    let mut decoder = UTF_8.new_decoder();
    let mut response = String::new();

    for generated_position in 0..MAX_OUTPUT_TOKENS {
        let token = sampler.sample(&context, batch.n_tokens() - 1);
        sampler.accept(token);

        if model.is_eog_token(token) {
            break;
        }

        let piece = model
            .token_to_piece(token, &mut decoder, true, None)
            .map_err(|e| format!("Failed to decode the llama response: {e}"))?;
        response.push_str(&piece);

        batch.clear();
        let position = i32::try_from(prompt_len + generated_position)
            .map_err(|_| "The generated command is too long for llama.cpp".to_string())?;
        batch
            .add(token, position, &[0], true)
            .map_err(|e| format!("Failed to create the llama generation batch: {e}"))?;
        context
            .decode(&mut batch)
            .map_err(|e| format!("llama.cpp generation failed: {e}"))?;
    }

    Ok(clean_response(response))
}

fn clean_response(response: String) -> String {
    let response = response
        .rsplit_once("</think>")
        .map_or(response.as_str(), |(_, answer)| answer);
    let response = response.trim();
    let response = response
        .split_once("Output:")
        .map_or(response, |(_, answer)| answer);
    let response = response
        .split_once("corrected command:")
        .map_or(response, |(_, answer)| answer);

    response
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .find(|line| !line.to_ascii_lowercase().contains("here's the corrected"))
        .unwrap_or_default()
        .trim_matches(|character| matches!(character, '`' | '*'))
        .trim()
        .to_string()
}

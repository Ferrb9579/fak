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
            "<|im_start|>system\n{preamble}\n",
            "Preserve the intended program and arguments; change only spelling mistakes.\n",
            "Examples:\n",
            "Input: ggit status\nOutput: git status\n",
            "Input: git statuss\nOutput: git status\n",
            "Input: crago check\nOutput: cargo check\n",
            "Input: git stats\nOutput: git status\n",
            "<|im_end|>\n",
            "<|im_start|>user\nInput: {cmd}\nOutput:<|im_end|>\n",
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

    // Use LFM2.5's published repetition penalty and deterministic decoding so
    // the same typo produces the same model-generated correction every time.
    let mut sampler = LlamaSampler::chain_simple([
        LlamaSampler::top_k(50),
        LlamaSampler::temp(0.1),
        LlamaSampler::penalties(-1, 1.05, 0.0, 0.0),
        LlamaSampler::greedy(),
    ]);
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
    let response = response
        .split_once("<|im_end|>")
        .map_or(response, |(answer, _)| answer)
        .trim();

    // Small instruction-tuned models sometimes wrap the answer in a label,
    // a sentence, or inline code even when asked for plain text. Extract the
    // model's answer; do not invent or rewrite a command here.
    for line in response
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(value) = after_output_marker(line) {
            if let Some(command) = extract_inline_code(value) {
                return command;
            }
            let value = clean_candidate(value);
            if !value.is_empty() {
                return value;
            }
        }
    }

    for line in response
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(command) = extract_inline_code(line) {
            return command;
        }

        let lower = line.to_ascii_lowercase();
        if !lower.starts_with("input:")
            && !lower.starts_with("wrong:")
            && !lower.contains("the command")
            && !lower.contains("the input")
            && !lower.contains("the output")
            && !lower.contains("corrected command")
            && !lower.contains("correct command")
            && !lower.contains("explanation")
            && !lower.contains("here's")
        {
            let value = clean_candidate(line);
            if !value.is_empty() {
                return value;
            }
        }
    }

    String::new()
}

fn after_output_marker(line: &str) -> Option<&str> {
    const MARKERS: [&str; 7] = [
        "output is:",
        "output:",
        "corrected command is:",
        "corrected command:",
        "correct:",
        "answer:",
        "command:",
    ];

    let lower = line.to_ascii_lowercase();
    MARKERS.iter().find_map(|marker| {
        lower
            .find(marker)
            .map(|index| &line[index + marker.len()..])
    })
}

fn extract_inline_code(value: &str) -> Option<String> {
    let mut result = None;
    let mut start = 0;

    while let Some(open_offset) = value[start..].find('`') {
        let open = start + open_offset + 1;
        let close = value[open..].find('`')? + open;
        let candidate = clean_candidate(&value[open..close]);
        if !candidate.is_empty() {
            result = Some(candidate);
        }
        start = close + 1;
    }

    result
}

fn clean_candidate(value: &str) -> String {
    value
        .trim()
        .trim_matches(|character| matches!(character, '`' | '*' | '"'))
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::clean_response;

    #[test]
    fn extracts_a_labeled_model_command() {
        assert_eq!(
            clean_response("Output: git status".to_string()),
            "git status"
        );
    }

    #[test]
    fn extracts_the_last_inline_answer_from_a_model_sentence() {
        assert_eq!(
            clean_response(
                "The input is `git statuss` and the output is `git status`.".to_string()
            ),
            "git status"
        );
    }

    #[test]
    fn extracts_a_command_from_a_markdown_answer() {
        assert_eq!(
            clean_response("The corrected command is:\n\n`git status`".to_string()),
            "git status"
        );
    }
}

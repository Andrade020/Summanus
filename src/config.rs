use serde::{Deserialize, Serialize};
use std::{env, path::PathBuf};

#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    pub temperature: f32,
    pub top_p: f32,
    pub max_tokens: u32,
    pub repeat_penalty: f32,
    #[serde(default = "default_presence_penalty")]
    pub presence_penalty: f32,
    pub thinking: bool,
    pub use_cache: bool,
}

fn env_or<T: std::str::FromStr>(key: &str, fallback: T) -> T {
    env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(fallback)
}

fn default_presence_penalty() -> f32 {
    1.5
}

pub fn configured_thinking() -> Option<bool> {
    let value = env::var("ENABLE_THINKING").ok()?;
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => Some(true),
        "false" | "0" | "no" => Some(false),
        _ => None,
    }
}

impl Default for Settings {
    fn default() -> Self {
        let thinking = configured_thinking().unwrap_or(true);
        Self {
            temperature: env_or("DEFAULT_TEMPERATURE", 0.6),
            top_p: env_or("DEFAULT_TOP_P", 0.95),
            max_tokens: env_or("DEFAULT_MAX_TOKENS", 4096),
            repeat_penalty: env_or("DEFAULT_REPEAT_PENALTY", 1.0),
            presence_penalty: env_or("DEFAULT_PRESENCE_PENALTY", if thinking { 0.0 } else { 1.5 }),
            thinking,
            use_cache: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_saved_settings_get_presence_penalty() {
        let settings: Settings = serde_json::from_str(r#"{"temperature":0.6,"top_p":0.95,"max_tokens":4096,"repeat_penalty":1.0,"thinking":false,"use_cache":false}"#).unwrap();
        assert_eq!(settings.presence_penalty, 1.5);
    }
}

#[derive(Clone)]
pub struct ServerConfig {
    pub executable: PathBuf,
    pub model: PathBuf,
    pub n_ctx: u32,
    pub n_threads: u32,
    pub n_threads_batch: u32,
    pub n_gpu_layers: u32,
    pub n_cpu_moe: u32,
    pub extra_args: Vec<String>,
}

impl ServerConfig {
    pub fn from_env(model_override: Option<String>) -> Self {
        let n_threads = env_or("N_THREADS", 0);
        let extra_args =
            shell_words::split(&env::var("LLAMA_SERVER_EXTRA_ARGS").unwrap_or_default())
                .unwrap_or_default();
        Self {
            executable: env::var("LLAMA_SERVER_PATH")
                .unwrap_or_else(|_| "llama-server.exe".into())
                .into(),
            model: model_override
                .or_else(|| env::var("MODEL_PATH").ok())
                .unwrap_or_default()
                .into(),
            n_ctx: env_or("N_CTX", 8192),
            n_threads,
            n_threads_batch: env_or("N_THREADS_BATCH", n_threads),
            n_gpu_layers: env_or("N_GPU_LAYERS", 0),
            n_cpu_moe: env_or("N_CPU_MOE", 0),
            extra_args,
        }
    }
}

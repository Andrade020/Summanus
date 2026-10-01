use crate::config::ServerConfig;
use serde::Deserialize;
use std::{env, path::PathBuf, process::Command};

pub const AUTO: &str = "auto";
pub const CUSTOM: &str = "custom";
pub const Q8_CACHE: &str = "-ctk q8_0 -ctv q8_0";
const Q3_FILE: &str = "Qwen3.6-35B-A3B-MTP-UD-IQ3_XXS.gguf";

pub struct Profile {
    pub id: &'static str,
    pub name: &'static str,
    pub detail: &'static str,
    pub recommender_id: Option<&'static str>,
    pub q3: bool,
}

pub const PROFILES: &[Profile] = &[
    Profile {
        id: "a",
        name: "A · Rápido",
        detail: "Q4 · 16K · cache Q8",
        recommender_id: Some("c16k"),
        q3: false,
    },
    Profile {
        id: "b",
        name: "B · Contexto livre",
        detail: "Q4 · 32K · saída na CPU para liberar VRAM",
        recommender_id: Some("c32k+saidaCPU"),
        q3: false,
    },
    Profile {
        id: "c",
        name: "C · Atual",
        detail: "Q4 · 32K · configuração original",
        recommender_id: Some("c32k"),
        q3: false,
    },
    Profile {
        id: "q3_fast",
        name: "Q3 · Ágil",
        detail: "IQ3 · 32K · melhor resultado medido: ~12 t/s",
        recommender_id: None,
        q3: true,
    },
    Profile {
        id: "q3_light",
        name: "Q3 · Poupar VRAM",
        detail: "IQ3 · 16K · saída na CPU; velocidade não medida",
        recommender_id: None,
        q3: true,
    },
    Profile {
        id: "patient",
        name: "Paciente",
        detail: "Q4 · 48K · só CPU · raciocínio e respostas longas",
        recommender_id: None,
        q3: false,
    },
];

#[derive(Clone, Debug, Deserialize)]
pub struct Recommendation {
    pub vram_usada_mib: u32,
    pub ram_disponivel_gib: f32,
    pub cpu_pct: f32,
    pub modelo: PathBuf,
    pub threads: u32,
    pub recomendada: Option<RecommendedLine>,
    #[serde(default)]
    pub linhas: Vec<MeasuredLine>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RecommendedLine {
    pub id: String,
    pub ctx: u32,
    pub extra: String,
    pub tps_esperado: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MeasuredLine {
    pub id: String,
    pub status: String,
}

impl Recommendation {
    pub fn status_for(&self, id: &str) -> Option<&str> {
        self.linhas
            .iter()
            .find(|line| line.id == id)
            .map(|line| line.status.as_str())
    }
}

pub fn q3_path(base: &ServerConfig) -> PathBuf {
    base.model.with_file_name(Q3_FILE)
}

pub fn known_choice(choice: &str) -> bool {
    choice == AUTO || choice == CUSTOM || PROFILES.iter().any(|profile| profile.id == choice)
}

pub fn config_for(
    base: &ServerConfig,
    choice: &str,
    recommendation: Option<&Recommendation>,
    forced_model: bool,
) -> Result<ServerConfig, String> {
    let mut config = base.clone();
    match choice {
        AUTO => {
            let recommendation = recommendation.ok_or("Recomendação ainda indisponível")?;
            let line = recommendation.recomendada.as_ref().ok_or(
                "O recomendador não encontrou uma configuração adequada agora. Libere RAM ou VRAM, ou escolha um perfil manual.",
            )?;
            config.model = recommendation.modelo.clone();
            config.n_ctx = line.ctx;
            config.n_threads = recommendation.threads;
            config.n_threads_batch = recommendation.threads;
            config.n_gpu_layers = if line.id == "so-CPU" { 0 } else { 99 };
            config.n_cpu_moe = if line.id == "so-CPU" { 0 } else { 41 };
            config.extra_args =
                shell_words::split(&line.extra).map_err(|error| error.to_string())?;
        }
        "a" => {
            config.n_ctx = 16384;
            config.n_gpu_layers = 99;
            config.n_cpu_moe = 41;
            config.extra_args = shell_words::split(Q8_CACHE).unwrap();
        }
        "b" => {
            config.n_ctx = 32768;
            config.n_gpu_layers = 99;
            config.n_cpu_moe = 41;
            config.extra_args =
                shell_words::split(&format!("{Q8_CACHE} -ot output.weight=CPU")).unwrap();
        }
        "c" => {
            config.n_ctx = 32768;
            config.n_gpu_layers = 99;
            config.n_cpu_moe = 41;
            config.extra_args = shell_words::split(Q8_CACHE).unwrap();
        }
        "q3_fast" => {
            config.model = q3_path(base);
            config.n_ctx = 32768;
            config.n_gpu_layers = 99;
            config.n_cpu_moe = 39;
            config.extra_args =
                shell_words::split(&format!("{Q8_CACHE} --load-mode none")).unwrap();
        }
        "q3_light" => {
            config.model = q3_path(base);
            config.n_ctx = 16384;
            config.n_gpu_layers = 99;
            config.n_cpu_moe = 41;
            config.extra_args = shell_words::split(&format!(
                "{Q8_CACHE} -ot output.weight=CPU --load-mode none"
            ))
            .unwrap();
        }
        "patient" => {
            config.n_ctx = 49152;
            config.n_gpu_layers = 0;
            config.n_cpu_moe = 0;
            config.extra_args = shell_words::split(Q8_CACHE).unwrap();
        }
        CUSTOM => {}
        _ => return Err(format!("Perfil desconhecido: {choice}")),
    }
    if forced_model {
        config.model = base.model.clone();
    }
    if !config.model.is_file() {
        return Err(format!(
            "Modelo do perfil não encontrado: {}",
            config.model.display()
        ));
    }
    Ok(config)
}

pub fn recommender_path() -> PathBuf {
    env::var_os("SUMMANUS_RECOMMENDER_PATH")
        .or_else(|| env::var_os("LOCAL_LLM_RECOMMENDER_PATH"))
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("USERPROFILE")
                .map(|home| PathBuf::from(home).join("Desktop/LLM/recomendar.bat"))
        })
        .unwrap_or_else(|| PathBuf::from("recomendar.bat"))
}

#[cfg(windows)]
pub fn run_recommender() -> Result<Recommendation, String> {
    use std::os::windows::process::CommandExt;
    let batch = recommender_path();
    if !batch.is_file() {
        return Err(format!("Recomendador não encontrado: {}", batch.display()));
    }
    let output = Command::new("cmd.exe")
        .args(["/D", "/C"])
        .arg(&batch)
        .args(["--json", "--ignorar-servidor"])
        .current_dir(batch.parent().unwrap())
        .creation_flags(0x08000000)
        .output()
        .map_err(|error| format!("Falha ao executar recomendar.bat: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "recomendar.bat: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("JSON inválido do recomendar.bat: {error}"))
}

#[cfg(not(windows))]
pub fn run_recommender() -> Result<Recommendation, String> {
    Err("O recomendador atual requer Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> ServerConfig {
        ServerConfig {
            executable: PathBuf::from("server.exe"),
            model: PathBuf::from("models/Qwen3.6-35B-A3B-UD-Q4_K_XL.gguf"),
            n_ctx: 32768,
            n_threads: 8,
            n_threads_batch: 8,
            n_gpu_layers: 99,
            n_cpu_moe: 41,
            extra_args: vec!["-ctk".into(), "q8_0".into(), "-ctv".into(), "q8_0".into()],
        }
    }

    #[test]
    fn profile_args_match_requested_modes() {
        let mut base = base();
        // Avoid testing files on disk: validate profile changes with an existing model path.
        base.model = std::env::current_exe().unwrap();
        let fast = config_for(&base, "a", None, false).unwrap();
        assert_eq!(fast.n_ctx, 16384);
        assert_eq!(fast.extra_args, vec!["-ctk", "q8_0", "-ctv", "q8_0"]);
        let safe = config_for(&base, "b", None, false).unwrap();
        assert!(safe.extra_args.iter().any(|arg| arg == "output.weight=CPU"));
        let patient = config_for(&base, "patient", None, false).unwrap();
        assert_eq!((patient.n_gpu_layers, patient.n_cpu_moe), (0, 0));
        assert_eq!(patient.n_ctx, 49152);
    }

    #[test]
    fn automatic_recommendation_maps_threads_and_model() {
        let mut base = base();
        base.model = std::env::current_exe().unwrap();
        let rec = Recommendation {
            vram_usada_mib: 900,
            ram_disponivel_gib: 18.0,
            cpu_pct: 50.0,
            modelo: base.model.clone(),
            threads: 6,
            recomendada: Some(RecommendedLine {
                id: "c32k+saidaCPU".into(),
                ctx: 32768,
                extra: format!("{Q8_CACHE} -ot output.weight=CPU"),
                tps_esperado: 8.0,
            }),
            linhas: vec![],
        };
        let selected = config_for(&base, AUTO, Some(&rec), false).unwrap();
        assert_eq!((selected.n_threads, selected.n_threads_batch), (6, 6));
        assert!(selected
            .extra_args
            .iter()
            .any(|arg| arg == "output.weight=CPU"));
    }

    #[test]
    fn q3_fast_uses_its_own_file_and_measured_settings() {
        let directory =
            std::env::temp_dir().join(format!("summanus-profile-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let q3 = directory.join(Q3_FILE);
        std::fs::write(&q3, b"GGUF").unwrap();
        let mut base = base();
        base.model = directory.join("q4.gguf");
        let selected = config_for(&base, "q3_fast", None, false).unwrap();
        assert_eq!(selected.model, q3);
        assert_eq!((selected.n_ctx, selected.n_cpu_moe), (32768, 39));
        assert!(selected
            .extra_args
            .windows(2)
            .any(|pair| pair == ["--load-mode", "none"]));
        std::fs::remove_file(q3).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}

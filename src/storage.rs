use crate::config::Settings;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::{fs, path::Path};

#[derive(Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub reasoning: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_tps: Option<f64>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: u64,
    pub title: String,
    pub messages: Vec<Message>,
    #[serde(default)]
    pub contexts: Vec<ContextCard>,
    #[serde(default)]
    pub compact_context_id: Option<u64>,
    #[serde(default)]
    pub use_full_history: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ContextCard {
    pub id: u64,
    pub title: String,
    pub note: String,
    pub summary: String,
    /// Zero-based first message in the saved recorte.
    pub start: usize,
    /// Exclusive end of the saved recorte.
    pub end: usize,
    #[serde(default)]
    pub reference: bool,
}

impl Conversation {
    pub fn add_context(&mut self, start: usize, end: usize, title: String, note: String) -> u64 {
        let id = self.contexts.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        self.contexts.push(ContextCard {
            id,
            title,
            note,
            summary: String::new(),
            start,
            end,
            reference: false,
        });
        id
    }

    pub fn effective_compact_context(&self) -> Option<&ContextCard> {
        if self.use_full_history {
            return None;
        }
        let valid = |card: &&ContextCard| {
            card.start == 0 && card.end <= self.messages.len() && !card.summary.trim().is_empty()
        };
        self.compact_context_id
            .and_then(|id| self.contexts.iter().find(|card| card.id == id))
            .filter(valid)
            .or_else(|| {
                self.contexts
                    .iter()
                    .filter(valid)
                    .max_by_key(|card| card.end)
            })
    }

    pub fn messages_for_request(&self) -> Vec<Message> {
        let compact = self.effective_compact_context();
        let mut request = Vec::new();
        if let Some(card) = compact {
            request.push(Message {
                role: "system".into(),
                content: format!(
                    "Memória compactada da conversa até a mensagem {}. Tema: {}. Anotação: {}. Resumo:\n{}",
                    card.end, card.title, card.note, card.summary
                ),
                reasoning: String::new(),
                speed_tps: None,
            });
        }
        for card in self.contexts.iter().filter(|card| card.reference) {
            if compact.is_some_and(|active| active.id == card.id) || card.summary.trim().is_empty()
            {
                continue;
            }
            request.push(Message {
                role: "system".into(),
                content: format!(
                    "Contexto de referência '{}' (mensagens {} a {}): {}\n{}",
                    card.title,
                    card.start + 1,
                    card.end,
                    card.note,
                    card.summary
                ),
                reasoning: String::new(),
                speed_tps: None,
            });
        }
        let from = compact.map_or(0, |card| card.end);
        request.extend_from_slice(&self.messages[from..]);
        request
    }
}

#[derive(Serialize, Deserialize)]
pub struct State {
    pub conversations: Vec<Conversation>,
    pub current_id: u64,
    pub settings: Settings,
    #[serde(default)]
    pub last_model: Option<PathBuf>,
    #[serde(default = "default_profile_choice")]
    pub profile_choice: String,
    #[serde(default)]
    pub patient_previous_settings: Option<Settings>,
}

fn default_profile_choice() -> String {
    "auto".into()
}

impl State {
    pub fn load() -> Self {
        fs::read("data/state.json")
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .filter(|s| !s.conversations.is_empty())
            .unwrap_or_else(|| Self {
                conversations: vec![Conversation {
                    id: 1,
                    title: "Nova conversa".into(),
                    messages: vec![],
                    contexts: vec![],
                    compact_context_id: None,
                    use_full_history: false,
                }],
                current_id: 1,
                settings: Settings::default(),
                last_model: None,
                profile_choice: default_profile_choice(),
                patient_previous_settings: None,
            })
    }

    pub fn save(&self) -> Result<(), String> {
        fs::create_dir_all("data").map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        fs::write("data/state.json.tmp", bytes).map_err(|e| e.to_string())?;
        fs::rename("data/state.json.tmp", "data/state.json").map_err(|e| e.to_string())
    }

    pub fn current(&self) -> &Conversation {
        self.conversations
            .iter()
            .find(|c| c.id == self.current_id)
            .unwrap_or(&self.conversations[0])
    }

    pub fn current_mut(&mut self) -> &mut Conversation {
        let index = self
            .conversations
            .iter()
            .position(|c| c.id == self.current_id)
            .unwrap_or(0);
        &mut self.conversations[index]
    }

    pub fn new_conversation(&mut self) {
        let id = self.conversations.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        self.current_id = id;
        self.conversations.insert(
            0,
            Conversation {
                id,
                title: "Nova conversa".into(),
                messages: vec![],
                contexts: vec![],
                compact_context_id: None,
                use_full_history: false,
            },
        );
    }

    pub fn delete_current(&mut self) {
        if self.conversations.len() > 1 {
            self.conversations.retain(|c| c.id != self.current_id);
            self.current_id = self.conversations[0].id;
        } else {
            self.conversations[0].messages.clear();
            self.conversations[0].contexts.clear();
            self.conversations[0].compact_context_id = None;
            self.conversations[0].use_full_history = false;
            self.conversations[0].title = "Nova conversa".into();
        }
    }
}

pub fn cache_key(model: &Path, settings: &Settings, messages: &[Message]) -> String {
    let mut hasher = Sha256::new();
    // Keep the old namespace so upgrading from Local_LLM/Lume preserves cache keys.
    hasher.update(b"local-llm-v3\0");
    hasher.update(model.to_string_lossy().as_bytes());
    if let Ok(meta) = fs::metadata(model) {
        hasher.update(meta.len().to_le_bytes());
        if let Ok(age) = meta.modified().and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .map_err(std::io::Error::other)
        }) {
            hasher.update(age.as_secs().to_le_bytes());
        }
    }
    let prompt = messages
        .iter()
        .map(|message| (&message.role, &message.content, &message.reasoning))
        .collect::<Vec<_>>();
    hasher.update(serde_json::to_vec(&(settings, prompt)).unwrap_or_default());
    format!("{:x}", hasher.finalize())
}

pub fn read_cache(key: &str) -> Option<String> {
    fs::read_to_string(format!("cache/{key}.txt")).ok()
}

pub fn write_cache(key: &str, content: &str) {
    if fs::create_dir_all("cache").is_ok() {
        let _ = fs::write(format!("cache/{key}.txt"), content);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_conversations_load_and_compaction_preserves_new_messages() {
        let old = r#"{"id":1,"title":"Teste","messages":[{"role":"user","content":"primeira"},{"role":"assistant","content":"resposta"},{"role":"user","content":"agora"}]}"#;
        let mut conv: Conversation = serde_json::from_str(old).unwrap();
        assert!(conv.contexts.is_empty());
        let id = conv.add_context(0, 2, "Início".into(), "Objetivo inicial".into());
        conv.contexts[0].summary = "Decidimos usar Rust.".into();
        conv.compact_context_id = Some(id);
        let request = conv.messages_for_request();
        assert_eq!(request.len(), 2);
        assert_eq!(request[0].role, "system");
        assert!(request[0].content.contains("Decidimos usar Rust."));
        assert_eq!(request[1].content, "agora");
        assert!(!request.iter().any(|message| message.content == "primeira"));
        conv.compact_context_id = None;
        assert_eq!(conv.messages_for_request().len(), 2);
        let latest = conv.add_context(0, 3, "Tudo".into(), String::new());
        conv.contexts.last_mut().unwrap().summary = "Resumo completo".into();
        let compacted = conv.messages_for_request();
        assert_eq!(compacted.len(), 1);
        assert!(compacted[0].content.contains("Resumo completo"));
        assert_eq!(conv.effective_compact_context().unwrap().id, latest);
        conv.use_full_history = true;
        assert_eq!(conv.messages_for_request().len(), 3);
    }

    #[test]
    fn cache_key_includes_model_and_sampling_settings() {
        let message = vec![Message {
            role: "user".into(),
            content: "Olá".into(),
            reasoning: String::new(),
            speed_tps: None,
        }];
        let a = Settings::default();
        let mut b = a.clone();
        b.repeat_penalty += 0.1;
        assert_ne!(
            cache_key(Path::new("a.gguf"), &a, &message),
            cache_key(Path::new("a.gguf"), &b, &message)
        );
        assert_ne!(
            cache_key(Path::new("a.gguf"), &a, &message),
            cache_key(Path::new("b.gguf"), &a, &message)
        );
        let mut c = a.clone();
        c.presence_penalty += 0.1;
        assert_ne!(
            cache_key(Path::new("a.gguf"), &a, &message),
            cache_key(Path::new("a.gguf"), &c, &message)
        );
    }

    #[test]
    fn speed_survives_history_without_changing_prompt_cache() {
        let old: Message =
            serde_json::from_str(r#"{"role":"assistant","content":"Olá","reasoning":""}"#).unwrap();
        assert_eq!(old.speed_tps, None);
        let mut measured = old.clone();
        measured.speed_tps = Some(12.5);
        let restored: Message =
            serde_json::from_slice(&serde_json::to_vec(&measured).unwrap()).unwrap();
        assert_eq!(restored.speed_tps, Some(12.5));
        let settings = Settings::default();
        assert_eq!(
            cache_key(Path::new("model.gguf"), &settings, &[old]),
            cache_key(Path::new("model.gguf"), &settings, &[restored])
        );
    }
}

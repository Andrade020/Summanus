#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app_view;
mod backend;
mod config;
mod file_import;
mod markdown;
mod profiles;
mod storage;
mod theme;

use backend::{Event, ServerHandle};
use config::{ServerConfig, Settings};
use eframe::egui::{self, RichText};
use file_import::{AttachedFile, FilePreview};
use profiles::Recommendation;
use std::{
    fs,
    hash::{Hash, Hasher},
    path::Path,
    sync::{
        atomic::AtomicBool,
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    time::{Duration, Instant},
};
use storage::{Message, State};
use theme::{Tone, CORAL, LILAC, MINT, MUTED, SIDEBAR, TEXT};

enum ActiveGeneration {
    Chat,
    Summary {
        context_id: u64,
        draft: String,
        automatic: bool,
    },
}

struct ContextDecision {
    measured_tokens: usize,
    deadline: Option<Instant>,
    target_ctx: u32,
    can_compact: bool,
}

fn context_is_critical(used: usize, capacity: usize, max_tokens: usize) -> bool {
    let headroom = (capacity / 20).max(256).min(max_tokens.max(256));
    used >= capacity.saturating_sub(headroom)
}

fn enlarged_context(used: usize, capacity: usize, max_tokens: usize) -> u32 {
    let needed = used.saturating_add(max_tokens).saturating_add(1024);
    let target = capacity
        .saturating_mul(2)
        .max(needed)
        .next_multiple_of(8192);
    target.min(262_144) as u32
}

fn plan_auto_compaction(conv: &storage::Conversation, capacity: usize) -> Option<(usize, String)> {
    let active = conv.effective_compact_context();
    let from = active.map_or(0, |card| card.end);
    if from >= conv.messages.len() {
        return None;
    }
    let max_chars = capacity.saturating_sub(4096).saturating_mul(2).min(60_000);
    let mut excerpt = String::new();
    if let Some(card) = active {
        excerpt.push_str("Memória anterior, que deve ser preservada:\nTema: ");
        excerpt.push_str(&card.title);
        excerpt.push_str("\nAnotação: ");
        excerpt.push_str(&card.note);
        excerpt.push_str("\nResumo:\n");
        excerpt.push_str(&card.summary);
        excerpt.push_str("\n\nMensagens novas:\n");
    }
    let mut chars = excerpt.chars().count();
    let mut end = from;
    let mut new_chars = 0;
    for (index, message) in conv.messages[from..].iter().enumerate() {
        let header = format!(
            "\n[M{} · {}]\n",
            from + index + 1,
            if message.role == "user" {
                "Usuário"
            } else {
                "Assistente"
            },
        );
        let section_chars = header.chars().count() + message.content.chars().count() + 1;
        if chars + section_chars > max_chars {
            break;
        }
        excerpt.push_str(&header);
        excerpt.push_str(&message.content);
        excerpt.push('\n');
        chars += section_chars;
        new_chars += section_chars;
        end = from + index + 1;
    }
    if end == from || new_chars < 1024 {
        return None;
    }
    let prompt = format!(
        "Compacte fielmente a conversa abaixo para permitir continuar no mesmo chat. Preserve decisões, código relevante, caminhos de arquivos, resultados, preferências, pendências e incertezas. Não invente fatos. Use tópicos curtos. O histórico original permanecerá salvo e só este resumo substituirá as mensagens antigas no próximo envio.\n\n{excerpt}"
    );
    Some((end, prompt))
}

struct LocalApp {
    state: State,
    config: ServerConfig,
    server: Option<ServerHandle>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    loading: bool,
    generating: bool,
    stop: Arc<AtomicBool>,
    generation: u64,
    cache_key: Option<String>,
    input: String,
    status: String,
    settings_open: bool,
    history_query: String,
    confirm_delete: bool,
    copied_until: Option<Instant>,
    hero: Option<egui::TextureHandle>,
    contexts_open: bool,
    context_title: String,
    context_note: String,
    context_start: usize,
    context_end: usize,
    file_preview: Option<FilePreview>,
    attachments: Vec<AttachedFile>,
    active_generation: Option<ActiveGeneration>,
    context_fingerprint: u64,
    context_changed_at: Instant,
    context_estimate: usize,
    context_exact: Option<usize>,
    context_measuring: Option<u64>,
    context_failed: Option<u64>,
    math: markdown::MathRenderer,
    base_config: ServerConfig,
    recommendation: Option<Recommendation>,
    recommender_error: Option<String>,
    recommending: bool,
    profiles_open: bool,
    start_after_recommendation: bool,
    forced_model: bool,
    startup_config_error: Option<String>,
    hardware_dirty: bool,
    active_n_ctx: Option<u32>,
    pending_send: bool,
    context_decision: Option<ContextDecision>,
    auto_compact_attempts: u8,
    resume_after_reload: bool,
    resize_original: Option<ServerConfig>,
    restore_after_resize_failure: bool,
}

impl LocalApp {
    fn new(
        cc: &eframe::CreationContext<'_>,
        mut config: ServerConfig,
        forced_model: bool,
        config_error: Option<String>,
        skip_auto_model: bool,
    ) -> Self {
        #[cfg(target_os = "windows")]
        theme::apply_native_window_theme(cc);
        theme::apply(&cc.egui_ctx);
        let hero = theme::hero_texture(&cc.egui_ctx);
        let mut state = State::load();
        let base_config = config.clone();
        if !profiles::known_choice(&state.profile_choice) {
            state.profile_choice = profiles::AUTO.into();
        }
        if let Some(thinking) = config::configured_thinking() {
            state.settings.thinking = thinking;
        }
        if !forced_model && state.profile_choice == profiles::CUSTOM {
            if let Some(path) = state.last_model.as_ref().filter(|path| path.is_file()) {
                config.model = path.clone();
            }
        } else if !forced_model && !config.model.is_file() {
            if let Some(path) = state.last_model.as_ref().filter(|p| p.is_file()) {
                config.model = path.clone();
            }
        }
        let (tx, rx) = mpsc::channel();
        let mut app = Self {
            state,
            config,
            server: None,
            tx,
            rx,
            loading: false,
            generating: false,
            stop: Arc::new(AtomicBool::new(false)),
            generation: 0,
            cache_key: None,
            input: String::new(),
            status: config_error
                .as_ref()
                .map(|error| format!("Erro no config.env: {error}"))
                .unwrap_or_else(|| "Pronto para carregar um modelo".into()),
            settings_open: false,
            history_query: String::new(),
            confirm_delete: false,
            copied_until: None,
            hero,
            contexts_open: false,
            context_title: String::new(),
            context_note: String::new(),
            context_start: 1,
            context_end: 1,
            file_preview: None,
            attachments: Vec::new(),
            active_generation: None,
            context_fingerprint: 0,
            context_changed_at: Instant::now(),
            context_estimate: 0,
            context_exact: None,
            context_measuring: None,
            context_failed: None,
            math: markdown::MathRenderer::default(),
            base_config,
            recommendation: None,
            recommender_error: None,
            recommending: false,
            profiles_open: false,
            start_after_recommendation: !skip_auto_model && config_error.is_none(),
            forced_model,
            startup_config_error: config_error,
            hardware_dirty: false,
            active_n_ctx: None,
            pending_send: false,
            context_decision: None,
            auto_compact_attempts: 0,
            resume_after_reload: false,
            resize_original: None,
            restore_after_resize_failure: false,
        };
        app.start_recommender(&cc.egui_ctx);
        app
    }

    fn start_recommender(&mut self, ctx: &egui::Context) {
        self.server = None;
        self.active_n_ctx = None;
        self.recommending = true;
        self.recommendation = None;
        self.recommender_error = None;
        self.status = "Analisando RAM, VRAM e CPU para escolher o perfil…".into();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = profiles::run_recommender();
            let _ = tx.send(Event::Recommended(result));
            ctx.request_repaint();
        });
    }

    fn choose_profile(&mut self, choice: &str, ctx: &egui::Context) {
        if self.recommending || self.loading || self.generating || self.pending_send {
            return;
        }
        match profiles::config_for(
            &self.base_config,
            choice,
            self.recommendation.as_ref(),
            self.forced_model,
        ) {
            Ok(config) => {
                self.config = config;
                self.hardware_dirty = false;
                self.set_profile_choice(choice);
                let _ = self.state.save();
                self.load_model(ctx);
            }
            Err(error) => self.status = format!("Erro no perfil: {error}"),
        }
    }

    fn set_profile_choice(&mut self, choice: &str) {
        let was_patient = self.state.profile_choice == "patient";
        if was_patient && choice != "patient" {
            if let Some(previous) = self.state.patient_previous_settings.take() {
                self.state.settings = previous;
            }
        } else if choice == "patient" {
            if !was_patient && self.state.patient_previous_settings.is_none() {
                self.state.patient_previous_settings = Some(self.state.settings.clone());
            }
            self.state.settings.thinking = true;
            self.state.settings.max_tokens = self.state.settings.max_tokens.max(8192);
            self.state.settings.presence_penalty = 0.0;
        }
        self.state.profile_choice = choice.into();
    }

    fn load_model(&mut self, ctx: &egui::Context) {
        if self.loading || self.generating {
            return;
        }
        self.server = None;
        self.active_n_ctx = None;
        self.context_exact = None;
        self.context_failed = None;
        self.context_measuring = None;
        self.loading = true;
        self.status = format!(
            "Carregando {}…",
            self.config
                .model
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        );
        backend::load_model(self.config.clone(), self.tx.clone(), ctx.clone());
    }

    fn draft_content(&self) -> String {
        let mut content = self.input.trim().to_string();
        for attachment in &self.attachments {
            if !content.is_empty() {
                content.push_str("\n\n");
            }
            content.push_str(&attachment.as_prompt());
        }
        content
    }

    fn update_context_meter(&mut self, ctx: &egui::Context) {
        let mut messages = self.state.current().messages_for_request();
        let draft = self.draft_content();
        if !draft.is_empty() {
            messages.push(Message {
                role: "user".into(),
                content: draft,
                reasoning: String::new(),
                speed_tps: None,
            });
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for message in &messages {
            message.role.hash(&mut hasher);
            message.content.hash(&mut hasher);
        }
        self.state.settings.thinking.hash(&mut hasher);
        let fingerprint = hasher.finish();
        self.context_estimate = messages
            .iter()
            .map(|message| message.content.chars().count().div_ceil(3) + 12)
            .sum();
        if fingerprint != self.context_fingerprint {
            self.context_decision = None;
            self.context_fingerprint = fingerprint;
            self.context_changed_at = Instant::now();
            self.context_exact = None;
            self.context_failed = None;
            ctx.request_repaint_after(Duration::from_millis(800));
        }
        if let Some(server) = &self.server {
            if !messages.is_empty()
                && !self.generating
                && self.context_exact.is_none()
                && self.context_measuring != Some(fingerprint)
                && self.context_failed != Some(fingerprint)
                && (self.pending_send
                    || self.context_changed_at.elapsed() >= Duration::from_millis(700))
            {
                self.context_measuring = Some(fingerprint);
                backend::measure_context(
                    server.port,
                    fingerprint,
                    messages,
                    self.state.settings.thinking,
                    self.tx.clone(),
                    ctx.clone(),
                );
            }
        }
    }

    fn send(&mut self, ctx: &egui::Context) {
        if self.pending_send
            || self.generating
            || self.server.is_none()
            || self.draft_content().is_empty()
        {
            return;
        }
        self.pending_send = true;
        self.auto_compact_attempts = 0;
        self.update_context_meter(ctx);
        if let Some(tokens) = self.context_exact {
            self.resolve_send_context(tokens, ctx);
        } else {
            self.status = "Medindo o contexto real antes de enviar…".into();
            self.update_context_meter(ctx);
        }
    }

    fn resolve_send_context(&mut self, tokens: usize, ctx: &egui::Context) {
        if !self.pending_send {
            return;
        }
        let capacity = self.active_n_ctx.unwrap_or(self.config.n_ctx) as usize;
        if context_is_critical(tokens, capacity, self.state.settings.max_tokens as usize) {
            let can_compact = self.auto_compact_attempts < 3 && self.auto_compact_plan().is_some();
            if self.auto_compact_attempts > 0 && can_compact {
                self.start_auto_compact(ctx);
                return;
            }
            self.context_decision = Some(ContextDecision {
                measured_tokens: tokens,
                deadline: can_compact.then(|| Instant::now() + Duration::from_secs(30)),
                target_ctx: enlarged_context(
                    tokens,
                    capacity,
                    self.state.settings.max_tokens as usize,
                ),
                can_compact,
            });
            self.status = "Contexto realmente perto do limite; escolha como continuar.".into();
        } else {
            self.pending_send = false;
            self.context_decision = None;
            self.auto_compact_attempts = 0;
            self.send_now(ctx);
        }
    }

    fn send_now(&mut self, ctx: &egui::Context) {
        let content = self.draft_content();
        if content.is_empty() || self.generating || self.server.is_none() {
            return;
        }
        let title_source = if self.input.trim().is_empty() {
            self.attachments
                .first()
                .and_then(|file| file.path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Conversa".into())
        } else {
            self.input.clone()
        };
        self.input.clear();
        self.attachments.clear();
        let messages = {
            let conv = self.state.current_mut();
            if conv.messages.is_empty() {
                conv.title = title_source
                    .lines()
                    .next()
                    .unwrap_or("Conversa")
                    .chars()
                    .take(34)
                    .collect();
            }
            conv.messages.push(Message {
                role: "user".into(),
                content,
                reasoning: String::new(),
                speed_tps: None,
            });
            conv.messages_for_request()
        };
        self.cache_key = Some(storage::cache_key(
            &self.config.model,
            &self.state.settings,
            &messages,
        ));
        if self.state.settings.use_cache {
            if let Some(cached) = self.cache_key.as_deref().and_then(storage::read_cache) {
                self.state.current_mut().messages.push(Message {
                    role: "assistant".into(),
                    content: cached,
                    reasoning: String::new(),
                    speed_tps: None,
                });
                self.status = "Resposta do cache".into();
                let _ = self.state.save();
                return;
            }
        }
        self.state.current_mut().messages.push(Message {
            role: "assistant".into(),
            content: String::new(),
            reasoning: String::new(),
            speed_tps: None,
        });
        let _ = self.state.save();
        self.generating = true;
        self.active_generation = Some(ActiveGeneration::Chat);
        self.status = "Gerando resposta…".into();
        self.stop = Arc::new(AtomicBool::new(false));
        self.generation += 1;
        let port = self.server.as_ref().unwrap().port;
        backend::generate(
            port,
            self.generation,
            messages,
            self.state.settings.clone(),
            self.stop.clone(),
            self.tx.clone(),
            ctx.clone(),
        );
    }

    fn auto_compact_plan(&self) -> Option<(usize, String)> {
        let capacity = self.active_n_ctx.unwrap_or(self.config.n_ctx) as usize;
        plan_auto_compaction(self.state.current(), capacity)
    }

    fn start_auto_compact(&mut self, ctx: &egui::Context) {
        let Some((end, prompt)) = self.auto_compact_plan() else {
            self.context_decision = None;
            self.pending_send = false;
            self.status = "Não há histórico que caiba em uma compactação segura. Selecione um trecho menor ou aumente o contexto.".into();
            return;
        };
        let id = self.state.current_mut().add_context(
            0,
            end,
            format!("Compactação automática {}", self.auto_compact_attempts + 1),
            "Criada quando o contexto medido chegou perto do limite.".into(),
        );
        self.context_decision = None;
        self.auto_compact_attempts += 1;
        let mut settings = self.state.settings.clone();
        settings.temperature = 0.2;
        settings.thinking = false;
        settings.max_tokens = 2048;
        settings.use_cache = false;
        self.generating = true;
        self.active_generation = Some(ActiveGeneration::Summary {
            context_id: id,
            draft: String::new(),
            automatic: true,
        });
        self.stop = Arc::new(AtomicBool::new(false));
        self.generation += 1;
        self.status = "Compactando o histórico antes do envio…".into();
        backend::generate(
            self.server.as_ref().unwrap().port,
            self.generation,
            vec![Message {
                role: "user".into(),
                content: prompt,
                reasoning: String::new(),
                speed_tps: None,
            }],
            settings,
            self.stop.clone(),
            self.tx.clone(),
            ctx.clone(),
        );
    }

    fn increase_context_for_pending_send(&mut self, target_ctx: u32, ctx: &egui::Context) {
        let current = self.active_n_ctx.unwrap_or(self.config.n_ctx);
        self.context_decision = None;
        if target_ctx <= current {
            self.pending_send = false;
            self.status =
                "O contexto já está no limite configurável. Escolha um trecho menor.".into();
            return;
        }
        self.resize_original = Some(self.config.clone());
        self.config.n_ctx = target_ctx;
        self.set_profile_choice(profiles::CUSTOM);
        self.hardware_dirty = false;
        let _ = self.state.save();
        self.resume_after_reload = true;
        self.status = format!("Ampliando contexto para {}K…", target_ctx / 1024);
        self.load_model(ctx);
    }

    fn summarize_context(&mut self, id: u64, ctx: &egui::Context) {
        if self.generating || self.pending_send || self.server.is_none() {
            return;
        }
        let conv = self.state.current();
        let Some(card) = conv.contexts.iter().find(|card| card.id == id) else {
            return;
        };
        if card.start >= card.end || card.end > conv.messages.len() {
            self.status = "O intervalo deste recorte não é válido.".into();
            return;
        }
        let mut excerpt = String::new();
        for (index, message) in conv.messages[card.start..card.end].iter().enumerate() {
            excerpt.push_str(&format!(
                "\n[M{} · {}]\n{}\n",
                card.start + index + 1,
                if message.role == "user" {
                    "Usuário"
                } else {
                    "Assistente"
                },
                message.content
            ));
        }
        let max_chars = (self
            .active_n_ctx
            .unwrap_or(self.config.n_ctx)
            .saturating_sub(2048) as usize
            * 3)
        .min(60_000);
        if excerpt.chars().count() > max_chars {
            self.status = format!(
                "Recorte grande demais para resumir com segurança. Selecione menos mensagens (limite aproximado: {max_chars} caracteres)."
            );
            return;
        }
        let prompt = format!(
            "Crie uma memória compacta e fiel deste trecho de conversa. Preserve decisões, nomes, caminhos, funções, resultados, pendências e incertezas relevantes. Não invente fatos. Organize em tópicos curtos.\nTítulo: {}\nAnotação do usuário: {}\nTrecho:\n{}",
            card.title, card.note, excerpt
        );
        let mut settings = self.state.settings.clone();
        settings.temperature = 0.2;
        settings.thinking = false;
        settings.max_tokens = 1024;
        settings.use_cache = false;
        let messages = vec![Message {
            role: "user".into(),
            content: prompt,
            reasoning: String::new(),
            speed_tps: None,
        }];
        self.generating = true;
        self.active_generation = Some(ActiveGeneration::Summary {
            context_id: id,
            draft: String::new(),
            automatic: false,
        });
        self.stop = Arc::new(AtomicBool::new(false));
        self.generation += 1;
        self.status = "Criando resumo do contexto…".into();
        backend::generate(
            self.server.as_ref().unwrap().port,
            self.generation,
            messages,
            settings,
            self.stop.clone(),
            self.tx.clone(),
            ctx.clone(),
        );
    }

    fn poll(&mut self, ctx: &egui::Context) {
        let mut should_save = false;
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Recommended(result) => {
                    self.recommending = false;
                    match result {
                        Ok(recommendation) => {
                            self.recommendation = Some(recommendation);
                            self.recommender_error = None;
                            self.status =
                                "Recomendação pronta. Escolha um perfil quando quiser.".into();
                        }
                        Err(error) => {
                            self.recommendation = None;
                            self.recommender_error = Some(error.clone());
                            self.status = format!("Recomendação indisponível: {error}");
                        }
                    }
                    let choice = self.state.profile_choice.clone();
                    if choice == "patient" {
                        self.set_profile_choice("patient");
                        let _ = self.state.save();
                    }
                    if let Some(error) = &self.startup_config_error {
                        self.start_after_recommendation = false;
                        self.status = format!("Erro no config.env: {error}");
                        continue;
                    }
                    if choice == profiles::CUSTOM {
                        if self.start_after_recommendation {
                            self.start_after_recommendation = false;
                            self.load_model(ctx);
                        }
                    } else {
                        match profiles::config_for(
                            &self.base_config,
                            &choice,
                            self.recommendation.as_ref(),
                            self.forced_model,
                        ) {
                            Ok(config) => {
                                self.config = config;
                                self.hardware_dirty = false;
                                if self.start_after_recommendation {
                                    self.start_after_recommendation = false;
                                    self.load_model(ctx);
                                }
                            }
                            Err(_)
                                if self.recommender_error.is_some() && choice == profiles::AUTO =>
                            {
                                self.config = self.base_config.clone();
                                self.hardware_dirty = false;
                                if self.start_after_recommendation {
                                    self.start_after_recommendation = false;
                                    self.load_model(ctx);
                                }
                            }
                            Err(error) => {
                                self.start_after_recommendation = false;
                                self.status = format!("Perfil indisponível: {error}");
                            }
                        }
                    }
                }
                Event::Loaded(result) => {
                    self.loading = false;
                    match result {
                        Ok(server) => {
                            self.server = Some(server);
                            self.active_n_ctx = Some(self.config.n_ctx);
                            self.context_fingerprint = 0;
                            self.context_exact = None;
                            self.context_failed = None;
                            self.context_changed_at = Instant::now();
                            self.status = "Modelo carregado. Pronto para conversar.".into();
                            if self.restore_after_resize_failure {
                                self.restore_after_resize_failure = false;
                                if self.pending_send {
                                    self.start_auto_compact(ctx);
                                }
                            } else if self.resume_after_reload {
                                self.resume_after_reload = false;
                                self.resize_original = None;
                                self.update_context_meter(ctx);
                            } else {
                                self.resize_original = None;
                            }
                        }
                        Err(error) => {
                            if let Some(original) = self.resize_original.take() {
                                self.config = original;
                                self.resume_after_reload = false;
                                self.restore_after_resize_failure = true;
                                self.status = format!(
                                    "Novo contexto não carregou ({error}); restaurando o anterior…"
                                );
                                self.load_model(ctx);
                            } else {
                                self.pending_send = false;
                                self.restore_after_resize_failure = false;
                                self.status = format!("Erro: {error}");
                            }
                        }
                    }
                }
                Event::ContextMeasured {
                    fingerprint,
                    result,
                } => {
                    if self.context_measuring == Some(fingerprint) {
                        self.context_measuring = None;
                    }
                    if fingerprint == self.context_fingerprint {
                        match result {
                            Ok(tokens) => {
                                self.context_exact = Some(tokens);
                                if self.pending_send
                                    && self.context_decision.is_none()
                                    && !self.generating
                                    && !self.loading
                                {
                                    self.resolve_send_context(tokens, ctx);
                                }
                            }
                            Err(error) => {
                                self.context_failed = Some(fingerprint);
                                if self.pending_send {
                                    self.pending_send = false;
                                    self.status = format!("Não consegui medir o contexto real; envio cancelado: {error}");
                                }
                            }
                        }
                    }
                }
                Event::Delta {
                    generation,
                    content,
                    reasoning,
                } if generation == self.generation && self.generating => {
                    match self.active_generation.as_mut() {
                        Some(ActiveGeneration::Chat) => {
                            if let Some(message) = self.state.current_mut().messages.last_mut() {
                                message.content.push_str(&content);
                                message.reasoning.push_str(&reasoning);
                            }
                        }
                        Some(ActiveGeneration::Summary { draft, .. }) => draft.push_str(&content),
                        None => {}
                    }
                }
                Event::Finished {
                    generation,
                    result,
                    stopped,
                } if generation == self.generation => {
                    self.generating = false;
                    match self.active_generation.take() {
                        Some(ActiveGeneration::Chat) => {
                            let answer = self
                                .state
                                .current()
                                .messages
                                .last()
                                .map(|m| m.content.clone())
                                .unwrap_or_default();
                            if stopped {
                                self.status = "Geração interrompida".into();
                            } else if let Err(error) = &result {
                                self.status = format!("Erro na geração: {error}");
                            } else {
                                if let Ok(Some(rate)) = &result {
                                    if rate.is_finite() && *rate > 0.0 {
                                        if let Some(message) =
                                            self.state.current_mut().messages.last_mut()
                                        {
                                            message.speed_tps = Some(*rate);
                                        }
                                    }
                                }
                                self.status = "Pronto".into();
                                if self.state.settings.use_cache && !answer.is_empty() {
                                    if let Some(key) = &self.cache_key {
                                        storage::write_cache(key, &answer);
                                    }
                                }
                            }
                            let has_reasoning = self
                                .state
                                .current()
                                .messages
                                .last()
                                .is_some_and(|m| !m.reasoning.is_empty());
                            if answer.is_empty() && !has_reasoning {
                                self.state.current_mut().messages.pop();
                            }
                            self.cache_key = None;
                            should_save = true;
                        }
                        Some(ActiveGeneration::Summary {
                            context_id,
                            draft,
                            automatic,
                        }) => {
                            let mut saved = false;
                            if stopped {
                                self.status = "Resumo interrompido; o anterior foi mantido".into();
                            } else if let Err(error) = &result {
                                self.status = format!("Erro no resumo: {error}");
                            } else if draft.trim().is_empty() {
                                self.status = "O modelo não retornou um resumo".into();
                            } else if let Some(card) = self
                                .state
                                .current_mut()
                                .contexts
                                .iter_mut()
                                .find(|card| card.id == context_id)
                            {
                                card.summary = draft.trim().to_string();
                                self.status = if automatic {
                                    "Histórico compactado; conferindo o espaço antes de enviar…"
                                        .into()
                                } else {
                                    "Resumo salvo no cartão de contexto".into()
                                };
                                should_save = true;
                                saved = true;
                            }
                            if automatic {
                                if saved {
                                    let conv = self.state.current_mut();
                                    conv.compact_context_id = Some(context_id);
                                    conv.use_full_history = false;
                                    self.context_fingerprint = 0;
                                    self.context_exact = None;
                                    self.context_failed = None;
                                    self.update_context_meter(ctx);
                                } else {
                                    self.state
                                        .current_mut()
                                        .contexts
                                        .retain(|card| card.id != context_id);
                                    self.pending_send = false;
                                    self.auto_compact_attempts = 0;
                                    should_save = true;
                                }
                            }
                        }
                        None => {}
                    }
                }
                _ => {}
            }
        }
        if should_save {
            let _ = self.state.save();
        }
    }

    fn export(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Markdown", &["md"])
            .add_filter("JSON", &["json"])
            .add_filter("Texto", &["txt"])
            .set_file_name("conversa.md")
            .save_file()
        else {
            return;
        };
        let conv = self.state.current();
        let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("md");
        let body = if extension.eq_ignore_ascii_case("json") {
            serde_json::to_string_pretty(conv).unwrap_or_default()
        } else {
            let mut body = if extension.eq_ignore_ascii_case("md") {
                format!("# {}\n\n", conv.title)
            } else {
                format!("{}\n\n", conv.title)
            };
            for message in &conv.messages {
                let role = if message.role == "user" {
                    "Você"
                } else {
                    "Assistente"
                };
                if extension.eq_ignore_ascii_case("md") {
                    body.push_str(&format!("## {role}\n\n{}\n\n", message.content));
                } else {
                    body.push_str(&format!("{role}: {}\n\n", message.content));
                }
            }
            if !conv.contexts.is_empty() {
                body.push_str(if extension.eq_ignore_ascii_case("md") {
                    "## Mapa de contextos\n\n"
                } else {
                    "Mapa de contextos\n\n"
                });
                for card in &conv.contexts {
                    body.push_str(&format!(
                        "{} (mensagens {}–{})\nAnotação: {}\nResumo: {}\n\n",
                        card.title,
                        card.start + 1,
                        card.end,
                        card.note,
                        card.summary
                    ));
                }
            }
            body
        };
        match fs::write(&path, body) {
            Ok(()) => self.status = format!("Exportado: {}", path.display()),
            Err(error) => self.status = format!("Erro ao exportar: {error}"),
        }
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .exact_width(264.0)
            .frame(
                egui::Frame::none()
                    .fill(SIDEBAR)
                    .inner_margin(egui::Margin::same(18.0)),
            )
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    theme::brand_mark(ui, 44.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("SUMMANUS").size(24.0).strong().color(TEXT));
                        ui.label(RichText::new("SUA IA, POR PERTO").size(10.0).color(LILAC));
                    });
                });
                ui.add_space(26.0);
                if ui
                    .add_enabled_ui(!self.generating && !self.pending_send, |ui| {
                        theme::button(
                            ui,
                            "+  Nova conversa",
                            Tone::Primary,
                            egui::vec2(ui.available_width(), 43.0),
                        )
                    })
                    .inner
                    .clicked()
                {
                    self.state.new_conversation();
                    let _ = self.state.save();
                }
                ui.add_space(14.0);
                ui.add_sized(
                    [ui.available_width(), 38.0],
                    egui::TextEdit::singleline(&mut self.history_query)
                        .hint_text("Buscar conversas"),
                );
                ui.add_space(22.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("CONVERSAS").size(11.0).strong().color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(self.state.conversations.len().to_string())
                                .size(11.0)
                                .color(LILAC),
                        );
                    });
                });
                ui.add_space(4.0);
                let query = self.history_query.trim().to_lowercase();
                let mut select = None;
                let list_height = (ui.available_height() - 136.0).max(100.0);
                let (list_rect, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), list_height),
                    egui::Sense::hover(),
                );
                let mut list_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(list_rect)
                        .layout(egui::Layout::top_down(egui::Align::LEFT)),
                );
                egui::ScrollArea::vertical().show(&mut list_ui, |ui| {
                    let mut matches = 0;
                    for conv in &self.state.conversations {
                        if !conv.title.to_lowercase().contains(&query) {
                            continue;
                        }
                        matches += 1;
                        let selected = conv.id == self.state.current_id;
                        let title: String = conv.title.chars().take(25).collect();
                        let label = format!("{}  {}", if selected { "●" } else { "·" }, title);
                        let fill = if selected {
                            egui::Color32::from_rgb(62, 51, 88)
                        } else {
                            SIDEBAR
                        };
                        let color = if selected { TEXT } else { MUTED };
                        let response = ui.add_sized(
                            [ui.available_width(), 38.0],
                            egui::Button::new(RichText::new(label).size(13.0).color(color))
                                .fill(fill)
                                .stroke(egui::Stroke::NONE)
                                .rounding(egui::Rounding::same(10.0)),
                        );
                        if response.clicked() && !self.generating && !self.pending_send {
                            select = Some(conv.id);
                        }
                        response.on_hover_text(&conv.title);
                    }
                    if matches == 0 {
                        ui.label(
                            RichText::new("Nenhuma conversa encontrada")
                                .size(12.0)
                                .color(MUTED),
                        );
                    }
                });
                if let Some(id) = select {
                    self.state.current_id = id;
                    let _ = self.state.save();
                }
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if theme::button(ui, "↗  Exportar", Tone::Quiet, egui::vec2(105.0, 36.0))
                        .clicked()
                    {
                        self.export();
                    }
                    if ui
                        .add_enabled_ui(!self.generating && !self.pending_send, |ui| {
                            theme::button(ui, "Excluir", Tone::Danger, egui::vec2(78.0, 36.0))
                        })
                        .inner
                        .clicked()
                    {
                        self.confirm_delete = true;
                    }
                });
                ui.add_space(12.0);
                ui.label(
                    RichText::new("●  LOCAL E PRIVADO")
                        .size(10.0)
                        .strong()
                        .color(MINT),
                );
                ui.label(
                    RichText::new("Suas conversas ficam neste computador.")
                        .size(11.0)
                        .color(MUTED),
                );
            });
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = self.settings_open;
        let mut changed = false;
        let mut reload = false;
        egui::Window::new("Preferências")
            .open(&mut open)
            .default_width(440.0)
            .show(ctx, |ui| {
                if self.pending_send {
                    ui.disable();
                }
                ui.label(
                    RichText::new("Seu jeito de conversar")
                        .size(21.0)
                        .strong()
                        .color(TEXT),
                );
                ui.label(
                    RichText::new("Ajuste o estilo das respostas quando quiser.")
                        .size(12.0)
                        .color(MUTED),
                );
                ui.add_space(14.0);
                ui.label(RichText::new("RESPOSTAS").size(11.0).strong().color(CORAL));
                let s: &mut Settings = &mut self.state.settings;
                changed |= ui
                    .add(egui::Slider::new(&mut s.temperature, 0.0..=2.0).text("Temperatura"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut s.top_p, 0.05..=1.0).text("Top P"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut s.repeat_penalty, 1.0..=2.0).text("Repetição"))
                    .changed();
                changed |= ui
                    .add(
                        egui::Slider::new(&mut s.presence_penalty, -2.0..=2.0)
                            .text("Presence penalty"),
                    )
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut s.max_tokens, 64..=16384).text("Máx. tokens"))
                    .changed();
                changed |= ui.checkbox(&mut s.thinking, "Modo raciocínio").changed();
                changed |= ui
                    .checkbox(&mut s.use_cache, "Cache de respostas")
                    .changed();
                ui.add_space(16.0);
                ui.separator();
                egui::CollapsingHeader::new(
                    RichText::new("Hardware e contexto  ·  avançado").color(LILAC),
                )
                .default_open(false)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Essas mudanças entram em vigor ao recarregar o modelo.")
                            .size(12.0)
                            .color(MUTED),
                    );
                    ui.horizontal(|ui| {
                        ui.label("Contexto");
                        self.hardware_dirty |= ui
                            .add(egui::DragValue::new(&mut self.config.n_ctx).range(512..=262144))
                            .changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label("Threads");
                        self.hardware_dirty |= ui
                            .add(egui::DragValue::new(&mut self.config.n_threads).range(0..=128))
                            .changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label("Threads do prompt");
                        self.hardware_dirty |= ui
                            .add(
                                egui::DragValue::new(&mut self.config.n_threads_batch)
                                    .range(0..=128),
                            )
                            .changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label("Camadas GPU");
                        self.hardware_dirty |= ui
                            .add(egui::DragValue::new(&mut self.config.n_gpu_layers).range(0..=999))
                            .changed();
                    });
                    ui.horizontal(|ui| {
                        ui.label("Camadas MoE na CPU");
                        self.hardware_dirty |= ui
                            .add(egui::DragValue::new(&mut self.config.n_cpu_moe).range(0..=999))
                            .changed();
                    });
                    if ui
                        .add_enabled_ui(!self.generating && !self.loading, |ui| {
                            theme::button(
                                ui,
                                "Recarregar modelo",
                                Tone::Secondary,
                                egui::vec2(180.0, 38.0),
                            )
                        })
                        .inner
                        .clicked()
                    {
                        reload = true;
                    }
                });
            });
        self.settings_open = open;
        if changed {
            let _ = self.state.save();
        }
        if reload {
            if self.hardware_dirty {
                self.set_profile_choice(profiles::CUSTOM);
                self.hardware_dirty = false;
                let _ = self.state.save();
            }
            self.load_model(ctx);
        }
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let config_path = args
        .windows(2)
        .find(|w| w[0] == "--config")
        .map(|w| w[1].clone())
        .unwrap_or_else(|| "config.env".into());
    if config_path == "config.env" && !Path::new(&config_path).is_file() {
        if let Ok(executable) = std::env::current_exe() {
            if let Some(root) = executable
                .ancestors()
                .find(|directory| directory.join(&config_path).is_file())
            {
                let _ = std::env::set_current_dir(root);
            }
        }
    }
    let config_error = dotenvy::from_path(&config_path)
        .err()
        .filter(|_| Path::new(&config_path).is_file())
        .map(|error| error.to_string());
    let model_override = args
        .windows(2)
        .find(|w| w[0] == "--model")
        .map(|w| w[1].clone());
    let forced_model = model_override.is_some();
    let skip_auto_model = args.iter().any(|arg| arg == "--no-auto-model");
    let config = ServerConfig::from_env(model_override);
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1100.0, 760.0])
        .with_min_inner_size([760.0, 520.0])
        .with_title("Summanus · IA local");
    if let Some(icon) = theme::window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Summanus · IA local",
        options,
        Box::new(move |cc| {
            Ok(Box::new(LocalApp::new(
                cc,
                config,
                forced_model,
                config_error,
                skip_auto_model,
            )))
        }),
    )
}

#[cfg(test)]
mod context_policy_tests {
    use super::{context_is_critical, enlarged_context, plan_auto_compaction};
    use crate::storage::{Conversation, Message};

    fn message(content: String) -> Message {
        Message {
            role: "user".into(),
            content,
            reasoning: String::new(),
            speed_tps: None,
        }
    }

    #[test]
    fn only_measured_near_full_context_triggers_the_decision() {
        assert!(!context_is_critical(30_000, 32_768, 4096));
        assert!(context_is_critical(31_300, 32_768, 4096));
        assert!(context_is_critical(32_768, 32_768, 4096));
        assert_eq!(enlarged_context(31_300, 32_768, 4096), 65_536);
    }

    #[test]
    fn compaction_keeps_whole_messages_and_previous_memory() {
        let mut conv = Conversation {
            id: 1,
            title: "Teste".into(),
            messages: vec![
                message("a".repeat(3_000)),
                message("b".repeat(3_000)),
                message("c".repeat(20_000)),
            ],
            contexts: vec![],
            compact_context_id: None,
            use_full_history: false,
        };
        let (end, prompt) = plan_auto_compaction(&conv, 8_192).unwrap();
        assert_eq!(end, 2);
        assert!(prompt.contains(&"a".repeat(3_000)));
        assert!(prompt.contains(&"b".repeat(3_000)));
        assert!(!prompt.contains(&"c".repeat(20_000)));
        let id = conv.add_context(0, 1, "Memória".into(), String::new());
        conv.contexts[0].summary = "decisão importante".into();
        conv.compact_context_id = Some(id);
        let (end, prompt) = plan_auto_compaction(&conv, 8_192).unwrap();
        assert_eq!(end, 2);
        assert!(prompt.contains("decisão importante"));
        assert!(prompt.contains(&"b".repeat(3_000)));
        assert!(!prompt.contains(&"a".repeat(3_000)));
    }
}

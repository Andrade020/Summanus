use crate::{markdown, profiles, theme, LocalApp};
use eframe::egui::{self, Color32, RichText, Stroke};
use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};
use theme::{Tone, BG, CORAL, LILAC, MINT, MUTED, OUTLINE, RED, SURFACE, TEXT};

impl LocalApp {
    fn render_header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header")
            .frame(
                egui::Frame::none()
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(24.0, 15.0)),
            )
            .show(ctx, |ui| {
                let compact = ui.available_width() < 580.0;
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("SEU ESPAÇO DE IDEIAS")
                                .size(10.0)
                                .strong()
                                .color(LILAC),
                        );
                        let title = &self.state.current().title;
                        let short: String =
                            title.chars().take(if compact { 22 } else { 38 }).collect();
                        ui.label(RichText::new(short).size(22.0).strong().color(TEXT))
                            .on_hover_text(title);
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if theme::button(
                            ui,
                            if compact { "Ajustes" } else { "⚙  Ajustes" },
                            Tone::Quiet,
                            egui::vec2(if compact { 78.0 } else { 100.0 }, 37.0),
                        )
                        .clicked()
                        {
                            self.settings_open = true;
                        }
                        let profile_label = if self.recommending {
                            "Analisando…".to_owned()
                        } else if self.state.profile_choice == profiles::AUTO {
                            let suggested = self
                                .recommendation
                                .as_ref()
                                .and_then(|recommendation| recommendation.recomendada.as_ref())
                                .map(|line| match line.id.as_str() {
                                    "c16k" => "A",
                                    "c32k+saidaCPU" => "B",
                                    "c32k" => "C",
                                    other => other,
                                })
                                .unwrap_or("padrão");
                            format!("Auto · {suggested}")
                        } else {
                            profiles::PROFILES
                                .iter()
                                .find(|profile| profile.id == self.state.profile_choice)
                                .map(|profile| profile.name.to_owned())
                                .unwrap_or_else(|| "Personalizado".into())
                        };
                        if theme::button(
                            ui,
                            if compact { "Perfis" } else { &profile_label },
                            Tone::Secondary,
                            egui::vec2(if compact { 82.0 } else { 120.0 }, 37.0),
                        )
                        .clicked()
                        {
                            self.profiles_open = true;
                        }
                        let context_label =
                            if self.state.current().effective_compact_context().is_some() {
                                "Contextos ●"
                            } else {
                                "Contextos"
                            };
                        if theme::button(
                            ui,
                            if compact { "Contextos" } else { context_label },
                            Tone::Secondary,
                            egui::vec2(if compact { 92.0 } else { 110.0 }, 37.0),
                        )
                        .clicked()
                        {
                            self.open_contexts();
                        }
                    });
                });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let (color, status) = if self.recommending {
                        (LILAC, "Analisando PC")
                    } else if self.loading {
                        (CORAL, "Carregando modelo")
                    } else if self.server.is_some() {
                        (MINT, "Modelo pronto")
                    } else if self.status.starts_with("Erro") {
                        (RED, "Atenção necessária")
                    } else {
                        (MUTED, "Sem modelo")
                    };
                    egui::Frame::none()
                        .fill(SURFACE)
                        .rounding(egui::Rounding::same(12.0))
                        .inner_margin(egui::Margin::symmetric(11.0, 6.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("●").color(color).size(11.0));
                                ui.label(RichText::new(status).color(color).size(11.0).strong());
                            });
                        });
                    let name = self
                        .config
                        .model
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy();
                    let short: String = name.chars().take(if compact { 16 } else { 35 }).collect();
                    ui.label(
                        RichText::new(if short.is_empty() {
                            "Escolha um GGUF"
                        } else {
                            &short
                        })
                        .size(11.0)
                        .color(MUTED),
                    )
                    .on_hover_text(name.as_ref());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can_change = !self.generating && !self.loading && !self.pending_send;
                        let clicked = ui
                            .add_enabled_ui(can_change, |ui| {
                                theme::button(
                                    ui,
                                    "Trocar modelo",
                                    Tone::Secondary,
                                    egui::vec2(128.0, 34.0),
                                )
                            })
                            .inner
                            .clicked();
                        if clicked {
                            if let Some(path) = rfd::FileDialog::new()
                                .add_filter("Modelos GGUF", &["gguf"])
                                .pick_file()
                            {
                                self.state.last_model = Some(path.clone());
                                self.set_profile_choice(profiles::CUSTOM);
                                let _ = self.state.save();
                                self.config.model = path;
                                self.load_model(ctx);
                            }
                        }
                    });
                });
            });
    }

    fn render_composer(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("composer")
            .frame(
                egui::Frame::none()
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(24.0, 12.0)),
            )
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(SURFACE)
                    .stroke(Stroke::new(1.0_f32, OUTLINE))
                    .rounding(egui::Rounding::same(16.0))
                    .inner_margin(egui::Margin::same(14.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("SUA MENSAGEM")
                                .size(10.0)
                                .strong()
                                .color(LILAC),
                        );
                        let mut remove_attachment = None;
                        for (index, attachment) in self.attachments.iter().enumerate() {
                            ui.horizontal(|ui| {
                                let name = attachment
                                    .path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy();
                                ui.label(
                                    RichText::new(format!(
                                        "Arquivo: {} · {} · {} linhas{}",
                                        name,
                                        attachment.label,
                                        attachment.line_count,
                                        if attachment.partial {
                                            " · trecho selecionado"
                                        } else {
                                            ""
                                        }
                                    ))
                                    .size(11.0)
                                    .color(MINT),
                                );
                                if ui.add_enabled(!self.pending_send, egui::Button::new("Remover")).clicked() {
                                    remove_attachment = Some(index);
                                }
                            });
                        }
                        if let Some(index) = remove_attachment {
                            self.attachments.remove(index);
                        }
                        let hint = if self.server.is_some() {
                            "Escreva uma pergunta ou ideia…"
                        } else {
                            "Escolha ou aguarde um modelo para começar…"
                        };
                        let composer_response = ui.add_enabled_ui(!self.pending_send, |ui| {
                            ui.add_sized(
                                [ui.available_width(), 64.0],
                                egui::TextEdit::multiline(&mut self.input)
                                    .hint_text(hint)
                                    .text_color(TEXT)
                                    .desired_rows(3)
                                    .frame(false),
                            )
                        }).inner;
                        let keyboard_send = composer_response.has_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.ctrl);
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.add_enabled_ui(!self.pending_send, |ui| {
                                theme::button(ui, "+ Arquivo", Tone::Quiet, egui::vec2(98.0, 37.0))
                            }).inner.clicked()
                            {
                                self.open_file_import();
                            }
                            if ui.add_enabled_ui(!self.pending_send, |ui| {
                                ui.checkbox(
                                    &mut self.state.settings.thinking,
                                    RichText::new("Raciocínio").color(TEXT),
                                )
                            }).inner.changed()
                            {
                                let _ = self.state.save();
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if self.generating {
                                        if theme::button(
                                            ui,
                                            "■  Parar",
                                            Tone::Danger,
                                            egui::vec2(100.0, 37.0),
                                        )
                                        .clicked()
                                        {
                                            self.stop.store(true, Ordering::Relaxed);
                                            self.status = "Interrompendo…".into();
                                        }
                                    } else if self.pending_send {
                                        if theme::button(ui, "Cancelar envio", Tone::Quiet, egui::vec2(135.0, 37.0)).clicked() {
                                            self.pending_send = false;
                                            self.context_decision = None;
                                            self.resume_after_reload = false;
                                            self.status = "Envio cancelado; texto e anexos preservados.".into();
                                        }
                                    } else {
                                        let enabled = self.server.is_some()
                                            && (!self.input.trim().is_empty()
                                                || !self.attachments.is_empty());
                                        let clicked = ui
                                            .add_enabled_ui(enabled, |ui| {
                                                theme::button(
                                                    ui,
                                                    "Enviar · Ctrl+Enter",
                                                    Tone::Primary,
                                                    egui::vec2(166.0, 37.0),
                                                )
                                            })
                                            .inner
                                            .clicked();
                                        if clicked || keyboard_send {
                                            self.send(ctx);
                                        }
                                    }
                                },
                            );
                        });
                    });
                ui.add_space(5.0);
                let used = self.context_exact.unwrap_or(self.context_estimate);
                let capacity = self.active_n_ctx.unwrap_or(self.config.n_ctx).max(1) as usize;
                let reserve = self.state.settings.max_tokens as usize;
                let remaining = capacity.saturating_sub(used);
                let meter_color = if used >= capacity || remaining < reserve {
                    RED
                } else if used * 5 >= capacity * 4 {
                    CORAL
                } else {
                    MINT
                };
                let source = if self.context_exact.is_some() {
                    "medido"
                } else if self.context_measuring.is_some() {
                    "medindo…"
                } else {
                    "estimado"
                };
                ui.label(
                    RichText::new(format!(
                        "Contexto: {used} / {capacity} tokens ({source})  ·  resposta reservada: até {reserve}"
                    ))
                    .size(11.0)
                    .color(meter_color),
                );
                ui.add(
                    egui::ProgressBar::new((used as f32 / capacity as f32).clamp(0.0, 1.0))
                        .fill(meter_color)
                        .desired_width(ui.available_width()),
                );
                ui.add_space(7.0);
                let speed = self
                    .state
                    .current()
                    .messages
                    .last()
                    .and_then(|message| message.speed_tps);
                let measuring = self.generating
                    && matches!(self.active_generation, Some(crate::ActiveGeneration::Chat));
                egui::Frame::none()
                    .fill(SURFACE)
                    .rounding(egui::Rounding::same(10.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 5.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("VELOCIDADE").size(10.0).strong().color(LILAC));
                            if measuring {
                                ui.spinner();
                                ui.label(RichText::new("medindo…").size(12.0).color(MUTED));
                            } else if let Some(rate) = speed {
                                ui.label(RichText::new(format!("{rate:.1} tokens/s")).size(13.0).strong().color(MINT));
                            } else {
                                ui.label(RichText::new("— tokens/s").size(12.0).color(MUTED));
                            }
                        });
                    })
                    .response
                    .on_hover_text("Taxa real de geração informada pelo llama-server ao concluir a última resposta desta conversa.");
                let copied = self
                    .copied_until
                    .is_some_and(|until| until > Instant::now());
                let status = if copied {
                    "✓  Copiado para a área de transferência".to_string()
                } else {
                    self.status.lines().next().unwrap_or("").to_string()
                };
                ui.label(
                    RichText::new(status)
                        .size(11.0)
                        .color(if copied { MINT } else { MUTED }),
                )
                .on_hover_text(&self.status);
            });
    }

    fn render_empty(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(10.0);
            ui.vertical_centered(|ui| {
                if let Some(hero) = &self.hero {
                    ui.add(egui::Image::new((hero.id(), egui::vec2(178.0, 178.0))));
                } else {
                    theme::brand_mark(ui, 85.0);
                }
                ui.label(
                    RichText::new("Acenda uma ideia.")
                        .size(28.0)
                        .strong()
                        .color(TEXT),
                );
                ui.label(
                    RichText::new("Um espaço tranquilo para pensar, criar e descobrir.")
                        .size(13.0)
                        .color(MUTED),
                );
                ui.add_space(22.0);
                ui.label(
                    RichText::new("COMECE POR AQUI")
                        .size(10.0)
                        .strong()
                        .color(LILAC),
                );
                ui.add_space(4.0);
                let prompt_width = 3.0 * 130.0 + 2.0 * ui.spacing().item_spacing.x;
                let leading_space = ((ui.available_width() - prompt_width) / 2.0).max(0.0);
                ui.horizontal(|ui| {
                    ui.add_space(leading_space);
                    if theme::button(ui, "✦  Explorar", Tone::Quiet, egui::vec2(130.0, 39.0))
                        .clicked()
                    {
                        self.input = "Me ajude a explorar uma ideia passo a passo.".into();
                    }
                    if theme::button(ui, "✎  Escrever", Tone::Quiet, egui::vec2(130.0, 39.0))
                        .clicked()
                    {
                        self.input =
                            "Me ajude a escrever um texto claro e envolvente sobre ".into();
                    }
                    if theme::button(ui, "⌘  Programar", Tone::Quiet, egui::vec2(130.0, 39.0))
                        .clicked()
                    {
                        self.input = "Me ajude a resolver este problema de programação: ".into();
                    }
                });
            });
        });
    }

    fn render_chat(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                ui.add_space(4.0);
                for message in &self.state.current().messages {
                    let is_user = message.role == "user";
                    let fill = if is_user {
                        Color32::from_rgb(47, 39, 68)
                    } else {
                        SURFACE
                    };
                    let border = if is_user {
                        Color32::from_rgb(91, 73, 123)
                    } else {
                        OUTLINE
                    };
                    egui::Frame::none()
                        .fill(fill)
                        .stroke(Stroke::new(1.0_f32, border))
                        .rounding(egui::Rounding::same(15.0))
                        .inner_margin(egui::Margin::same(16.0))
                        .outer_margin(egui::Margin::symmetric(0.0, 7.0))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(if is_user { "VOCÊ" } else { "SUMMANUS" })
                                        .size(11.0)
                                        .strong()
                                        .color(if is_user { LILAC } else { CORAL }),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if !message.content.is_empty()
                                            && theme::button(
                                                ui,
                                                "Copiar",
                                                Tone::Quiet,
                                                egui::vec2(72.0, 29.0),
                                            )
                                            .clicked()
                                        {
                                            ctx.output_mut(|output| {
                                                output.copied_text = message.content.clone()
                                            });
                                            self.copied_until =
                                                Some(Instant::now() + Duration::from_secs(2));
                                            ctx.request_repaint_after(Duration::from_secs(2));
                                        }
                                    },
                                );
                            });
                            ui.add_space(6.0);
                            if !message.reasoning.is_empty() {
                                egui::CollapsingHeader::new(
                                    RichText::new("Raciocínio").color(MUTED),
                                )
                                .default_open(false)
                                .show(ui, |ui| {
                                    markdown::render(
                                        ui,
                                        ctx,
                                        &message.reasoning,
                                        MUTED,
                                        &mut self.math,
                                    )
                                });
                            }
                            if message.content.is_empty() {
                                ui.spinner();
                            } else {
                                markdown::render(ui, ctx, &message.content, TEXT, &mut self.math);
                            }
                            if let Some(rate) = message.speed_tps {
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new(format!("◉  {rate:.1} tokens/s"))
                                        .size(11.0)
                                        .color(MINT),
                                )
                                .on_hover_text(
                                    "Velocidade real desta resposta, informada pelo llama-server.",
                                );
                            }
                        });
                }
                ui.add_space(6.0);
            });
    }

    fn open_contexts(&mut self) {
        let conv = self.state.current();
        let count = conv.messages.len();
        self.context_start = 1;
        self.context_end = count.max(1);
        self.contexts_open = true;
    }

    fn render_profiles_window(&mut self, ctx: &egui::Context) {
        if !self.profiles_open {
            return;
        }
        let mut open = true;
        let mut selected = None;
        let mut recheck = false;
        let can_switch =
            !self.recommending && !self.loading && !self.generating && !self.pending_send;
        egui::Window::new("Perfis de execução")
            .open(&mut open)
            .default_width(630.0)
            .default_height(600.0)
            .show(ctx, |ui| {
                if self.pending_send { ui.disable(); }
                ui.label(RichText::new("Escolha o ritmo do Summanus").size(21.0).strong().color(TEXT));
                ui.label(RichText::new("Trocar de perfil reinicia o modelo e preserva as conversas.").color(MUTED).size(12.0));
                ui.add_space(8.0);
                if self.recommending {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Executando recomendar.bat antes de carregar o modelo…").color(MINT));
                    });
                } else if let Some(recommendation) = &self.recommendation {
                    ui.label(RichText::new(format!(
                        "Agora: {} MiB de VRAM em uso · {:.1} GiB de RAM livre · CPU {:.0}%",
                        recommendation.vram_usada_mib,
                        recommendation.ram_disponivel_gib,
                        recommendation.cpu_pct,
                    )).color(MINT).size(12.0));
                    if let Some(line) = &recommendation.recomendada {
                        ui.label(RichText::new(format!(
                            "Sugestão do script: {} · {}K · {} · estimativa {:.1} tokens/s",
                            line.id,
                            line.ctx / 1024,
                            recommendation.modelo.file_name().unwrap_or_default().to_string_lossy(),
                            line.tps_esperado,
                        )).color(TEXT).size(12.0));
                    } else {
                        ui.label(RichText::new("O script não encontrou um perfil que caiba agora.").color(RED));
                    }
                    ui.label(RichText::new("As velocidades do script são estimativas; o resultado real aparece após cada resposta.").color(MUTED).size(11.0));
                } else if let Some(error) = &self.recommender_error {
                    ui.label(RichText::new(format!("Recomendação indisponível: {error}")).color(RED).size(12.0));
                }
                ui.add_space(7.0);
                if ui.add_enabled_ui(can_switch, |ui| {
                    theme::button(ui, "Reavaliar PC e reiniciar", Tone::Quiet, egui::vec2(210.0, 34.0))
                }).inner.clicked() {
                    recheck = true;
                }
                ui.add_space(8.0);
                egui::ScrollArea::vertical().max_height(470.0).show(ui, |ui| {
                    let auto_active = self.state.profile_choice == profiles::AUTO;
                    egui::Frame::none()
                        .fill(SURFACE)
                        .stroke(Stroke::new(1.0_f32, if auto_active { MINT } else { OUTLINE }))
                        .rounding(egui::Rounding::same(12.0))
                        .inner_margin(egui::Margin::same(12.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Automático").strong().color(TEXT));
                                if auto_active { ui.label(RichText::new("ATIVO").color(MINT).size(10.0)); }
                            });
                            ui.label(RichText::new("Usa o modelo, contexto e threads escolhidos pelo recomendar.bat neste momento.").color(MUTED).size(11.0));
                            if ui.add_enabled_ui(can_switch && self.recommendation.as_ref().is_some_and(|r| r.recomendada.is_some()), |ui| {
                                theme::button(ui, "Usar automático", Tone::Secondary, egui::vec2(150.0, 32.0))
                            }).inner.clicked() { selected = Some(profiles::AUTO); }
                        });
                    ui.add_space(9.0);
                    for profile in profiles::PROFILES {
                        let active = self.state.profile_choice == profile.id;
                        let status = profile.recommender_id.and_then(|id| self.recommendation.as_ref().and_then(|r| r.status_for(id)));
                        let q3_available = !profile.q3 || (!self.forced_model && profiles::q3_path(&self.base_config).is_file());
                        let low_ram = profile.id == "patient" && self.recommendation.as_ref().is_some_and(|r| r.ram_disponivel_gib < 25.0);
                        let q3_low_ram = profile.q3 && self.recommendation.as_ref().is_some_and(|r| r.ram_disponivel_gib < 14.0);
                        let available = q3_available;
                        egui::Frame::none()
                            .fill(SURFACE)
                            .stroke(Stroke::new(1.0_f32, if active { LILAC } else { OUTLINE }))
                            .rounding(egui::Rounding::same(12.0))
                            .inner_margin(egui::Margin::same(12.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(profile.name).strong().color(TEXT));
                                    if active { ui.label(RichText::new("ATIVO").color(LILAC).size(10.0)); }
                                    if let Some(status) = status {
                                        ui.label(RichText::new(format!("Script: {status}")).color(if status == "não cabe" { RED } else { MINT }).size(10.0));
                                    }
                                });
                                ui.label(RichText::new(profile.detail).color(MUTED).size(11.0));
                                if !q3_available { ui.label(RichText::new("Arquivo IQ3 não encontrado ou --model força outro arquivo.").color(RED).size(11.0)); }
                                if status == Some("não cabe") { ui.label(RichText::new("Pouca VRAM neste momento: pode ficar muito lento ou falhar ao carregar.").color(RED).size(11.0)); }
                                if low_ram { ui.label(RichText::new("RAM livre abaixo de 25 GiB: pode usar paginação e ficar muito lento.").color(RED).size(11.0)); }
                                if q3_low_ram { ui.label(RichText::new("Pouca RAM livre para o IQ3: pode ficar lento ou falhar ao carregar.").color(RED).size(11.0)); }
                                if ui.add_enabled_ui(can_switch && available, |ui| {
                                    theme::button(ui, "Selecionar perfil", Tone::Quiet, egui::vec2(156.0, 32.0))
                                }).inner.clicked() { selected = Some(profile.id); }
                            });
                        ui.add_space(9.0);
                    }
                });
            });
        self.profiles_open = open;
        if recheck {
            self.start_after_recommendation = true;
            self.start_recommender(ctx);
        } else if let Some(profile) = selected {
            self.choose_profile(profile, ctx);
        }
    }

    fn render_context_decision(&mut self, ctx: &egui::Context) {
        let Some(decision) = &self.context_decision else {
            return;
        };
        let measured_tokens = decision.measured_tokens;
        let target_ctx = decision.target_ctx;
        let deadline = decision.deadline;
        let can_compact = decision.can_compact;
        let mut open = true;
        let mut compact = false;
        let mut increase = false;
        let mut cancel = false;
        let capacity = self.active_n_ctx.unwrap_or(self.config.n_ctx);
        egui::Window::new("Contexto quase cheio")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(470.0)
            .show(ctx, |ui| {
                ui.label(RichText::new(format!("{measured_tokens} / {capacity} tokens medidos pelo modelo")).strong().color(CORAL));
                ui.label(RichText::new("O envio foi pausado antes de chegar ao limite. Sua mensagem e seus anexos continuam no compositor.").color(TEXT));
                ui.add_space(8.0);
                if let Some(deadline) = deadline {
                    let seconds = deadline.saturating_duration_since(Instant::now()).as_secs().min(30);
                    ui.label(RichText::new(format!("Compactação automática em {seconds}s se você não escolher uma opção.")).color(MINT));
                    ctx.request_repaint_after(Duration::from_secs(1));
                } else if !can_compact {
                    ui.label(RichText::new("Não há histórico anterior que caiba em uma compactação segura. Selecione menos conteúdo ou amplie o contexto.").color(RED));
                }
                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled_ui(can_compact, |ui| {
                        theme::button(ui, "Compactar e enviar", Tone::Primary, egui::vec2(175.0, 38.0))
                    }).inner.clicked() { compact = true; }
                    if ui.add_enabled_ui(target_ctx > capacity, |ui| {
                        theme::button(ui, &format!("Ampliar para {}K", target_ctx / 1024), Tone::Secondary, egui::vec2(165.0, 38.0))
                    }).inner.clicked() { increase = true; }
                });
                ui.label(RichText::new("Ampliar reinicia o modelo e pode exigir mais RAM/VRAM. Se falhar, o app restaura o contexto anterior e tenta compactar.").size(11.0).color(MUTED));
                if theme::button(ui, "Voltar ao texto", Tone::Quiet, egui::vec2(145.0, 34.0)).clicked() {
                    cancel = true;
                }
            });
        if !open || cancel {
            self.pending_send = false;
            self.context_decision = None;
            self.status = "Envio pausado; ajuste o texto quando quiser.".into();
        } else if increase {
            self.increase_context_for_pending_send(target_ctx, ctx);
        } else if compact || deadline.is_some_and(|when| Instant::now() >= when) {
            self.start_auto_compact(ctx);
        }
    }

    fn render_context_window(&mut self, ctx: &egui::Context) {
        if !self.contexts_open {
            return;
        }
        let mut open = true;
        let mut changed = false;
        let mut summarize = None;
        let mut remove = None;
        let mut compact: Option<(Option<u64>, bool)> = None;
        egui::Window::new("Mapa de contextos")
            .open(&mut open)
            .default_width(670.0)
            .default_height(600.0)
            .show(ctx, |ui| {
                if self.pending_send { ui.disable(); }
                ui.label(
                    RichText::new("Guarde onde cada assunto ficou")
                        .size(19.0)
                        .strong()
                        .color(TEXT),
                );
                ui.label(
                    RichText::new("Selecione mensagens, anote o objetivo e crie uma memória para usar depois.")
                        .size(12.0)
                        .color(MUTED),
                );
                ui.label(RichText::new("Um resumo das mensagens 1 até a atual substitui esse histórico automaticamente. A conversa inteira só é reenviada se você escolher isso abaixo.").size(11.0).color(MINT));
                ui.add_space(12.0);
                let count = self.state.current().messages.len();
                if count == 0 {
                    ui.label(RichText::new("Envie uma mensagem para começar um recorte.").color(MUTED));
                } else {
                    self.context_start = self.context_start.clamp(1, count);
                    self.context_end = self.context_end.clamp(1, count);
                    egui::Frame::none()
                        .fill(SURFACE)
                        .rounding(egui::Rounding::same(12.0))
                        .inner_margin(egui::Margin::same(13.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new("NOVO RECORTE").size(11.0).strong().color(CORAL));
                            ui.horizontal(|ui| {
                                ui.label("Da mensagem");
                                ui.add(egui::DragValue::new(&mut self.context_start).range(1..=count));
                                ui.label("até");
                                ui.add(egui::DragValue::new(&mut self.context_end).range(1..=count));
                                ui.label(RichText::new(format!("de {count}")).color(MUTED));
                            });
                            ui.add_sized(
                                [ui.available_width(), 32.0],
                                egui::TextEdit::singleline(&mut self.context_title)
                                    .hint_text("Título do assunto")
                                    .text_color(TEXT),
                            );
                            ui.add_sized(
                                [ui.available_width(), 54.0],
                                egui::TextEdit::multiline(&mut self.context_note)
                                    .hint_text("Anotação: o que este trecho resolve ou onde paramos?")
                                    .text_color(TEXT),
                            );
                            if self.context_start > self.context_end {
                                ui.label(RichText::new("O início precisa vir antes do fim.").color(RED));
                            }
                            let can_save = !self.generating
                                && self.context_start <= self.context_end
                                && !self.context_title.trim().is_empty();
                            if ui.add_enabled_ui(can_save, |ui| {
                                theme::button(ui, "Salvar recorte", Tone::Primary, egui::vec2(145.0, 37.0))
                            }).inner.clicked() {
                                self.state.current_mut().add_context(
                                    self.context_start - 1,
                                    self.context_end,
                                    self.context_title.trim().to_string(),
                                    self.context_note.trim().to_string(),
                                );
                                self.context_title.clear();
                                self.context_note.clear();
                                changed = true;
                            }
                        });
                }
                ui.add_space(14.0);
                let cards = self.state.current().contexts.len();
                ui.label(RichText::new(format!("RECORTES SALVOS · {cards}")).size(11.0).strong().color(LILAC));
                egui::ScrollArea::vertical().max_height(370.0).show(ui, |ui| {
                    for index in 0..cards {
                        let (id, is_compact) = {
                            let conv = self.state.current();
                            let id = conv.contexts[index].id;
                            (id, conv.effective_compact_context().is_some_and(|active| active.id == id))
                        };
                        let draft = match &self.active_generation {
                            Some(crate::ActiveGeneration::Summary { context_id, draft, .. }) if *context_id == id => Some(draft.clone()),
                            _ => None,
                        };
                        let card = &mut self.state.current_mut().contexts[index];
                        egui::Frame::none()
                            .fill(SURFACE)
                            .stroke(Stroke::new(1.0_f32, if is_compact { LILAC } else { OUTLINE }))
                            .rounding(egui::Rounding::same(12.0))
                            .inner_margin(egui::Margin::same(13.0))
                            .outer_margin(egui::Margin::symmetric(0.0, 5.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("MENSAGENS {}–{}", card.start + 1, card.end)).size(10.0).color(MINT));
                                    if is_compact {
                                        ui.label(RichText::new("MEMÓRIA ATIVA").size(10.0).strong().color(LILAC));
                                    }
                                });
                                changed |= ui.add(egui::TextEdit::singleline(&mut card.title).text_color(TEXT)).changed();
                                ui.label(RichText::new("Sua anotação").size(11.0).color(MUTED));
                                changed |= ui.add(egui::TextEdit::multiline(&mut card.note).desired_rows(2).text_color(TEXT)).changed();
                                ui.label(RichText::new("Resumo para a memória do modelo").size(11.0).color(MUTED));
                                changed |= ui.add(egui::TextEdit::multiline(&mut card.summary).desired_rows(4).text_color(TEXT).hint_text("Escreva um resumo ou peça ao modelo para criá-lo.")).changed();
                                if let Some(draft) = &draft {
                                    ui.label(RichText::new(format!("Gerando: {draft}")).size(11.0).color(CORAL));
                                }
                                ui.horizontal_wrapped(|ui| {
                                    if ui.add_enabled_ui(!self.generating && self.server.is_some(), |ui| {
                                        theme::button(ui, "Resumir com IA", Tone::Secondary, egui::vec2(150.0, 34.0))
                                    }).inner.clicked() {
                                        summarize = Some(id);
                                    }
                                    changed |= ui.add_enabled_ui(!card.summary.trim().is_empty(), |ui| {
                                        ui.checkbox(&mut card.reference, "Usar como referência")
                                    }).inner.changed();
                                });
                                if card.start == 0 && !card.summary.trim().is_empty() {
                                    if theme::button(ui, if is_compact { "Usar conversa inteira" } else { "Usar esta memória" }, Tone::Quiet, egui::vec2(if is_compact { 195.0 } else { 190.0 }, 34.0)).clicked() {
                                        compact = Some(if is_compact { (None, true) } else { (Some(id), false) });
                                    }
                                }
                                if theme::button(ui, "Excluir recorte", Tone::Danger, egui::vec2(135.0, 32.0)).clicked() {
                                    remove = Some(id);
                                }
                            });
                    }
                });
            });
        if let Some(id) = remove {
            let conv = self.state.current_mut();
            conv.contexts.retain(|card| card.id != id);
            if conv.compact_context_id == Some(id) {
                conv.compact_context_id = None;
            }
            changed = true;
        }
        if let Some((id, full)) = compact {
            let conv = self.state.current_mut();
            conv.compact_context_id = id;
            conv.use_full_history = full;
            changed = true;
        }
        if changed {
            let _ = self.state.save();
        }
        if let Some(id) = summarize {
            self.summarize_context(id, ctx);
        }
        self.contexts_open = open;
    }

    fn open_file_import(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_file() {
            match crate::file_import::FilePreview::open(path) {
                Ok(preview) => self.file_preview = Some(preview),
                Err(error) => self.status = format!("Erro ao importar: {error}"),
            }
        }
    }

    fn render_file_import_window(&mut self, ctx: &egui::Context) {
        let Some(preview) = &mut self.file_preview else {
            return;
        };
        let mut open = true;
        let mut attach = false;
        egui::Window::new("Importar arquivo local")
            .open(&mut open)
            .default_width(630.0)
            .show(ctx, |ui| {
                ui.label(RichText::new(preview.path.display().to_string()).color(TEXT));
                ui.label(RichText::new("Escolha uma função, um intervalo ou o arquivo inteiro. O app não corta o trecho escolhido.").size(12.0).color(MUTED));
                if !preview.functions.is_empty() {
                    let selection = preview.selected_function
                        .and_then(|index| preview.functions.get(index))
                        .map_or("Arquivo completo", |function| function.name.as_str());
                    egui::ComboBox::from_label("Trecho Python")
                        .selected_text(selection)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut preview.selected_function, None, "Arquivo completo");
                            for (index, function) in preview.functions.iter().enumerate() {
                                ui.selectable_value(&mut preview.selected_function, Some(index), &function.name);
                            }
                        });
                }
                if preview.selected_function.is_none() {
                    ui.horizontal(|ui| {
                        ui.label("Linhas");
                        ui.add(egui::DragValue::new(&mut preview.line_start).range(1..=preview.total_lines));
                        ui.label("até");
                        ui.add(egui::DragValue::new(&mut preview.line_end).range(1..=preview.total_lines));
                        ui.label(RichText::new(format!("de {}", preview.total_lines)).color(MUTED));
                    });
                    preview.line_end = preview.line_end.max(preview.line_start).min(preview.total_lines);
                }
                let snippet = preview.snippet().clone();
                let chars = snippet.content.chars().count();
                let other_chars: usize = self.attachments.iter().map(|file| file.content.chars().count()).sum();
                let over_recommended = snippet.line_count > crate::file_import::RECOMMENDED_LINES
                    || chars > crate::file_import::RECOMMENDED_CHARS
                    || other_chars + chars > crate::file_import::RECOMMENDED_CHARS;
                let estimated_tokens = self.context_exact.unwrap_or(self.context_estimate)
                    + snippet.as_prompt().chars().count().div_ceil(3);
                let context_capacity = self.active_n_ctx.unwrap_or(self.config.n_ctx) as usize;
                let may_exceed_context = estimated_tokens + self.state.settings.max_tokens as usize > context_capacity;
                ui.label(RichText::new(format!("{} linhas · {} caracteres{}", snippet.line_count, chars, if snippet.partial { " · trecho escolhido" } else { " · arquivo inteiro" })).color(if over_recommended || may_exceed_context { CORAL } else { MINT }));
                if over_recommended || may_exceed_context {
                    let mut reasons = Vec::new();
                    if snippet.line_count > crate::file_import::RECOMMENDED_LINES {
                        reasons.push(format!("mais de {} linhas", crate::file_import::RECOMMENDED_LINES));
                    }
                    if chars > crate::file_import::RECOMMENDED_CHARS || other_chars + chars > crate::file_import::RECOMMENDED_CHARS {
                        reasons.push("mais de 60 mil caracteres nos anexos".into());
                    }
                    if may_exceed_context {
                        reasons.push(format!("~{estimated_tokens} tokens de entrada + {} reservados para resposta; capacidade {}", self.state.settings.max_tokens, context_capacity));
                    }
                    ui.label(RichText::new(format!("Atenção: {}. O conteúdo será anexado sem cortes, mas pode não caber no contexto do modelo.", reasons.join("; "))).size(12.0).color(CORAL));
                    ui.checkbox(&mut preview.confirm_large, "Entendi. Quero anexar este trecho completo.");
                }
                ui.label(RichText::new("Prévia completa: role até o fim para conferir o corpo da função.").size(11.0).color(MUTED));
                let lines = snippet.content.lines().collect::<Vec<_>>();
                egui::ScrollArea::both().max_height(210.0).show_rows(ui, 17.0, lines.len(), |ui, range| {
                    for index in range {
                        ui.add(egui::Label::new(RichText::new(format!("{:>4}  {}", index + 1, lines[index])).monospace().color(TEXT).size(12.0)).extend());
                    }
                });
                if ui.add_enabled_ui(!self.pending_send && (!over_recommended && !may_exceed_context || preview.confirm_large), |ui| {
                    theme::button(ui, "Anexar sem cortes", Tone::Primary, egui::vec2(175.0, 38.0))
                }).inner.clicked() {
                    attach = true;
                }
            });
        if attach {
            if let Some(preview) = &mut self.file_preview {
                let snippet = preview.snippet().clone();
                self.attachments.push(snippet);
                self.file_preview = None;
            }
        } else if !open {
            self.file_preview = None;
        }
    }

    fn render_delete_confirmation(&mut self, ctx: &egui::Context) {
        if !self.confirm_delete {
            return;
        }
        let mut open = true;
        let mut delete = false;
        egui::Window::new("Excluir conversa?")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("Esta conversa será removida do histórico local.").color(MUTED),
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if theme::button(ui, "Cancelar", Tone::Quiet, egui::vec2(100.0, 37.0)).clicked()
                    {
                        self.confirm_delete = false;
                    }
                    if theme::button(
                        ui,
                        "Excluir conversa",
                        Tone::Danger,
                        egui::vec2(145.0, 37.0),
                    )
                    .clicked()
                    {
                        delete = true;
                    }
                });
            });
        if delete {
            self.state.delete_current();
            let _ = self.state.save();
            self.confirm_delete = false;
        } else if !open {
            self.confirm_delete = false;
        }
    }
}

impl eframe::App for LocalApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.update_context_meter(ctx);
        if ctx.input(|i| i.key_pressed(egui::Key::N) && i.modifiers.ctrl)
            && !self.generating
            && !self.pending_send
        {
            self.state.new_conversation();
            let _ = self.state.save();
        }
        self.sidebar(ctx);
        self.render_header(ctx);
        self.render_composer(ctx);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(24.0, 12.0)),
            )
            .show(ctx, |ui| {
                if self.state.current().messages.is_empty() {
                    self.render_empty(ui);
                } else {
                    self.render_chat(ui, ctx);
                }
            });
        self.settings_window(ctx);
        self.render_profiles_window(ctx);
        self.render_context_window(ctx);
        self.render_file_import_window(ctx);
        self.render_delete_confirmation(ctx);
        self.render_context_decision(ctx);
    }
}

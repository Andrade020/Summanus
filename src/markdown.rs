use crate::theme::{self, Tone, CORAL, LILAC, OUTLINE, TEXT};
use eframe::egui::{self, text::LayoutJob, Color32, FontId, RichText, TextFormat};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::collections::HashMap;

enum Part {
    Text(String),
    Code { language: String, code: String },
    Math { latex: String, display: bool },
}

#[derive(Default)]
pub struct MathRenderer {
    font: Option<latex_rust::MathFont>,
    textures: HashMap<String, Option<egui::TextureHandle>>,
}

fn math_parts(text: &str) -> Vec<Part> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut pos = 0;
    let mut ticks = 0usize;
    while pos < text.len() {
        let rest = &text[pos..];
        if rest.starts_with('`') {
            let run = rest.bytes().take_while(|&b| b == b'`').count();
            if ticks == 0 {
                ticks = run;
            } else if ticks == run {
                ticks = 0;
            }
            pos += run;
            continue;
        }
        if ticks > 0 {
            pos += rest.chars().next().unwrap().len_utf8();
            continue;
        }
        let delimiter = if rest.starts_with("$$") {
            Some(("$$", "$$", true))
        } else if rest.starts_with("\\[") {
            Some(("\\[", "\\]", true))
        } else if rest.starts_with("[/") {
            Some(("[/", "/]", true))
        } else if rest.starts_with("\\(") {
            Some(("\\(", "\\)", false))
        } else if rest.starts_with('$') {
            Some(("$", "$", false))
        } else {
            None
        };
        if let Some((open, close, display)) = delimiter {
            let opening_pos = pos;
            let escaped = text[..pos]
                .bytes()
                .rev()
                .take_while(|&b| b == b'\\')
                .count()
                % 2
                == 1;
            let after_open = pos + open.len();
            if !escaped
                && (display
                    || text[after_open..]
                        .chars()
                        .next()
                        .is_some_and(|c| !c.is_whitespace()))
            {
                let mut end = after_open;
                while end < text.len() && end - after_open <= 4096 {
                    if text[end..].starts_with(close) {
                        let formula = &text[after_open..end];
                        let close_ok =
                            display || formula.chars().last().is_some_and(|c| !c.is_whitespace());
                        if close_ok && !formula.trim().is_empty() {
                            if start < pos {
                                result.push(Part::Text(text[start..pos].to_owned()));
                            }
                            result.push(Part::Math {
                                latex: formula.trim().to_owned(),
                                display,
                            });
                            pos = end + close.len();
                            start = pos;
                            break;
                        }
                    }
                    end += text[end..].chars().next().unwrap().len_utf8();
                }
                if pos != opening_pos {
                    continue;
                }
            }
        }
        pos += rest.chars().next().unwrap().len_utf8();
    }
    if start < text.len() {
        result.push(Part::Text(text[start..].to_owned()));
    }
    result
}

fn parts(markdown: &str) -> Vec<Part> {
    let mut result = Vec::new();
    let mut text = String::new();
    let mut code = String::new();
    let mut language = String::new();
    let mut fence = 0;
    for line in markdown.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let ticks = trimmed.chars().take_while(|&c| c == '`').count();
        if fence == 0 && ticks >= 3 {
            if !text.is_empty() {
                result.push(Part::Text(std::mem::take(&mut text)));
            }
            fence = ticks;
            language = trimmed[ticks..].trim().to_string();
        } else if fence > 0 && ticks >= fence && trimmed[ticks..].trim().is_empty() {
            result.push(Part::Code {
                language: std::mem::take(&mut language),
                code: std::mem::take(&mut code),
            });
            fence = 0;
        } else if fence > 0 {
            code.push_str(line);
        } else {
            text.push_str(line);
        }
    }
    if fence > 0 {
        result.push(Part::Code { language, code });
    }
    if !text.is_empty() {
        result.push(Part::Text(text));
    }
    result
}

fn format(color: Color32, strong: bool, code: bool, heading: bool, link: bool) -> TextFormat {
    TextFormat {
        font_id: if code {
            FontId::monospace(14.0)
        } else {
            FontId::proportional(if heading { 20.0 } else { 15.0 })
        },
        color: if link {
            LILAC
        } else if code {
            CORAL
        } else {
            color
        },
        italics: false,
        underline: if link {
            egui::Stroke::new(1.0_f32, LILAC)
        } else {
            egui::Stroke::NONE
        },
        extra_letter_spacing: if strong { 0.4 } else { 0.0 },
        ..Default::default()
    }
}

fn render_text(ui: &mut egui::Ui, markdown: &str, base: Color32) {
    let mut job = LayoutJob::default();
    let (mut strong, mut heading, mut link) = (false, false, false);
    let mut list_depth = 0usize;
    for event in Parser::new_ext(
        markdown,
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    ) {
        match event {
            Event::Start(Tag::Strong) => strong = true,
            Event::End(TagEnd::Strong) => strong = false,
            Event::Start(Tag::Heading { .. }) => heading = true,
            Event::End(TagEnd::Heading(_)) => {
                heading = false;
                job.append("\n", 0.0, format(base, false, false, false, false));
            }
            Event::Start(Tag::Link { .. }) => link = true,
            Event::End(TagEnd::Link) => link = false,
            Event::Start(Tag::List(_)) => list_depth += 1,
            Event::End(TagEnd::List(_)) => {
                list_depth = list_depth.saturating_sub(1);
                job.append("\n", 0.0, format(base, false, false, false, false));
            }
            Event::Start(Tag::Item) => job.append(
                &format!("{}•  ", "  ".repeat(list_depth.saturating_sub(1))),
                0.0,
                format(base, false, false, false, false),
            ),
            Event::End(TagEnd::Item) | Event::End(TagEnd::Paragraph) => {
                job.append("\n", 0.0, format(base, false, false, false, false))
            }
            Event::Text(s) => job.append(&s, 0.0, format(base, strong, false, heading, link)),
            Event::Code(s) => job.append(&s, 0.0, format(base, strong, true, heading, link)),
            Event::SoftBreak | Event::HardBreak => {
                job.append("\n", 0.0, format(base, strong, false, heading, link))
            }
            Event::Rule => job.append(
                "------------\n",
                0.0,
                format(base, false, false, false, false),
            ),
            Event::TaskListMarker(done) => job.append(
                if done { "[x] " } else { "[ ] " },
                0.0,
                format(base, false, false, false, false),
            ),
            _ => {}
        }
    }
    job.wrap.max_width = ui.available_width();
    ui.add(egui::Label::new(job).wrap());
}

impl MathRenderer {
    fn show(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, latex: &str, display: bool) {
        let key = format!("{display}:{latex}");
        if !self.textures.contains_key(&key) {
            if self.font.is_none() {
                self.font = latex_rust::MathFont::stix_two_math().ok();
            }
            let texture = (|| {
                let font = self.font.as_ref()?;
                let mut options = latex_rust::PngOptions::new();
                options.font_size_pt = latex_rust::Dim::from_i64(if display { 17 } else { 15 });
                options.color = latex_rust::Color::rgb(244, 241, 247);
                options.display = display;
                let png = latex_rust::latex_to_png(latex, font, &options).ok()?;
                let image = image::load_from_memory(&png).ok()?.to_rgba8();
                let size = [image.width() as usize, image.height() as usize];
                Some(ctx.load_texture(
                    key.clone(),
                    egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
                    egui::TextureOptions::LINEAR,
                ))
            })();
            self.textures.insert(key.clone(), texture);
        }
        egui::Frame::none()
            .fill(Color32::from_rgb(18, 21, 35))
            .rounding(egui::Rounding::same(10.0))
            .inner_margin(egui::Margin::symmetric(11.0, 8.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if let Some(Some(texture)) = self.textures.get(&key) {
                        let size = texture.size_vec2() / 2.0;
                        let width = size.x.min((ui.available_width() - 72.0).max(40.0));
                        ui.add(egui::Image::new((texture.id(), size * (width / size.x))));
                    } else {
                        ui.label(RichText::new(latex).monospace().color(TEXT));
                    }
                    if ui.small_button("Copiar LaTeX").clicked() {
                        ctx.output_mut(|o| o.copied_text = latex.to_owned());
                    }
                });
            });
        ui.add_space(4.0);
    }
}

pub fn render(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    markdown: &str,
    base: Color32,
    math: &mut MathRenderer,
) {
    for part in parts(markdown) {
        let expanded = match part {
            Part::Text(text) => math_parts(&text),
            other => vec![other],
        };
        for part in expanded {
            match part {
                Part::Text(text) => render_text(ui, &text, base),
                Part::Math { latex, display } => math.show(ui, ctx, &latex, display),
                Part::Code { language, code } => {
                    egui::Frame::none()
                        .fill(Color32::from_rgb(18, 21, 35))
                        .stroke(egui::Stroke::new(1.0_f32, OUTLINE))
                        .rounding(egui::Rounding::same(11.0))
                        .inner_margin(egui::Margin::same(12.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(if language.is_empty() {
                                        "código"
                                    } else {
                                        &language
                                    })
                                    .color(LILAC)
                                    .size(12.0),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if theme::button(
                                            ui,
                                            "Copiar código",
                                            Tone::Quiet,
                                            egui::vec2(112.0, 30.0),
                                        )
                                        .clicked()
                                        {
                                            ctx.output_mut(|o| o.copied_text = code.clone());
                                        }
                                    },
                                );
                            });
                            ui.separator();
                            egui::ScrollArea::both().max_height(300.0).show(ui, |ui| {
                                ui.label(
                                    RichText::new(code.trim_end_matches('\n'))
                                        .monospace()
                                        .color(TEXT)
                                        .size(14.0),
                                );
                            });
                        });
                    ui.add_space(7.0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fenced_code_and_unclosed_stream() {
        let parsed = parts("Antes\n```rust\nfn main() {}\n```\nDepois");
        assert!(
            matches!(&parsed[1], Part::Code { language, code } if language == "rust" && code == "fn main() {}\n")
        );
        let parsed = parts("```python\nprint(1)");
        assert!(
            matches!(&parsed[0], Part::Code { language, code } if language == "python" && code == "print(1)")
        );
    }

    #[test]
    fn math_delimiters_and_inline_code() {
        let parsed =
            math_parts("A $$x^2$$ B \\[\\frac{1}{2}\\] C \\(a+b\\) D [/y^2/] E $z$ F ` $no$ `");
        let formulas: Vec<_> = parsed
            .iter()
            .filter_map(|p| match p {
                Part::Math { latex, .. } => Some(latex.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(formulas, ["x^2", "\\frac{1}{2}", "a+b", "y^2", "z"]);
    }

    #[test]
    fn latex_renders_to_png() {
        let font = latex_rust::MathFont::stix_two_math().unwrap();
        let png = latex_rust::latex_to_png("\\frac{1}{2}", &font, &latex_rust::PngOptions::new())
            .unwrap();
        assert!(png.starts_with(b"\x89PNG"));
    }
}

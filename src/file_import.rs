use regex::Regex;
use std::{fs, path::PathBuf, sync::OnceLock};

const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
pub const RECOMMENDED_LINES: usize = 1000;
pub const RECOMMENDED_CHARS: usize = 60_000;

#[derive(Clone)]
pub struct PythonFunction {
    pub name: String,
    pub start: usize,
    pub end: usize,
}

pub struct FilePreview {
    pub path: PathBuf,
    source: String,
    pub functions: Vec<PythonFunction>,
    pub selected_function: Option<usize>,
    pub line_start: usize,
    pub line_end: usize,
    pub total_lines: usize,
    pub confirm_large: bool,
    cached: Option<(Option<usize>, usize, usize, AttachedFile)>,
}

#[derive(Clone)]
pub struct AttachedFile {
    pub path: PathBuf,
    pub label: String,
    pub language: String,
    pub content: String,
    pub line_count: usize,
    pub partial: bool,
}

impl FilePreview {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let metadata = fs::metadata(&path).map_err(|error| error.to_string())?;
        if metadata.len() > MAX_FILE_BYTES {
            return Err(
                "Arquivo maior que 8 MB; nada foi anexado ou cortado. Escolha um arquivo menor."
                    .into(),
            );
        }
        let bytes = fs::read(&path).map_err(|error| error.to_string())?;
        let source = String::from_utf8(bytes).map_err(|_| {
            "Este arquivo não é texto UTF-8. Converta-o antes de importar.".to_string()
        })?;
        let functions = if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("py"))
        {
            python_functions(&source)
        } else {
            Vec::new()
        };
        let total_lines = source.lines().count().max(1);
        Ok(Self {
            path,
            source,
            functions,
            selected_function: None,
            line_start: 1,
            line_end: total_lines,
            total_lines,
            confirm_large: false,
            cached: None,
        })
    }

    pub fn snippet(&mut self) -> &AttachedFile {
        let key = (self.selected_function, self.line_start, self.line_end);
        let stale = self
            .cached
            .as_ref()
            .is_none_or(|cached| (cached.0, cached.1, cached.2) != key);
        if stale {
            let snippet = self.build_snippet();
            self.cached = Some((key.0, key.1, key.2, snippet));
            self.confirm_large = false;
        }
        &self.cached.as_ref().unwrap().3
    }

    fn build_snippet(&self) -> AttachedFile {
        let language = self
            .path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("txt")
            .to_ascii_lowercase();
        let lines: Vec<&str> = self.source.lines().collect();
        let (label, source, partial) = if let Some(function) = self
            .selected_function
            .and_then(|index| self.functions.get(index))
        {
            (
                function.name.clone(),
                lines[function.start..function.end].join("\n"),
                true,
            )
        } else {
            let start = self.line_start.saturating_sub(1).min(lines.len());
            let end = self.line_end.min(lines.len()).max(start);
            (
                format!("Linhas {}–{}", start + 1, end),
                lines[start..end].join("\n"),
                start > 0 || end < lines.len(),
            )
        };
        let clean = if language == "html" || language == "htm" {
            strip_base64_data_uris(&source)
        } else {
            source
        };
        let line_count = clean.lines().count();
        AttachedFile {
            path: self.path.clone(),
            label,
            language,
            content: clean,
            line_count,
            partial,
        }
    }
}

fn python_functions(source: &str) -> Vec<PythonFunction> {
    let mut parser = tree_sitter::Parser::new();
    if parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    collect_functions(tree.root_node(), source, "", None, &mut result);
    result
}

fn collect_functions(
    node: tree_sitter::Node<'_>,
    source: &str,
    prefix: &str,
    decorated_start: Option<usize>,
    output: &mut Vec<PythonFunction>,
) {
    if node.kind() == "decorated_definition" {
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "function_definition" || child.kind() == "class_definition" {
                collect_functions(
                    child,
                    source,
                    prefix,
                    Some(node.start_position().row),
                    output,
                );
            }
        }
        return;
    }
    let is_function = node.kind() == "function_definition";
    let is_class = node.kind() == "class_definition";
    let next_prefix = if is_function || is_class {
        let name = node
            .child_by_field_name("name")
            .and_then(|name| name.utf8_text(source.as_bytes()).ok())
            .unwrap_or("?");
        let qualified = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}.{name}")
        };
        if is_function {
            let end_position = node.end_position();
            let end = end_position.row + usize::from(end_position.column > 0);
            output.push(PythonFunction {
                name: qualified.clone(),
                start: decorated_start.unwrap_or(node.start_position().row),
                end,
            });
        }
        qualified
    } else {
        prefix.to_string()
    };
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() != "identifier" {
            collect_functions(child, source, &next_prefix, None, output);
        }
    }
}

fn strip_base64_data_uris(input: &str) -> String {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| {
        Regex::new(r#"(?i)data:[^\s"'<>)]{0,200};base64,(?:[a-z0-9+/=_-]+(?:\r?\n)?)+"#).unwrap()
    });
    pattern.replace_all(input, "[base64 removido]").into_owned()
}

impl AttachedFile {
    pub fn as_prompt(&self) -> String {
        let longest_ticks = self
            .content
            .split(|c| c != '`')
            .map(str::len)
            .max()
            .unwrap_or(0);
        let fence = "`".repeat((longest_ticks + 1).max(3));
        format!(
            "Arquivo local: {}\nTrecho: {} ({} linhas{}).\n{fence}{}\n{}\n{fence}",
            self.path.display(),
            self.label,
            self.line_count,
            if self.partial {
                ", trecho selecionado"
            } else {
                ""
            },
            self.language,
            self.content
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_only_selected_python_function() {
        let source = "class Tools:\n    @staticmethod\n    def run(x):\n        return x\n\ndef other():\n    return 2\n";
        let functions = python_functions(source);
        assert_eq!(functions.len(), 2);
        assert_eq!(functions[0].name, "Tools.run");
        let lines: Vec<_> = source.lines().collect();
        assert_eq!(
            lines[functions[0].start..functions[0].end].join("\n"),
            "    @staticmethod\n    def run(x):\n        return x"
        );
    }

    #[test]
    fn handles_multiline_signature_and_ignores_code_in_strings() {
        let source = "text = '''\ndef fake():\n    pass\n'''\n\ndef real(\n    item: str,\n):\n    return (\n        item\n    )\n";
        let functions = python_functions(source);
        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0].name, "real");
        let lines: Vec<_> = source.lines().collect();
        assert!(lines[functions[0].start..functions[0].end]
            .join("\n")
            .contains("return (\n        item\n    )"));
    }

    #[test]
    fn selected_function_does_not_include_the_other_script() {
        let source = "def first():\n    return 1\n\ndef second():\n    return 2\n".to_string();
        let functions = python_functions(&source);
        let mut preview = FilePreview {
            path: PathBuf::from("sample.py"),
            source,
            functions,
            selected_function: Some(0),
            line_start: 1,
            line_end: 5,
            total_lines: 5,
            confirm_large: false,
            cached: None,
        };
        let snippet = preview.snippet();
        assert_eq!(snippet.label, "first");
        assert!(snippet.content.contains("return 1"));
        assert!(!snippet.content.contains("second"));
        preview.selected_function = None;
        preview.line_start = 4;
        preview.line_end = 5;
        let lines = preview.snippet();
        assert!(lines.content.contains("second"));
        assert!(!lines.content.contains("first"));
    }

    #[test]
    fn selected_long_function_keeps_body_after_preview_rows() {
        let mut source = "def long_function(value):\n".to_string();
        for index in 0..80 {
            source.push_str(&format!("    step_{index} = value + {index}\n"));
        }
        source.push_str("    return step_79\n\ndef unrelated():\n    return 0\n");
        let functions = python_functions(&source);
        let total_lines = source.lines().count();
        let mut preview = FilePreview {
            path: PathBuf::from("long.py"),
            source,
            functions,
            selected_function: Some(0),
            line_start: 1,
            line_end: total_lines,
            total_lines,
            confirm_large: false,
            cached: None,
        };
        let snippet = preview.snippet();
        assert!(snippet.line_count > 30);
        assert!(snippet.content.contains("return step_79"));
        assert!(!snippet.content.contains("unrelated"));
    }

    #[test]
    fn removes_base64_without_cutting_python_source() {
        let html = "<img src=\"data:image/png;base64,aGVsbG8=\"><p>Olá</p>";
        assert_eq!(
            strip_base64_data_uris(html),
            "<img src=\"[base64 removido]\"><p>Olá</p>"
        );
        assert_eq!(
            strip_base64_data_uris("url(data:image/png;base64,AAAA\nBBBB)"),
            "url([base64 removido])"
        );
        let source = "x\n".repeat(1001);
        let mut preview = FilePreview {
            path: PathBuf::from("sample.py"),
            source,
            functions: Vec::new(),
            selected_function: None,
            line_start: 1,
            line_end: 1001,
            total_lines: 1001,
            confirm_large: false,
            cached: None,
        };
        let snippet = preview.snippet();
        assert_eq!(snippet.line_count, 1001);
        assert_eq!(snippet.content.lines().last(), Some("x"));
        assert!(!snippet.partial);
        preview.confirm_large = true;
        preview.line_end = 500;
        assert!(preview.snippet().partial);
        assert!(!preview.confirm_large);
        preview.source = "z".repeat(70_000);
        preview.line_end = 1;
        preview.total_lines = 1;
        assert_eq!(preview.snippet().content.len(), 70_000);
    }
}

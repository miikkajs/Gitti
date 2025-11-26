use std::path::Path;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Style, ThemeSet};
use syntect::parsing::SyntaxSet;

pub struct Highlighter {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl Highlighter {
    pub fn new() -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }

    pub fn highlight_lines(&self, path: &str, lines: &[String]) -> Vec<Vec<(Style, String)>> {
        let extension = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let syntax = self
            .syntax_set
            .find_syntax_by_extension(extension)
            .or_else(|| {
                self.syntax_set
                    .find_syntax_by_first_line(lines.first().map(|s| s.as_str()).unwrap_or(""))
            })
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        let theme = &self.theme_set.themes["base16-eighties.dark"];
        let mut highlighter = HighlightLines::new(syntax, theme);

        lines
            .iter()
            .map(|line| {
                highlighter
                    .highlight_line(line, &self.syntax_set)
                    .map(|ranges| {
                        ranges
                            .into_iter()
                            .map(|(style, text)| (style, text.to_string()))
                            .collect()
                    })
                    .unwrap_or_else(|_| vec![(Style::default(), line.clone())])
            })
            .collect()
    }
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new()
    }
}

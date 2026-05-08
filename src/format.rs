use regex::Regex;
use std::fmt;
use std::sync::OnceLock;

pub const MISSING_GLYPH: &str = "\u{2014}";

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Format {
    Concise,
    Json,
    Raw,
}

pub fn non_empty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

pub fn display_value(value: Option<&str>) -> &str {
    value.filter(|value| !value.is_empty()).unwrap_or(MISSING_GLYPH)
}

impl fmt::Display for Format {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Concise => "concise",
            Self::Json => "json",
            Self::Raw => "raw",
        };
        formatter.write_str(value)
    }
}

pub fn render_capture(raw: &str, format: Format) -> String {
    match format {
        Format::Raw => raw.to_owned(),
        Format::Concise | Format::Json => render_concise(raw),
    }
}

pub fn strip_ansi(input: &str) -> String {
    strip_ansi_escapes::strip_str(input)
}

fn render_concise(raw: &str) -> String {
    let stripped = strip_ansi(raw);
    let mut lines: Vec<String> = stripped
        .lines()
        .map(|line| line.trim_end().to_owned())
        .collect();

    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }

    let mut emitted = Vec::with_capacity(lines.len());
    for line in lines {
        let is_duplicate = emitted.last().is_some_and(|previous| previous == &line);
        let is_idle = is_idle_prompt_line(&line);

        if is_idle && is_duplicate {
            continue;
        }

        emitted.push(line);
    }

    emitted.join("\n")
}

fn is_idle_prompt_line(line: &str) -> bool {
    static IDLE_PROMPT: OnceLock<Regex> = OnceLock::new();

    IDLE_PROMPT
        .get_or_init(|| Regex::new(r"^[>$#%▌] ?$").expect("idle prompt regex compiles"))
        .is_match(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_removes_escape_sequences() {
        assert_eq!(strip_ansi("\x1b[32mhello\x1b[0m"), "hello");
    }

    #[test]
    fn concise_trims_trailing_blank_lines_and_line_whitespace() {
        let raw = "alpha   \nbeta\t \n  \n\n";

        assert_eq!(render_capture(raw, Format::Concise), "alpha\nbeta");
    }

    #[test]
    fn concise_collapses_idle_prompt_runs() {
        let raw = "> \n> \n> \nready\n";

        assert_eq!(render_capture(raw, Format::Concise), ">\nready");
    }

    #[test]
    fn concise_preserves_non_idle_consecutive_duplicate_lines() {
        let raw = "total: 5 files\ntotal: 5 files\ntotal: 5 files\nother\n";

        assert_eq!(
            render_capture(raw, Format::Concise),
            "total: 5 files\ntotal: 5 files\ntotal: 5 files\nother"
        );
    }

    #[test]
    fn json_output_matches_concise_text() {
        let raw = "\x1b[31mhi\x1b[0m   \n> \n> \n\n";

        assert_eq!(
            render_capture(raw, Format::Json),
            render_capture(raw, Format::Concise)
        );
    }

    #[test]
    fn raw_output_is_untouched() {
        let raw = "\x1b[31mhi\x1b[0m   \n\n";

        assert_eq!(render_capture(raw, Format::Raw), raw);
    }
}

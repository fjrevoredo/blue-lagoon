use anyhow::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedContentInput<'a> {
    pub url: &'a str,
    pub content_type: Option<&'a str>,
    pub body: &'a str,
    pub response_truncated: bool,
    pub max_response_bytes: u64,
    pub max_preview_chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedContentForModel {
    pub formatter_kind: &'static str,
    pub preview: String,
    pub preview_truncated: bool,
}

pub trait FetchedContentFormatter {
    fn format(&self, input: &FetchedContentInput<'_>) -> Result<FetchedContentForModel>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultFetchedContentFormatter;

impl FetchedContentFormatter for DefaultFetchedContentFormatter {
    fn format(&self, input: &FetchedContentInput<'_>) -> Result<FetchedContentForModel> {
        if is_html(input) {
            HtmlMarkdownFormatter.format(input)
        } else {
            PlainTextFormatter.format(input)
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct HtmlMarkdownFormatter;

impl FetchedContentFormatter for HtmlMarkdownFormatter {
    fn format(&self, input: &FetchedContentInput<'_>) -> Result<FetchedContentForModel> {
        let html_without_non_content = remove_non_content_html_blocks(input.body);
        if let Some(pre_content) = extract_first_tag_content(&html_without_non_content, "pre") {
            let text = html_fragment_to_text(pre_content);
            let without_ansi = strip_ansi_escape_sequences(&text);
            let without_terminal_rules = replace_terminal_rule_chars(&without_ansi);
            let normalized = normalize_plain_text(&without_terminal_rules);
            return Ok(bounded_preview(
                "html_pre_text_sanitized",
                &normalized,
                input.max_preview_chars,
            ));
        }

        let markdown = html2md::parse_html(&html_without_non_content);
        let normalized = normalize_markdown(&markdown);
        Ok(bounded_preview(
            "html_markdown",
            &normalized,
            input.max_preview_chars,
        ))
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PlainTextFormatter;

impl FetchedContentFormatter for PlainTextFormatter {
    fn format(&self, input: &FetchedContentInput<'_>) -> Result<FetchedContentForModel> {
        let without_ansi = strip_ansi_escape_sequences(input.body);
        let without_terminal_rules = replace_terminal_rule_chars(&without_ansi);
        let normalized = normalize_plain_text(&without_terminal_rules);
        Ok(bounded_preview(
            "plain_text_sanitized",
            &normalized,
            input.max_preview_chars,
        ))
    }
}

fn is_html(input: &FetchedContentInput<'_>) -> bool {
    input
        .content_type
        .map(|value| value.to_ascii_lowercase().contains("html"))
        .unwrap_or_else(|| {
            let trimmed = input.body.trim_start().to_ascii_lowercase();
            trimmed.starts_with("<!doctype html")
                || trimmed.starts_with("<html")
                || trimmed.contains("<body")
        })
}

fn bounded_preview(
    formatter_kind: &'static str,
    value: &str,
    max_chars: usize,
) -> FetchedContentForModel {
    let preview = value.chars().take(max_chars).collect::<String>();
    FetchedContentForModel {
        formatter_kind,
        preview,
        preview_truncated: value.chars().count() > max_chars,
    }
}

fn normalize_markdown(value: &str) -> String {
    normalize_lines(value, true)
}

fn normalize_plain_text(value: &str) -> String {
    normalize_lines(value, false)
}

fn normalize_lines(value: &str, preserve_markdown_blank_lines: bool) -> String {
    let mut normalized = Vec::new();
    let mut previous_blank = false;
    for line in value.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        let line = line.trim();
        if line.is_empty() {
            if preserve_markdown_blank_lines && !previous_blank && !normalized.is_empty() {
                normalized.push(String::new());
            }
            previous_blank = true;
            continue;
        }
        normalized.push(line.to_string());
        previous_blank = false;
    }
    normalized.join("\n").trim().to_string()
}

fn remove_non_content_html_blocks(value: &str) -> String {
    let mut output = value.to_string();
    for tag_name in ["script", "style", "noscript", "svg"] {
        output = remove_html_tag_blocks(&output, tag_name);
    }
    output
}

fn remove_html_tag_blocks(value: &str, tag_name: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    let lower = value.to_ascii_lowercase();
    let open_pattern = format!("<{tag_name}");
    let close_pattern = format!("</{tag_name}>");

    while let Some(relative_start) = lower[cursor..].find(&open_pattern) {
        let start = cursor + relative_start;
        output.push_str(&value[cursor..start]);
        let Some(relative_end) = lower[start..].find(&close_pattern) else {
            return output;
        };
        cursor = start + relative_end + close_pattern.len();
    }

    output.push_str(&value[cursor..]);
    output
}

fn extract_first_tag_content<'a>(value: &'a str, tag_name: &str) -> Option<&'a str> {
    let lower = value.to_ascii_lowercase();
    let open_pattern = format!("<{tag_name}");
    let open_start = lower.find(&open_pattern)?;
    let content_start = lower[open_start..].find('>')? + open_start + 1;
    let close_pattern = format!("</{tag_name}>");
    let content_end = lower[content_start..].find(&close_pattern)? + content_start;
    Some(&value[content_start..content_end])
}

fn html_fragment_to_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '<' => {
                let mut tag = String::new();
                for next in chars.by_ref() {
                    if next == '>' {
                        break;
                    }
                    tag.push(next);
                }
                let tag_name = tag
                    .trim_start_matches('/')
                    .split_whitespace()
                    .next()
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if matches!(tag_name.as_str(), "br" | "p" | "div" | "tr" | "li") {
                    output.push('\n');
                }
            }
            '&' => {
                let mut entity = String::new();
                while let Some(next) = chars.peek().copied() {
                    chars.next();
                    if next == ';' {
                        break;
                    }
                    entity.push(next);
                    if entity.len() > 16 {
                        break;
                    }
                }
                output.push_str(&decode_html_entity(&entity));
            }
            _ => output.push(character),
        }
    }
    output
}

fn decode_html_entity(entity: &str) -> String {
    match entity {
        "amp" => "&".to_string(),
        "gt" => ">".to_string(),
        "lt" => "<".to_string(),
        "nbsp" => " ".to_string(),
        "quot" => "\"".to_string(),
        "apos" => "'".to_string(),
        value if value.starts_with("#x") => u32::from_str_radix(&value[2..], 16)
            .ok()
            .and_then(char::from_u32)
            .map(|character| character.to_string())
            .unwrap_or_default(),
        value if value.starts_with('#') => value[1..]
            .parse::<u32>()
            .ok()
            .and_then(char::from_u32)
            .map(|character| character.to_string())
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn replace_terminal_rule_chars(value: &str) -> String {
    value
        .chars()
        .map(|character| match character {
            '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' | '─' | '│' | '═' | '║' | '╔'
            | '╗' | '╚' | '╝' | '╠' | '╣' | '╦' | '╩' | '╬' => ' ',
            _ => character,
        })
        .collect()
}

fn strip_ansi_escape_sequences(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '\u{1b}' {
            output.push(character);
            continue;
        }

        match chars.peek().copied() {
            Some('[') => {
                chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                let mut previous_was_escape = false;
                for next in chars.by_ref() {
                    if previous_was_escape && next == '\\' {
                        break;
                    }
                    if next == '\u{7}' {
                        break;
                    }
                    previous_was_escape = next == '\u{1b}';
                }
            }
            _ => {}
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(content_type: Option<&'a str>, body: &'a str) -> FetchedContentInput<'a> {
        FetchedContentInput {
            url: "https://example.com",
            content_type,
            body,
            response_truncated: false,
            max_response_bytes: 65_536,
            max_preview_chars: 1_500,
        }
    }

    #[test]
    fn plain_text_formatter_strips_terminal_control_sequences() {
        let formatted = DefaultFetchedContentFormatter
            .format(&input(
                Some("text/plain; charset=utf-8"),
                "\u{1b}[38;5;226mSunny\u{1b}[0m\n┌────┐\n│ 16 °C │",
            ))
            .expect("plain text should format");

        assert_eq!(formatted.formatter_kind, "plain_text_sanitized");
        assert!(formatted.preview.contains("Sunny"));
        assert!(formatted.preview.contains("16 °C"));
        assert!(!formatted.preview.contains("\u{1b}"));
        assert!(!formatted.preview.contains("┌"));
    }

    #[test]
    fn html_formatter_converts_document_to_markdown() {
        let formatted = DefaultFetchedContentFormatter
            .format(&input(
                Some("text/html"),
                "<html><body><h1>Weather</h1><p>Sunny <strong>16 C</strong></p></body></html>",
            ))
            .expect("html should format");

        assert_eq!(formatted.formatter_kind, "html_markdown");
        assert!(formatted.preview.contains("Weather"));
        assert!(formatted.preview.contains("Sunny"));
    }

    #[test]
    fn html_formatter_prefers_pre_text_and_skips_styles() {
        let formatted = DefaultFetchedContentFormatter
            .format(&input(
                Some("text/html; charset=utf-8"),
                r#"<html>
                    <head><style>.f1 { color: red; }</style></head>
                    <body><pre>Market Cap: $2T
                    ┌────┬─────┐
                    │ Rank │ Coin │ Price │
                    │ 1 │ <span>BTC</span> │ 76326.2 │
                    </pre></body>
                </html>"#,
            ))
            .expect("html terminal page should format");

        assert_eq!(formatted.formatter_kind, "html_pre_text_sanitized");
        assert!(formatted.preview.contains("Market Cap: $2T"));
        assert!(formatted.preview.contains("Rank Coin Price"));
        assert!(formatted.preview.contains("1 BTC 76326.2"));
        assert!(!formatted.preview.contains("color: red"));
        assert!(!formatted.preview.contains("<span>"));
        assert!(!formatted.preview.contains("┌"));
    }

    #[test]
    fn formatter_marks_preview_truncation() {
        let mut input = input(Some("text/plain"), "abcdef");
        input.max_preview_chars = 3;

        let formatted = DefaultFetchedContentFormatter
            .format(&input)
            .expect("plain text should format");

        assert_eq!(formatted.preview, "abc");
        assert!(formatted.preview_truncated);
    }
}

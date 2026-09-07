use serde::{Deserialize, Serialize};

use crate::{ColumnInfo, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CsvDelimiter {
    Comma,
    Semicolon,
    Tab,
    Pipe,
}

impl CsvDelimiter {
    pub const ALL: [CsvDelimiter; 4] = [
        CsvDelimiter::Comma,
        CsvDelimiter::Semicolon,
        CsvDelimiter::Tab,
        CsvDelimiter::Pipe,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            CsvDelimiter::Comma => ",",
            CsvDelimiter::Semicolon => ";",
            CsvDelimiter::Tab => "\t",
            CsvDelimiter::Pipe => "|",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CsvQuote {
    Always,
    IfNeeded,
    Never,
}

impl CsvQuote {
    pub const ALL: [CsvQuote; 3] = [CsvQuote::Always, CsvQuote::IfNeeded, CsvQuote::Never];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CsvLineBreak {
    Lf,
    CrLf,
    Cr,
}

impl CsvLineBreak {
    pub const ALL: [CsvLineBreak; 3] = [CsvLineBreak::Lf, CsvLineBreak::CrLf, CsvLineBreak::Cr];

    pub fn as_str(self) -> &'static str {
        match self {
            CsvLineBreak::Lf => "\n",
            CsvLineBreak::CrLf => "\r\n",
            CsvLineBreak::Cr => "\r",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CsvDecimal {
    Period,
    Comma,
}

impl CsvDecimal {
    pub const ALL: [CsvDecimal; 2] = [CsvDecimal::Period, CsvDecimal::Comma];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CsvOptions {
    pub null_to_empty: bool,
    pub line_break_to_space: bool,
    pub header_row: bool,
    pub sanitize_formulas: bool,
    pub delimiter: CsvDelimiter,
    pub quote: CsvQuote,
    pub line_break: CsvLineBreak,
    pub decimal: CsvDecimal,
}

impl Default for CsvOptions {
    fn default() -> Self {
        CsvOptions {
            null_to_empty: true,
            line_break_to_space: false,
            header_row: true,
            sanitize_formulas: true,
            delimiter: CsvDelimiter::Comma,
            quote: CsvQuote::IfNeeded,
            line_break: CsvLineBreak::Lf,
            decimal: CsvDecimal::Period,
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Full text of a value for export. Never truncates. `None` for Null.
pub fn value_to_text(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::Bool(b) => Some(if *b { "true".to_string() } else { "false".to_string() }),
        Value::Int(i) => Some(i.to_string()),
        Value::Float(f) => Some(f.to_string()),
        Value::Text(s) => Some(s.clone()),
        Value::Bytes(b) => Some(format!("0x{}", hex_encode(b))),
        Value::Date(d) => Some(d.format("%Y-%m-%d").to_string()),
        Value::Time(t) => Some(t.format("%H:%M:%S").to_string()),
        Value::DateTime(dt) => Some(dt.format("%Y-%m-%d %H:%M:%S").to_string()),
        Value::TimestampTz(dt) => Some(dt.to_rfc3339()),
        Value::Decimal(d) => Some(d.to_string()),
        Value::Uuid(u) => Some(u.to_string()),
        Value::Json(j) => Some(serde_json::to_string(j).unwrap_or_default()),
    }
}

fn is_plain_decimal(s: &str) -> bool {
    let unsigned = s.strip_prefix(['+', '-']).unwrap_or(s);
    let Some((int_part, frac_part)) = unsigned.split_once('.') else {
        return false;
    };
    !int_part.is_empty()
        && !frac_part.is_empty()
        && int_part.chars().all(|c| c.is_ascii_digit())
        && frac_part.chars().all(|c| c.is_ascii_digit())
}

fn quote_field(field: &str) -> String {
    format!("\"{}\"", field.replace('"', "\"\""))
}

/// `had_line_breaks` carries whether the raw value contained a line
/// break before `line_break_to_space` scrubbed it, so `IfNeeded`
/// still quotes a converted multi-line value even though the
/// resulting text no longer contains `\n`/`\r` itself.
fn escape_field(field: &str, opts: &CsvOptions, had_line_breaks: bool) -> String {
    let mut field = field.to_string();
    if opts.sanitize_formulas && field.starts_with(['=', '+', '-', '@']) {
        field.insert(0, '\'');
    }
    match opts.quote {
        CsvQuote::Always => quote_field(&field),
        CsvQuote::Never => field,
        CsvQuote::IfNeeded => {
            let delim = opts.delimiter.as_str();
            if field.contains(delim)
                || field.contains('"')
                || field.contains('\n')
                || field.contains('\r')
                || had_line_breaks
            {
                quote_field(&field)
            } else {
                field
            }
        }
    }
}

fn format_cell(value: &Value, opts: &CsvOptions) -> String {
    let Some(mut text) = value_to_text(value) else {
        let empty = if opts.null_to_empty {
            String::new()
        } else {
            "NULL".to_string()
        };
        return escape_field(&empty, opts, false);
    };
    let had_line_breaks = text.contains('\n') || text.contains('\r');
    if opts.line_break_to_space {
        text = text.replace("\r\n", " ").replace(['\r', '\n'], " ");
    }
    if opts.decimal == CsvDecimal::Comma && is_plain_decimal(&text) {
        text = text.replace('.', ",");
    }
    escape_field(&text, opts, had_line_breaks)
}

pub fn render_csv(columns: &[ColumnInfo], rows: &[Vec<Value>], opts: &CsvOptions) -> String {
    let delim = opts.delimiter.as_str();
    let line_break = opts.line_break.as_str();
    let mut out = String::new();
    if opts.header_row {
        let header: Vec<String> = columns.iter().map(|c| escape_field(&c.name, opts, false)).collect();
        out.push_str(&header.join(delim));
        out.push_str(line_break);
    }
    for row in rows {
        let cells: Vec<String> = row.iter().map(|v| format_cell(v, opts)).collect();
        out.push_str(&cells.join(delim));
        out.push_str(line_break);
    }
    out
}

pub fn render_tsv(columns: &[ColumnInfo], rows: &[Vec<Value>], with_headers: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    if with_headers {
        let header: Vec<&str> = columns.iter().map(|c| c.name.as_str()).collect();
        lines.push(header.join("\t"));
    }
    for row in rows {
        let cells: Vec<String> = row
            .iter()
            .map(|v| value_to_text(v).unwrap_or_else(|| "NULL".to_string()))
            .collect();
        lines.push(cells.join("\t"));
    }
    lines.join("\n")
}

fn value_to_json(v: &Value) -> serde_json::Value {
    match v {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => serde_json::Value::Number((*i).into()),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Value::Decimal(d) => {
            let s = d.to_string();
            match s.parse::<serde_json::Number>() {
                Ok(n) => serde_json::Value::Number(n),
                Err(_) => serde_json::Value::String(s),
            }
        }
        Value::Json(j) => j.clone(),
        other => match value_to_text(other) {
            Some(s) => serde_json::Value::String(s),
            None => serde_json::Value::Null,
        },
    }
}

pub fn row_to_json(columns: &[ColumnInfo], row: &[Value]) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    for (i, col) in columns.iter().enumerate() {
        let value = row.get(i).map(value_to_json).unwrap_or(serde_json::Value::Null);
        map.insert(col.name.clone(), value);
    }
    serde_json::Value::Object(map)
}

pub fn render_json(columns: &[ColumnInfo], rows: &[Vec<Value>]) -> String {
    let values: Vec<serde_json::Value> = rows.iter().map(|row| row_to_json(columns, row)).collect();
    serde_json::to_string_pretty(&values).unwrap_or_else(|_| "[]".to_string())
}

fn markdown_cell(value: &Value) -> String {
    let text = value_to_text(value).unwrap_or_else(|| "NULL".to_string());
    text.replace('|', "\\|")
        .replace("\r\n", "<br>")
        .replace(['\r', '\n'], "<br>")
}

pub fn render_markdown(columns: &[ColumnInfo], rows: &[Vec<Value>]) -> String {
    let mut lines: Vec<String> = Vec::new();
    let header: Vec<&str> = columns.iter().map(|c| c.name.as_str()).collect();
    lines.push(format!("| {} |", header.join(" | ")));
    let separator: Vec<&str> = columns.iter().map(|_| "---").collect();
    lines.push(format!("| {} |", separator.join(" | ")));
    for row in rows {
        let cells: Vec<String> = row.iter().map(markdown_cell).collect();
        lines.push(format!("| {} |", cells.join(" | ")));
    }
    lines.join("\n")
}

fn in_clause_literal(v: &Value) -> Option<String> {
    match v {
        Value::Null | Value::Bytes(_) => None,
        Value::Bool(b) => Some(if *b { "TRUE".to_string() } else { "FALSE".to_string() }),
        Value::Int(_) | Value::Float(_) | Value::Decimal(_) => value_to_text(v),
        other => value_to_text(other).map(|s| format!("'{}'", s.replace('\'', "''"))),
    }
}

pub fn render_in_clause(rows: &[Vec<Value>], col_index: usize) -> String {
    let literals: Vec<String> = rows
        .iter()
        .filter_map(|row| row.get(col_index))
        .filter_map(in_clause_literal)
        .collect();
    format!("({})", literals.join(", "))
}

#[cfg(test)]
mod tests {
    use chrono::{NaiveDate, NaiveTime};
    use rust_decimal::Decimal;
    use std::str::FromStr;
    use uuid::Uuid;

    use super::*;

    fn col(name: &str) -> ColumnInfo {
        ColumnInfo {
            name: name.into(),
            data_type: "text".into(),
            nullable: true,
            primary_key: false,
            is_auto_increment: false,
            default_value: None,
            is_generated: false,
        }
    }

    fn cols(names: &[&str]) -> Vec<ColumnInfo> {
        names.iter().map(|n| col(n)).collect()
    }

    #[test]
    fn value_to_text_covers_every_variant() {
        assert_eq!(value_to_text(&Value::Null), None);
        assert_eq!(value_to_text(&Value::Bool(true)), Some("true".to_string()));
        assert_eq!(value_to_text(&Value::Bool(false)), Some("false".to_string()));
        assert_eq!(value_to_text(&Value::Int(42)), Some("42".to_string()));
        assert_eq!(value_to_text(&Value::Float(1.5)), Some("1.5".to_string()));
        assert_eq!(value_to_text(&Value::Text("hi".into())), Some("hi".to_string()));
        assert_eq!(
            value_to_text(&Value::Bytes(vec![0xde, 0xad])),
            Some("0xdead".to_string())
        );
        assert_eq!(
            value_to_text(&Value::Date(NaiveDate::from_ymd_opt(2024, 1, 2).unwrap())),
            Some("2024-01-02".to_string())
        );
        assert_eq!(
            value_to_text(&Value::Time(NaiveTime::from_hms_opt(13, 5, 9).unwrap())),
            Some("13:05:09".to_string())
        );
        assert_eq!(
            value_to_text(&Value::DateTime(
                NaiveDate::from_ymd_opt(2024, 1, 2)
                    .unwrap()
                    .and_hms_opt(13, 5, 9)
                    .unwrap()
            )),
            Some("2024-01-02 13:05:09".to_string())
        );
        assert_eq!(
            value_to_text(&Value::Decimal(Decimal::from_str("12.30").unwrap())),
            Some("12.30".to_string())
        );
        let uuid = Uuid::from_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(value_to_text(&Value::Uuid(uuid)), Some(uuid.to_string()));
        assert_eq!(
            value_to_text(&Value::Json(serde_json::json!({"a": 1}))),
            Some("{\"a\":1}".to_string())
        );
    }

    #[test]
    fn csv_defaults_render_comma_lf_if_needed() {
        let columns = cols(&["id", "name"]);
        let rows = vec![vec![Value::Int(1), Value::Text("Alice".into())]];
        let out = render_csv(&columns, &rows, &CsvOptions::default());
        assert_eq!(out, "id,name\n1,Alice\n");
    }

    #[test]
    fn csv_quote_if_needed_triggers_on_delimiter() {
        let columns = cols(&["a"]);
        let rows = vec![vec![Value::Text("has,comma".into())]];
        let out = render_csv(&columns, &rows, &CsvOptions::default());
        assert_eq!(out, "a\n\"has,comma\"\n");
    }

    #[test]
    fn csv_quote_if_needed_triggers_on_quote_char() {
        let columns = cols(&["a"]);
        let rows = vec![vec![Value::Text("say \"hi\"".into())]];
        let out = render_csv(&columns, &rows, &CsvOptions::default());
        assert_eq!(out, "a\n\"say \"\"hi\"\"\"\n");
    }

    #[test]
    fn csv_quote_if_needed_triggers_on_original_line_break_even_when_converted() {
        let columns = cols(&["a"]);
        let rows = vec![vec![Value::Text("line1\nline2".into())]];
        let opts = CsvOptions {
            line_break_to_space: true,
            ..Default::default()
        };
        let out = render_csv(&columns, &rows, &opts);
        assert_eq!(out, "a\n\"line1 line2\"\n");
    }

    #[test]
    fn csv_quote_always_quotes_everything() {
        let columns = cols(&["a"]);
        let rows = vec![vec![Value::Text("plain".into())]];
        let opts = CsvOptions {
            quote: CsvQuote::Always,
            ..Default::default()
        };
        let out = render_csv(&columns, &rows, &opts);
        assert_eq!(out, "\"a\"\n\"plain\"\n");
    }

    #[test]
    fn csv_quote_never_quotes_nothing_even_with_delimiter() {
        let columns = cols(&["a"]);
        let rows = vec![vec![Value::Text("has,comma".into())]];
        let opts = CsvOptions {
            quote: CsvQuote::Never,
            ..Default::default()
        };
        let out = render_csv(&columns, &rows, &opts);
        assert_eq!(out, "a\nhas,comma\n");
    }

    #[test]
    fn csv_sanitizes_formula_prefixes() {
        let columns = cols(&["a"]);
        for ch in ['=', '+', '-', '@'] {
            let rows = vec![vec![Value::Text(format!("{ch}cmd"))]];
            let out = render_csv(&columns, &rows, &CsvOptions::default());
            assert_eq!(out, format!("a\n'{ch}cmd\n"), "prefix {ch} should be sanitized");
        }
    }

    #[test]
    fn csv_does_not_sanitize_non_formula_prefixes() {
        let columns = cols(&["a"]);
        let rows = vec![vec![Value::Text("plain text".into())]];
        let out = render_csv(&columns, &rows, &CsvOptions::default());
        assert_eq!(out, "a\nplain text\n");
    }

    #[test]
    fn csv_decimal_comma_only_for_plain_decimals() {
        let columns = cols(&["a"]);
        let opts = CsvOptions {
            decimal: CsvDecimal::Comma,
            delimiter: CsvDelimiter::Semicolon,
            ..Default::default()
        };
        assert_eq!(
            render_csv(&columns, &[vec![Value::Text("1.5".into())]], &opts),
            "a\n1,5\n"
        );
        assert_eq!(
            render_csv(&columns, &[vec![Value::Text("1e5".into())]], &opts),
            "a\n1e5\n"
        );
        assert_eq!(
            render_csv(&columns, &[vec![Value::Text("12".into())]], &opts),
            "a\n12\n"
        );
        assert_eq!(
            render_csv(&columns, &[vec![Value::Text("1.2.3".into())]], &opts),
            "a\n1.2.3\n"
        );
    }

    #[test]
    fn csv_null_modes() {
        let columns = cols(&["a"]);
        let rows = vec![vec![Value::Null]];
        let out_empty = render_csv(&columns, &rows, &CsvOptions::default());
        assert_eq!(out_empty, "a\n\n");
        let opts = CsvOptions {
            null_to_empty: false,
            ..Default::default()
        };
        let out_null = render_csv(&columns, &rows, &opts);
        assert_eq!(out_null, "a\nNULL\n");
    }

    #[test]
    fn csv_crlf_line_break() {
        let columns = cols(&["a"]);
        let rows = vec![vec![Value::Int(1)]];
        let opts = CsvOptions {
            line_break: CsvLineBreak::CrLf,
            ..Default::default()
        };
        let out = render_csv(&columns, &rows, &opts);
        assert_eq!(out, "a\r\n1\r\n");
    }

    #[test]
    fn csv_semicolon_delimiter() {
        let columns = cols(&["a", "b"]);
        let rows = vec![vec![Value::Int(1), Value::Int(2)]];
        let opts = CsvOptions {
            delimiter: CsvDelimiter::Semicolon,
            ..Default::default()
        };
        let out = render_csv(&columns, &rows, &opts);
        assert_eq!(out, "a;b\n1;2\n");
    }

    #[test]
    fn csv_header_off() {
        let columns = cols(&["a"]);
        let rows = vec![vec![Value::Int(1)]];
        let opts = CsvOptions {
            header_row: false,
            ..Default::default()
        };
        let out = render_csv(&columns, &rows, &opts);
        assert_eq!(out, "1\n");
    }

    #[test]
    fn tsv_with_and_without_header() {
        let columns = cols(&["a", "b"]);
        let rows = vec![vec![Value::Int(1), Value::Null]];
        assert_eq!(render_tsv(&columns, &rows, true), "a\tb\n1\tNULL");
        assert_eq!(render_tsv(&columns, &rows, false), "1\tNULL");
    }

    #[test]
    fn json_number_vs_string_handling() {
        let columns = cols(&["i", "f", "d", "s"]);
        let row = vec![
            Value::Int(5),
            Value::Float(1.5),
            Value::Decimal(Decimal::from_str("9.99").unwrap()),
            Value::Text("hi".into()),
        ];
        let json = row_to_json(&columns, &row);
        assert_eq!(json["i"], serde_json::json!(5));
        assert_eq!(json["f"], serde_json::json!(1.5));
        assert_eq!(json["d"], serde_json::json!(9.99));
        assert_eq!(json["s"], serde_json::json!("hi"));
    }

    #[test]
    fn json_missing_cell_is_null() {
        let columns = cols(&["a", "b"]);
        let row = vec![Value::Int(1)];
        let json = row_to_json(&columns, &row);
        assert_eq!(json["b"], serde_json::Value::Null);
    }

    #[test]
    fn render_json_empty_rows_is_empty_array() {
        let columns = cols(&["a"]);
        assert_eq!(render_json(&columns, &[]), "[]");
    }

    #[test]
    fn markdown_escapes_pipe_and_converts_line_breaks() {
        let columns = cols(&["a"]);
        let rows = vec![vec![Value::Text("has|pipe\nand newline".into())]];
        let out = render_markdown(&columns, &rows);
        assert_eq!(out, "| a |\n| --- |\n| has\\|pipe<br>and newline |");
    }

    #[test]
    fn in_clause_skips_null_and_quotes_text() {
        let rows = vec![
            vec![Value::Text("O'Brien".into())],
            vec![Value::Null],
            vec![Value::Int(5)],
            vec![Value::Bool(true)],
        ];
        let out = render_in_clause(&rows, 0);
        assert_eq!(out, "('O''Brien', 5, TRUE)");
    }

    #[test]
    fn in_clause_empty_when_all_skipped() {
        let rows = vec![vec![Value::Null], vec![Value::Bytes(vec![1, 2])]];
        assert_eq!(render_in_clause(&rows, 0), "()");
    }
}

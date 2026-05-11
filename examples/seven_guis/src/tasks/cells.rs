use dioxus_native::prelude::*;
use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
struct Sheet {
    raw: HashMap<(usize, usize), String>,
}

impl Sheet {
    fn get_raw(&self, row: usize, col: usize) -> &str {
        self.raw.get(&(row, col)).map(|s| s.as_str()).unwrap_or("")
    }

    fn evaluate(&self, row: usize, col: usize) -> String {
        self.evaluate_inner(row, col, &mut HashSet::new())
    }

    fn evaluate_inner(
        &self,
        row: usize,
        col: usize,
        visiting: &mut HashSet<(usize, usize)>,
    ) -> String {
        if !visiting.insert((row, col)) {
            return "#CYCLE".to_string();
        }
        let raw = self.get_raw(row, col);
        let result = if let Some(formula) = raw.strip_prefix('=') {
            match eval_expr(self, formula.trim(), visiting) {
                Some(v) => {
                    // Show integers without a decimal point for cleanliness
                    if v.fract() == 0.0 && v.abs() < 1e15 {
                        format!("{}", v as i64)
                    } else {
                        format!("{}", v)
                    }
                }
                None => "#ERR".to_string(),
            }
        } else {
            raw.to_string()
        };
        visiting.remove(&(row, col));
        result
    }
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Num(f64),
    CellRef(usize, usize), // (row, col)
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    LParen,
    RParen,
    Colon,
    Comma,
}

fn tokenize(input: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = input.chars().collect();
    let mut pos = 0;
    let mut tokens = Vec::new();

    while pos < chars.len() {
        let ch = chars[pos];
        match ch {
            ' ' | '\t' => {
                pos += 1;
            }
            '+' => {
                tokens.push(Token::Plus);
                pos += 1;
            }
            '-' => {
                tokens.push(Token::Minus);
                pos += 1;
            }
            '*' => {
                tokens.push(Token::Star);
                pos += 1;
            }
            '/' => {
                tokens.push(Token::Slash);
                pos += 1;
            }
            '(' => {
                tokens.push(Token::LParen);
                pos += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                pos += 1;
            }
            ':' => {
                tokens.push(Token::Colon);
                pos += 1;
            }
            ',' => {
                tokens.push(Token::Comma);
                pos += 1;
            }
            '0'..='9' | '.' => {
                // Parse a number
                let start = pos;
                while pos < chars.len() && (chars[pos].is_ascii_digit() || chars[pos] == '.') {
                    pos += 1;
                }
                let num_str: String = chars[start..pos].iter().collect();
                let v: f64 = num_str.parse().ok()?;
                tokens.push(Token::Num(v));
            }
            'A'..='Z' | 'a'..='z' => {
                // Could be a cell ref (e.g. A0, B3, Z25) or an ident (e.g. SUM)
                let start = pos;
                // Collect leading letters
                while pos < chars.len() && chars[pos].is_ascii_alphabetic() {
                    pos += 1;
                }
                let letters: String = chars[start..pos].iter().collect();
                // If followed by digits, treat as a cell reference
                if pos < chars.len() && chars[pos].is_ascii_digit() {
                    let num_start = pos;
                    while pos < chars.len() && chars[pos].is_ascii_digit() {
                        pos += 1;
                    }
                    let digits: String = chars[num_start..pos].iter().collect();
                    let col = col_from_letters(&letters)?;
                    let row: usize = digits.parse().ok()?;
                    tokens.push(Token::CellRef(row, col));
                } else {
                    tokens.push(Token::Ident(letters));
                }
            }
            _ => return None,
        }
    }
    Some(tokens)
}

/// Convert a column letter string (A, B, …, Z) to a 0-based index.
/// Only single-letter columns (A–Z) are supported.
fn col_from_letters(s: &str) -> Option<usize> {
    if s.len() == 1 {
        let b = s.as_bytes()[0].to_ascii_uppercase();
        if b.is_ascii_uppercase() {
            return Some((b as usize) - (b'A' as usize));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Recursive-descent parser / evaluator
// ---------------------------------------------------------------------------

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    sheet: &'a Sheet,
    visiting: &'a mut HashSet<(usize, usize)>,
}

impl<'a> Parser<'a> {
    fn new(
        tokens: &'a [Token],
        sheet: &'a Sheet,
        visiting: &'a mut HashSet<(usize, usize)>,
    ) -> Self {
        Parser {
            tokens,
            pos: 0,
            sheet,
            visiting,
        }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn consume(&mut self) -> Option<&Token> {
        let t = self.tokens.get(self.pos);
        self.pos += 1;
        t
    }

    /// expr = term (('+' | '-') term)*
    fn parse_expr(&mut self) -> Option<f64> {
        let mut val = self.parse_term()?;
        loop {
            match self.peek() {
                Some(Token::Plus) => {
                    self.consume();
                    val += self.parse_term()?;
                }
                Some(Token::Minus) => {
                    self.consume();
                    val -= self.parse_term()?;
                }
                _ => break,
            }
        }
        Some(val)
    }

    /// term = unary (('*' | '/') unary)*
    fn parse_term(&mut self) -> Option<f64> {
        let mut val = self.parse_unary()?;
        loop {
            match self.peek() {
                Some(Token::Star) => {
                    self.consume();
                    val *= self.parse_unary()?;
                }
                Some(Token::Slash) => {
                    self.consume();
                    let divisor = self.parse_unary()?;
                    if divisor == 0.0 {
                        return None;
                    }
                    val /= divisor;
                }
                _ => break,
            }
        }
        Some(val)
    }

    /// unary = '-' unary | factor
    fn parse_unary(&mut self) -> Option<f64> {
        if self.peek() == Some(&Token::Minus) {
            self.consume();
            return Some(-self.parse_factor()?);
        }
        self.parse_factor()
    }

    /// factor = '(' expr ')' | SUM '(' cellref ':' cellref ')' | cellref | number
    fn parse_factor(&mut self) -> Option<f64> {
        match self.peek().cloned() {
            Some(Token::LParen) => {
                self.consume(); // '('
                let val = self.parse_expr()?;
                match self.consume() {
                    Some(Token::RParen) => Some(val),
                    _ => None,
                }
            }
            Some(Token::Ident(name)) => {
                self.consume();
                let upper = name.to_uppercase();
                match upper.as_str() {
                    "SUM" => self.parse_sum(),
                    _ => None,
                }
            }
            Some(Token::CellRef(row, col)) => {
                self.consume();
                // Evaluate the referenced cell, converting its display value to f64
                let cell_val = self.sheet.evaluate_inner(row, col, self.visiting);
                cell_val.trim().parse::<f64>().ok()
            }
            Some(Token::Num(v)) => {
                self.consume();
                Some(v)
            }
            _ => None,
        }
    }

    /// Parses: '(' CellRef ':' CellRef ')'  and returns the sum
    fn parse_sum(&mut self) -> Option<f64> {
        match self.consume() {
            Some(Token::LParen) => {}
            _ => return None,
        }
        let (r1, c1) = match self.consume() {
            Some(Token::CellRef(r, c)) => (*r, *c),
            _ => return None,
        };
        match self.consume() {
            Some(Token::Colon) => {}
            _ => return None,
        }
        let (r2, c2) = match self.consume() {
            Some(Token::CellRef(r, c)) => (*r, *c),
            _ => return None,
        };
        match self.consume() {
            Some(Token::RParen) => {}
            _ => return None,
        }

        let row_min = r1.min(r2);
        let row_max = r1.max(r2);
        let col_min = c1.min(c2);
        let col_max = c1.max(c2);

        let mut sum = 0.0f64;
        for r in row_min..=row_max {
            for c in col_min..=col_max {
                let cell_val = self.sheet.evaluate_inner(r, c, self.visiting);
                if let Ok(v) = cell_val.trim().parse::<f64>() {
                    sum += v;
                }
                // non-numeric cells contribute 0 (silently ignored)
            }
        }
        Some(sum)
    }
}

fn eval_expr(sheet: &Sheet, expr: &str, visiting: &mut HashSet<(usize, usize)>) -> Option<f64> {
    let tokens = tokenize(expr)?;
    let mut parser = Parser::new(&tokens, sheet, visiting);
    let result = parser.parse_expr()?;
    // Ensure all tokens were consumed
    if parser.pos != parser.tokens.len() {
        return None;
    }
    Some(result)
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const COLS: usize = 26;
const ROWS: usize = 26;

fn col_label(c: usize) -> String {
    ((b'A' + c as u8) as char).to_string()
}

#[component]
pub fn Cells() -> Element {
    let mut sheet: Signal<Sheet> = use_signal(Sheet::default);
    let mut selected: Signal<Option<(usize, usize)>> = use_signal(|| None);
    let mut edit_val: Signal<String> = use_signal(String::new);

    rsx! {
        div { class: "cells-root",
            style { {CSS} }
            div { class: "cells-grid",
                // Corner header cell
                div { class: "cell header corner", "" }
                // Column headers A–Z
                for c in 0..COLS {
                    div { class: "cell header col-header", {col_label(c)} }
                }
                // Data rows
                for r in 0..ROWS {
                    // Row header
                    div { class: "cell header row-header", "{r}" }
                    // Data cells
                    for c in 0..COLS {
                        {
                            let is_selected = selected() == Some((r, c));
                            let display = sheet().evaluate(r, c);
                            rsx! {
                                div {
                                    key: "{r}-{c}",
                                    class: if is_selected { "cell selected" } else { "cell" },
                                    onclick: move |_| {
                                        let raw = sheet().get_raw(r, c).to_string();
                                        edit_val.set(raw);
                                        selected.set(Some((r, c)));
                                    },
                                    if is_selected {
                                        input {
                                            class: "cell-input",
                                            value: "{edit_val}",
                                            oninput: move |evt| edit_val.set(evt.value()),
                                            onblur: move |_| {
                                                sheet.write().raw.insert((r, c), edit_val());
                                                selected.set(None);
                                            },
                                            onkeydown: move |evt| {
                                                if evt.key() == Key::Enter {
                                                    sheet.write().raw.insert((r, c), edit_val());
                                                    selected.set(None);
                                                }
                                            }
                                        }
                                    } else {
                                        "{display}"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CSS
// ---------------------------------------------------------------------------

const CSS: &str = r#"
.cells-root {
    width: 100%;
    height: 100%;
    overflow: auto;
    font-family: sans-serif;
    font-size: 13px;
    background-color: #f0f0f0;
    box-sizing: border-box;
    padding: 8px;
}

.cells-grid {
    display: grid;
    /* 27 columns: 1 row-header column (50px) + 26 data columns (80px each) */
    grid-template-columns: 50px repeat(26, 80px);
    border-left: 1px solid #b0b0b0;
    border-top: 1px solid #b0b0b0;
    width: max-content;
}

.cell {
    border-right: 1px solid #b0b0b0;
    border-bottom: 1px solid #b0b0b0;
    padding: 2px 4px;
    height: 22px;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    box-sizing: border-box;
    background-color: #ffffff;
    color: #1a1a1a;
    cursor: default;
    display: flex;
    align-items: center;
}

.cell.header {
    background-color: #e8e8e8;
    color: #444444;
    font-weight: 600;
    justify-content: center;
    cursor: default;
}

.cell.header.corner {
    background-color: #d8d8d8;
}

.cell.header.row-header {
    background-color: #e8e8e8;
    justify-content: center;
}

.cell.selected {
    background-color: #e8f0fe;
    outline: 2px solid #4a6cf7;
    outline-offset: -2px;
    padding: 0;
}

.cell-input {
    width: 100%;
    height: 100%;
    border: none;
    outline: none;
    background: transparent;
    font-size: 13px;
    font-family: sans-serif;
    padding: 2px 4px;
    box-sizing: border-box;
    color: #1a1a1a;
}
"#;

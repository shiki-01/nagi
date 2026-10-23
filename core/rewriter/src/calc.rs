//! 電卓: 四則演算の式を安全な自前のパーサーで評価する（[`crate::SimpleCalculator`] の実体）。
//!
//! 対応する式: 非負の整数・小数、演算子 `+ - * / × ÷`（全角 `＋ － ＊ ／` も可）、括弧 `( )`
//! （全角 `（ ）` も可）、優先順位つきの複数項、末尾の `=`（全角 `＝` も可）。
//! 先頭の符号（単項マイナス）・べき乗は対象外。0 除算・桁あふれ・非有限の結果は `None`。

/// 式の最大文字数（正規化後）。極端に長い入力を評価しない
const MAX_EXPR_CHARS: usize = 64;
/// 括弧の最大の深さ（再帰の深さの上限）
const MAX_DEPTH: usize = 16;
/// 数値 1 つの最大文字数
const MAX_NUMBER_CHARS: usize = 15;
/// 結果の絶対値の上限（これ以上は桁あふれとして出さない）
const MAX_RESULT: f64 = 1e15;
/// 結果の有効桁数
const SIGNIFICANT_DIGITS: i32 = 10;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Token {
    Num(f64),
    Op(char),
    LParen,
    RParen,
}

/// 全角の数字・演算子・括弧・小数点を半角にそろえる（演算子は `+ - * /` に統一する）
fn normalize(c: char) -> char {
    match c {
        '０'..='９' => char::from_u32(c as u32 - '０' as u32 + '0' as u32).unwrap_or(c),
        '＋' => '+',
        '－' | '−' | '―' | 'ー' => '-',
        '×' | '＊' | '✕' => '*',
        '÷' | '／' => '/',
        '（' => '(',
        '）' => ')',
        '．' => '.',
        '＝' => '=',
        _ => c,
    }
}

/// 式を字句に分ける。数字・演算子・括弧以外が混じる、数値が不正なら `None`
fn tokenize(expr: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = expr.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '0'..='9' | '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                if text.len() > MAX_NUMBER_CHARS
                    || text.matches('.').count() > 1
                    || text.starts_with('.')
                    || text.ends_with('.')
                {
                    return None;
                }
                tokens.push(Token::Num(text.parse().ok()?));
                continue;
            }
            '+' | '-' | '*' | '/' => tokens.push(Token::Op(c)),
            '(' => tokens.push(Token::LParen),
            ')' => tokens.push(Token::RParen),
            _ => return None,
        }
        i += 1;
    }
    Some(tokens)
}

/// 再帰下降パーサー（式 = 項 (+|- 項)*、項 = 因子 (*|/ 因子)*、因子 = 数値 | ( 式 )）
struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    /// 二項演算を 1 回でも評価したか（数値だけ・括弧だけの入力に候補を出さないため）
    used_operator: bool,
}

impl Parser {
    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.pos).copied()
    }

    fn expr(&mut self, depth: usize) -> Option<f64> {
        let mut value = self.term(depth)?;
        while let Some(Token::Op(op @ ('+' | '-'))) = self.peek() {
            self.pos += 1;
            let rhs = self.term(depth)?;
            self.used_operator = true;
            value = if op == '+' { value + rhs } else { value - rhs };
            if !value.is_finite() {
                return None;
            }
        }
        Some(value)
    }

    fn term(&mut self, depth: usize) -> Option<f64> {
        let mut value = self.factor(depth)?;
        while let Some(Token::Op(op @ ('*' | '/'))) = self.peek() {
            self.pos += 1;
            let rhs = self.factor(depth)?;
            self.used_operator = true;
            if op == '*' {
                value *= rhs;
            } else {
                if rhs == 0.0 {
                    return None;
                }
                value /= rhs;
            }
            if !value.is_finite() {
                return None;
            }
        }
        Some(value)
    }

    fn factor(&mut self, depth: usize) -> Option<f64> {
        match self.peek()? {
            Token::Num(n) => {
                self.pos += 1;
                Some(n)
            }
            Token::LParen => {
                if depth >= MAX_DEPTH {
                    return None;
                }
                self.pos += 1;
                let value = self.expr(depth + 1)?;
                if self.peek() != Some(Token::RParen) {
                    return None;
                }
                self.pos += 1;
                Some(value)
            }
            _ => None,
        }
    }
}

/// 式を評価して、表示用の文字列で返す。式として成り立たない・演算子が無い・0 除算・
/// 桁あふれのときは `None`
pub(crate) fn evaluate(input: &str) -> Option<String> {
    let normalized: String = input
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(normalize)
        .collect();
    // 末尾の `=` は 1 つだけ許す
    let expr = normalized.strip_suffix('=').unwrap_or(&normalized);
    if expr.is_empty() || expr.chars().count() > MAX_EXPR_CHARS {
        return None;
    }
    let tokens = tokenize(expr)?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        used_operator: false,
    };
    let value = parser.expr(0)?;
    if parser.pos != parser.tokens.len() || !parser.used_operator {
        return None;
    }
    format_result(value)
}

/// 計算結果を整形する。整数は整数、小数は有効桁 10 桁で丸めて末尾の 0 を落とす。
/// 絶対値が `MAX_RESULT` 以上・非有限は `None`
fn format_result(x: f64) -> Option<String> {
    if !x.is_finite() || x.abs() >= MAX_RESULT {
        return None;
    }
    if x == 0.0 {
        return Some("0".to_string());
    }
    let magnitude = x.abs().log10().floor() as i32;
    let decimals = (SIGNIFICANT_DIGITS - 1 - magnitude).clamp(0, 15) as usize;
    let s = format!("{x:.decimals$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    };
    Some(if s == "-0" { "0".to_string() } else { s })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_binary_and_multi_term() {
        assert_eq!(evaluate("1+2").as_deref(), Some("3"));
        assert_eq!(evaluate("1+2*3").as_deref(), Some("7"));
        assert_eq!(evaluate("(1+2)*3").as_deref(), Some("9"));
        assert_eq!(evaluate("10-4-3").as_deref(), Some("3"));
        assert_eq!(evaluate("2*(3+4)*5").as_deref(), Some("70"));
        assert_eq!(evaluate("((1+2))*2").as_deref(), Some("6"));
    }

    #[test]
    fn formats_decimals_with_ten_significant_digits() {
        assert_eq!(evaluate("7/2").as_deref(), Some("3.5"));
        assert_eq!(evaluate("10/3").as_deref(), Some("3.333333333"));
        assert_eq!(evaluate("1/3").as_deref(), Some("0.3333333333"));
        assert_eq!(evaluate("0.1+0.2").as_deref(), Some("0.3"));
        assert_eq!(evaluate("1.1+2.2").as_deref(), Some("3.3"));
        assert_eq!(evaluate("100/8").as_deref(), Some("12.5"));
    }

    #[test]
    fn accepts_trailing_equal_and_fullwidth() {
        assert_eq!(evaluate("1+2=").as_deref(), Some("3"));
        assert_eq!(evaluate("1+2＝").as_deref(), Some("3"));
        assert_eq!(evaluate("１＋２").as_deref(), Some("3"));
        assert_eq!(evaluate("６÷４").as_deref(), Some("1.5"));
        assert_eq!(evaluate("3×4").as_deref(), Some("12"));
        assert_eq!(evaluate("3＊4").as_deref(), Some("12"));
        assert_eq!(evaluate("8／2").as_deref(), Some("4"));
        assert_eq!(evaluate("（１＋２）×３").as_deref(), Some("9"));
        assert_eq!(evaluate("1.5＋1．5").as_deref(), Some("3"));
    }

    #[test]
    fn rejects_invalid_or_unsafe() {
        // 0 除算
        assert_eq!(evaluate("5/0"), None);
        assert_eq!(evaluate("1/(2-2)"), None);
        // 演算子が無い（数値・括弧だけ）
        assert_eq!(evaluate("3"), None);
        assert_eq!(evaluate("(3)"), None);
        assert_eq!(evaluate("3="), None);
        // 先頭の符号・べき乗・連続する演算子
        assert_eq!(evaluate("-3+4"), None);
        assert_eq!(evaluate("2**3"), None);
        assert_eq!(evaluate("2^3"), None);
        assert_eq!(evaluate("1++2"), None);
        // 演算子の前後が空・括弧の不一致・空
        assert_eq!(evaluate("+4"), None);
        assert_eq!(evaluate("4+"), None);
        assert_eq!(evaluate("(1+2"), None);
        assert_eq!(evaluate("1+2)"), None);
        assert_eq!(evaluate("()"), None);
        assert_eq!(evaluate(""), None);
        assert_eq!(evaluate("="), None);
        assert_eq!(evaluate("1+2=="), None);
        // 数値の形式・数字以外
        assert_eq!(evaluate("1.2.3+1"), None);
        assert_eq!(evaluate(".5+1"), None);
        assert_eq!(evaluate("abc"), None);
        assert_eq!(evaluate("1+a"), None);
    }

    #[test]
    fn rejects_overflow_and_deep_nesting() {
        assert_eq!(evaluate("999999999999999*999999999999999"), None);
        assert_eq!(evaluate("1234567890123456+1"), None);
        let deep = format!("{}1+1{}", "(".repeat(40), ")".repeat(40));
        assert_eq!(evaluate(&deep), None);
        let long = vec!["1"; 40].join("+");
        assert_eq!(evaluate(&long), None);
    }
}

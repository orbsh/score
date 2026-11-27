//! Line-aware lexer: the parser groups tokens by line.

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Ident(String),
    Int(i64),
    Str(String),
    Arrow,
    Branch,
    CondOpen,
    CondClose,
    Lt,
    Gt,
    Le,
    Ge,
    Pipe,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Eq,
    EqEq,
    Ne,
    Bang,
    And,
    Or,
    Plus,
    Minus,
    Star,
    Slash,
    At,
    Const,
    Def,
}

pub fn lex(src: &str) -> Result<Vec<(usize, Tok)>, String> {
    let b = src.as_bytes();
    let mut i = 0usize;
    let mut line = 1usize;
    let mut out = Vec::new();

    while i < b.len() {
        let c = b[i] as char;
        match c {
            '\n' => {
                line += 1;
                i += 1;
            }
            ' ' | '\t' | '\r' => i += 1,
            '#' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            '"' => {
                let start = line;
                i += 1;
                let mut s = String::new();
                loop {
                    if i >= b.len() {
                        return Err(format!("line {start}: unterminated string"));
                    }
                    match b[i] as char {
                        '"' => {
                            i += 1;
                            break;
                        }
                        '\\' => {
                            i += 1;
                            if i >= b.len() {
                                return Err(format!("line {start}: unterminated escape"));
                            }
                            match b[i] as char {
                                'n' => s.push('\n'),
                                't' => s.push('\t'),
                                '\\' => s.push('\\'),
                                '"' => s.push('"'),
                                other => {
                                    return Err(format!("line {start}: unknown escape \\{other}"));
                                }
                            }
                            i += 1;
                        }
                        ch => {
                            if ch.is_ascii() {
                                s.push(ch);
                                i += 1;
                            } else {
                                let rest = std::str::from_utf8(&b[i..])
                                    .map_err(|_| format!("line {start}: bad utf-8"))?;
                                let c = rest.chars().next().expect("non-empty");
                                s.push(c);
                                i += c.len_utf8();
                            }
                        }
                    }
                }
                out.push((start, Tok::Str(s)));
            }
            '0'..='9' => {
                let start = i;
                while i < b.len() && (b[i] as char).is_ascii_digit() {
                    i += 1;
                }
                let text =
                    std::str::from_utf8(&b[start..i]).map_err(|_| "bad number".to_string())?;
                let n: i64 = text
                    .parse()
                    .map_err(|_| format!("line {line}: bad number `{text}`"))?;
                out.push((line, Tok::Int(n)));
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < b.len() && ((b[i] as char).is_alphanumeric() || b[i] == b'_') {
                    i += 1;
                }
                let text =
                    std::str::from_utf8(&b[start..i]).map_err(|_| "bad identifier".to_string())?;
                let tok = match text {
                    "const" => Tok::Const,
                    "def" => Tok::Def,
                    other => Tok::Ident(other.to_string()),
                };
                out.push((line, tok));
            }
            '-' => {
                if src[i..].starts_with("-->") {
                    out.push((line, Tok::Branch));
                    i += 3;
                } else if src[i..].starts_with("->") {
                    out.push((line, Tok::Arrow));
                    i += 2;
                } else if src[i..].starts_with("-[") {
                    out.push((line, Tok::CondOpen));
                    i += 2;
                } else {
                    out.push((line, Tok::Minus));
                    i += 1;
                }
            }
            ']' => {
                if !src[i..].starts_with("]->") {
                    return Err(format!("line {line}: expected `]->`"));
                }
                out.push((line, Tok::CondClose));
                i += 3;
            }
            '<' => {
                if src[i..].starts_with("<=") {
                    out.push((line, Tok::Le));
                    i += 2;
                } else {
                    out.push((line, Tok::Lt));
                    i += 1;
                }
            }
            '>' => {
                if src[i..].starts_with(">=") {
                    out.push((line, Tok::Ge));
                    i += 2;
                } else {
                    out.push((line, Tok::Gt));
                    i += 1;
                }
            }
            '|' => {
                if src[i..].starts_with("||") {
                    out.push((line, Tok::Or));
                    i += 2;
                } else {
                    out.push((line, Tok::Pipe));
                    i += 1;
                }
            }
            '&' => {
                if src[i..].starts_with("&&") {
                    out.push((line, Tok::And));
                    i += 2;
                } else {
                    return Err(format!("line {line}: expected `&&`"));
                }
            }
            '=' => {
                if src[i..].starts_with("==") {
                    out.push((line, Tok::EqEq));
                    i += 2;
                } else {
                    out.push((line, Tok::Eq));
                    i += 1;
                }
            }
            '!' => {
                if src[i..].starts_with("!=") {
                    out.push((line, Tok::Ne));
                    i += 2;
                } else {
                    out.push((line, Tok::Bang));
                    i += 1;
                }
            }
            '(' => {
                out.push((line, Tok::LParen));
                i += 1;
            }
            ')' => {
                out.push((line, Tok::RParen));
                i += 1;
            }
            '{' => {
                out.push((line, Tok::LBrace));
                i += 1;
            }
            '}' => {
                out.push((line, Tok::RBrace));
                i += 1;
            }
            ',' => {
                out.push((line, Tok::Comma));
                i += 1;
            }
            ':' => {
                out.push((line, Tok::Colon));
                i += 1;
            }
            '@' => {
                out.push((line, Tok::At));
                i += 1;
            }
            '+' => {
                out.push((line, Tok::Plus));
                i += 1;
            }
            '*' => {
                out.push((line, Tok::Star));
                i += 1;
            }
            '/' => {
                out.push((line, Tok::Slash));
                i += 1;
            }
            other => return Err(format!("line {line}: unexpected character `{other}`")),
        }
    }
    Ok(out)
}

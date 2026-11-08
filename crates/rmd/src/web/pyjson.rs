//! Python's `json.loads`, so requests are accepted and rejected exactly like
//! the original browser API and its error messages read the same
//! (`Expecting value: line 1 column 1 (char 0)`). Follows CPython's C scanner
//! (`_json.c`): positions count characters, `NaN`/`Infinity` are accepted
//! (they become `null`, which JSON values cannot hold otherwise), and
//! duplicate keys keep their first position with the last value.

use serde_json::{Map, Number, Value};

/// Nesting deeper than this is rejected instead of overflowing the stack.
const MAX_DEPTH: usize = 900;

enum Fail {
    /// No JSON value starts at this byte offset (Python's `StopIteration`).
    Stop(usize),
    /// A decode error at a byte offset.
    Error(&'static str, usize),
}

type Scan<T> = Result<(T, usize), Fail>;

struct Scanner<'a> {
    text: &'a str,
    bytes: &'a [u8],
    depth: usize,
}

fn is_ws(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

fn hex_value(byte: u8) -> Option<u32> {
    (byte as char).to_digit(16)
}

impl Scanner<'_> {
    fn skip_ws(&self, mut idx: usize) -> usize {
        while idx < self.bytes.len() && is_ws(self.bytes[idx]) {
            idx += 1;
        }
        idx
    }

    fn starts_with(&self, idx: usize, literal: &str) -> bool {
        self.bytes[idx..].starts_with(literal.as_bytes())
    }

    fn scan_once(&mut self, idx: usize) -> Scan<Value> {
        let Some(&first) = self.bytes.get(idx) else {
            return Err(Fail::Stop(idx));
        };
        match first {
            b'"' => return self.scan_string(idx + 1).map(|(s, end)| (Value::String(s), end)),
            b'{' => return self.nested(|scanner| scanner.parse_object(idx + 1)),
            b'[' => return self.nested(|scanner| scanner.parse_array(idx + 1)),
            b'n' if self.starts_with(idx, "null") => return Ok((Value::Null, idx + 4)),
            b't' if self.starts_with(idx, "true") => return Ok((Value::Bool(true), idx + 4)),
            b'f' if self.starts_with(idx, "false") => return Ok((Value::Bool(false), idx + 5)),
            b'N' if self.starts_with(idx, "NaN") => return Ok((Value::Null, idx + 3)),
            b'I' if self.starts_with(idx, "Infinity") => return Ok((Value::Null, idx + 8)),
            b'-' if self.starts_with(idx, "-Infinity") => return Ok((Value::Null, idx + 9)),
            _ => {}
        }
        self.match_number(idx)
    }

    fn nested(&mut self, parse: impl FnOnce(&mut Self) -> Scan<Value>) -> Scan<Value> {
        if self.depth >= MAX_DEPTH {
            return Err(Fail::Error("maximum recursion depth exceeded while decoding a JSON document", 0));
        }
        self.depth += 1;
        let result = parse(self);
        self.depth -= 1;
        result
    }

    fn digit_at(&self, idx: usize) -> bool {
        self.bytes.get(idx).is_some_and(u8::is_ascii_digit)
    }

    fn match_number(&self, start: usize) -> Scan<Value> {
        let len = self.bytes.len();
        let mut idx = start;
        if self.bytes[idx] == b'-' {
            idx += 1;
            if idx >= len {
                return Err(Fail::Stop(start));
            }
        }
        match self.bytes[idx] {
            b'1'..=b'9' => {
                idx += 1;
                while self.digit_at(idx) {
                    idx += 1;
                }
            }
            b'0' => idx += 1,
            _ => return Err(Fail::Stop(start)),
        }
        let mut is_float = false;
        if idx + 1 < len && self.bytes[idx] == b'.' && self.digit_at(idx + 1) {
            is_float = true;
            idx += 2;
            while self.digit_at(idx) {
                idx += 1;
            }
        }
        if idx + 1 < len && matches!(self.bytes[idx], b'e' | b'E') {
            let e_start = idx;
            idx += 1;
            if idx + 1 < len && matches!(self.bytes[idx], b'-' | b'+') {
                idx += 1;
            }
            while self.digit_at(idx) {
                idx += 1;
            }
            if self.bytes[idx - 1].is_ascii_digit() {
                is_float = true;
            } else {
                idx = e_start;
            }
        }
        let literal = &self.text[start..idx];
        let value = if is_float {
            float_value(literal.parse::<f64>().unwrap_or(f64::NAN))
        } else if let Ok(int) = literal.parse::<i64>() {
            Value::from(int)
        } else if let Ok(int) = literal.parse::<u64>() {
            Value::from(int)
        } else {
            // Python keeps arbitrarily large integers; JSON values cannot.
            float_value(literal.parse::<f64>().unwrap_or(f64::NAN))
        };
        Ok((value, idx))
    }

    fn scan_string(&self, mut end: usize) -> Scan<String> {
        let begin = end - 1;
        let len = self.bytes.len();
        let mut out = String::new();
        loop {
            let mut next = end;
            while next < len {
                let byte = self.bytes[next];
                if byte == b'"' || byte == b'\\' {
                    break;
                }
                if byte <= 0x1f {
                    return Err(Fail::Error("Invalid control character at", next));
                }
                next += 1;
            }
            if next == len {
                return Err(Fail::Error("Unterminated string starting at", begin));
            }
            let terminator = self.bytes[next];
            out.push_str(&self.text[end..next]);
            next += 1;
            if terminator == b'"' {
                end = next;
                break;
            }
            if next == len {
                return Err(Fail::Error("Unterminated string starting at", begin));
            }
            let escape = self.bytes[next];
            if escape != b'u' {
                end = next + 1;
                let ch = match escape {
                    b'"' => '"',
                    b'\\' => '\\',
                    b'/' => '/',
                    b'b' => '\u{8}',
                    b'f' => '\u{c}',
                    b'n' => '\n',
                    b'r' => '\r',
                    b't' => '\t',
                    _ => return Err(Fail::Error("Invalid \\escape", end - 2)),
                };
                out.push(ch);
                continue;
            }
            next += 1;
            end = next + 4;
            if end >= len {
                return Err(Fail::Error("Invalid \\uXXXX escape", next - 1));
            }
            let mut code = 0u32;
            for &byte in &self.bytes[next..end] {
                let Some(digit) = hex_value(byte) else {
                    return Err(Fail::Error("Invalid \\uXXXX escape", end - 5));
                };
                code = (code << 4) | digit;
            }
            if (0xd800..=0xdbff).contains(&code)
                && end + 6 < len
                && self.bytes[end] == b'\\'
                && self.bytes[end + 1] == b'u'
            {
                let mut low = 0u32;
                for &byte in &self.bytes[end + 2..end + 6] {
                    let Some(digit) = hex_value(byte) else {
                        return Err(Fail::Error("Invalid \\uXXXX escape", end + 1));
                    };
                    low = (low << 4) | digit;
                }
                if (0xdc00..=0xdfff).contains(&low) {
                    code = 0x10000 + (((code - 0xd800) << 10) | (low - 0xdc00));
                    end += 6;
                }
            }
            // Python keeps lone surrogates; Rust strings cannot hold them.
            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
        }
        Ok((out, end))
    }

    fn parse_object(&mut self, idx: usize) -> Scan<Value> {
        let len = self.bytes.len();
        let mut map = Map::new();
        let mut idx = self.skip_ws(idx);
        if idx >= len || self.bytes[idx] != b'}' {
            loop {
                if idx >= len || self.bytes[idx] != b'"' {
                    return Err(Fail::Error("Expecting property name enclosed in double quotes", idx));
                }
                let (key, next) = self.scan_string(idx + 1)?;
                idx = self.skip_ws(next);
                if idx >= len || self.bytes[idx] != b':' {
                    return Err(Fail::Error("Expecting ':' delimiter", idx));
                }
                idx = self.skip_ws(idx + 1);
                let (value, next) = self.scan_once(idx)?;
                map.insert(key, value);
                idx = self.skip_ws(next);
                if idx < len && self.bytes[idx] == b'}' {
                    break;
                }
                if idx >= len || self.bytes[idx] != b',' {
                    return Err(Fail::Error("Expecting ',' delimiter", idx));
                }
                idx = self.skip_ws(idx + 1);
            }
        }
        Ok((Value::Object(map), idx + 1))
    }

    fn parse_array(&mut self, idx: usize) -> Scan<Value> {
        let len = self.bytes.len();
        let mut items = Vec::new();
        let mut idx = self.skip_ws(idx);
        if idx >= len || self.bytes[idx] != b']' {
            loop {
                let (value, next) = self.scan_once(idx)?;
                items.push(value);
                idx = self.skip_ws(next);
                if idx < len && self.bytes[idx] == b']' {
                    break;
                }
                if idx >= len || self.bytes[idx] != b',' {
                    return Err(Fail::Error("Expecting ',' delimiter", idx));
                }
                idx = self.skip_ws(idx + 1);
            }
        }
        Ok((Value::Array(items), idx + 1))
    }
}

fn float_value(value: f64) -> Value {
    Number::from_f64(value).map(Value::Number).unwrap_or(Value::Null)
}

/// `JSONDecodeError.__str__`: the message with line, column and character
/// position (characters, not bytes).
fn error_message(text: &str, message: &str, byte_pos: usize) -> String {
    let before = &text[..byte_pos.min(text.len())];
    let pos = before.chars().count();
    let lineno = before.matches('\n').count() + 1;
    let colno = match before.rfind('\n') {
        Some(newline) => before[newline + 1..].chars().count() + 1,
        None => pos + 1,
    };
    format!("{message}: line {lineno} column {colno} (char {pos})")
}

/// `json.loads(text)`; the error is `str(JSONDecodeError)`.
pub fn loads(text: &str) -> Result<Value, String> {
    if text.starts_with('\u{feff}') {
        return Err(error_message(text, "Unexpected UTF-8 BOM (decode using utf-8-sig)", 0));
    }
    let mut scanner = Scanner { text, bytes: text.as_bytes(), depth: 0 };
    let start = scanner.skip_ws(0);
    match scanner.scan_once(start) {
        Ok((value, end)) => {
            let end = scanner.skip_ws(end);
            if end != text.len() {
                return Err(error_message(text, "Extra data", end));
            }
            Ok(value)
        }
        Err(Fail::Stop(pos)) => Err(error_message(text, "Expecting value", pos)),
        Err(Fail::Error(message, pos)) => Err(error_message(text, message, pos)),
    }
}

/// `json.loads(bytes)`: a UTF-8 byte-order mark is skipped and the bytes must
/// be valid UTF-8.
pub fn loads_bytes(bytes: &[u8]) -> Result<Value, String> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let text = std::str::from_utf8(bytes).map_err(|e| format!("'utf-8' codec can't decode bytes: {e}"))?;
    loads(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn err(text: &str) -> String {
        loads(text).unwrap_err()
    }

    #[test]
    fn parses_like_python() {
        assert_eq!(loads(r#" {"a": [1, 2.5, -0, true, null, "xé\n"], "a": 3} "#).unwrap(), json!({"a": 3}));
        assert_eq!(loads(r#"{"b": 1, "a": 2, "b": 3}"#).unwrap().to_string(), r#"{"b":3,"a":2}"#);
        assert_eq!(loads(r#""😀""#).unwrap(), json!("😀"));
        assert_eq!(loads("[NaN, Infinity, -Infinity]").unwrap(), json!([null, null, null]));
        assert_eq!(loads("1e2").unwrap(), json!(100.0));
        assert_eq!(loads("12345678901234567890").unwrap(), json!(12345678901234567890u64));
        assert_eq!(loads_bytes(b"\xef\xbb\xbf{}").unwrap(), json!({}));
    }

    #[test]
    fn errors_match_python_messages() {
        // Expected strings come from CPython 3.12's json.loads.
        assert_eq!(err(""), "Expecting value: line 1 column 1 (char 0)");
        assert_eq!(err("{not json"), "Expecting property name enclosed in double quotes: line 1 column 2 (char 1)");
        assert_eq!(err("[1,]"), "Expecting value: line 1 column 4 (char 3)");
        assert_eq!(err(r#"{"a":1,}"#), "Expecting property name enclosed in double quotes: line 1 column 8 (char 7)");
        assert_eq!(err(r#"{"a" 1}"#), "Expecting ':' delimiter: line 1 column 6 (char 5)");
        assert_eq!(err("[1 2]"), "Expecting ',' delimiter: line 1 column 4 (char 3)");
        assert_eq!(err("{} x"), "Extra data: line 1 column 4 (char 3)");
        assert_eq!(err("1e+"), "Extra data: line 1 column 2 (char 1)");
        assert_eq!(err("\"abc"), "Unterminated string starting at: line 1 column 1 (char 0)");
        assert_eq!(err("\"a\u{1}\""), "Invalid control character at: line 1 column 3 (char 2)");
        assert_eq!(err(r#""\x""#), "Invalid \\escape: line 1 column 2 (char 1)");
        assert_eq!(err(r#""\u12""#), "Invalid \\uXXXX escape: line 1 column 3 (char 2)");
        assert_eq!(err(r#""\u12zz""#), "Invalid \\uXXXX escape: line 1 column 3 (char 2)");
        assert_eq!(err("[\n  1,\n  ]"), "Expecting value: line 3 column 3 (char 9)");
        assert_eq!(err("\"é\" x"), "Extra data: line 1 column 5 (char 4)");
        assert_eq!(err("\u{feff}{}"), "Unexpected UTF-8 BOM (decode using utf-8-sig): line 1 column 1 (char 0)");
        assert_eq!(err("-"), "Expecting value: line 1 column 1 (char 0)");
        assert_eq!(err("nul"), "Expecting value: line 1 column 1 (char 0)");
        assert_eq!(err(r#""\ud83d\uzzzz""#), "Invalid \\uXXXX escape: line 1 column 9 (char 8)");
        assert_eq!(err("[1.]"), "Expecting ',' delimiter: line 1 column 3 (char 2)");
        assert_eq!(err("-01"), "Extra data: line 1 column 3 (char 2)");
        assert_eq!(err("1.5e"), "Extra data: line 1 column 4 (char 3)");
        assert_eq!(err(r#"{"a":}"#), "Expecting value: line 1 column 6 (char 5)");
        assert_eq!(err("["), "Expecting value: line 1 column 2 (char 1)");
        assert_eq!(err("{"), "Expecting property name enclosed in double quotes: line 1 column 2 (char 1)");
        assert_eq!(err(r#"{"a""#), "Expecting ':' delimiter: line 1 column 5 (char 4)");
        assert_eq!(err("  "), "Expecting value: line 1 column 3 (char 2)");
        assert_eq!(err("\"\\"), "Unterminated string starting at: line 1 column 1 (char 0)");
        assert_eq!(err(r#""\u""#), "Invalid \\uXXXX escape: line 1 column 3 (char 2)");
    }
}

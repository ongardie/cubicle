use std::ffi::OsStr;

use crate::somehow::{somehow as anyhow, Context, Result};

pub struct FilenameEncoder {
    input: String,
}

impl FilenameEncoder {
    pub fn new() -> Self {
        Self {
            input: String::new(),
        }
    }

    pub fn push(mut self, s: &str) -> Self {
        self.input.push_str(s);
        self
    }

    /// Returns a string for use as a filename.
    ///
    /// This encodes filenames so that they do not:
    ///  - equal "." or "..",
    ///  - start with '-', or
    ///  - contain '/' or '\\' or ASCII control characters.
    pub fn encode(self) -> String {
        use std::fmt::Write;

        if self.input == "." {
            return format!("%{:02x}", b'.');
        }
        if self.input == ".." {
            return format!("%{:02x}%{:02x}", b'.', b'.');
        }

        let mut buf = String::new();
        for (i, char) in self.input.chars().enumerate() {
            if char.is_ascii()
                && (char.is_control()
                    || matches!(char, '%' | '/' | '\\')
                    || (i == 0 && char == '-'))
            {
                write!(buf, "%{:02x}", char as u8).unwrap();
            } else {
                buf.push(char)
            }
        }
        buf
    }

    /// Returns the string that is encoded in the given filename, if valid.
    pub fn decode(filename: &OsStr) -> Result<String> {
        let mut buf: Vec<u8> = Vec::new();
        let mut bytes = filename
            .to_str()
            .ok_or_else(|| anyhow!("invalid UTF-8"))?
            .bytes();
        while let Some(byte) = bytes.next() {
            if byte == b'%' {
                match (
                    bytes.next().and_then(from_hexdigit),
                    bytes.next().and_then(from_hexdigit),
                ) {
                    (Some(hi), Some(lo)) => buf.push((hi << 4) | lo),
                    _ => return Err(anyhow!("% sequence invalid")),
                }
            } else {
                buf.push(byte);
            }
        }
        String::from_utf8(buf).enough_context()
    }
}

/// Similar to `char::to_digit(16)` but for `u8`.
pub fn from_hexdigit(byte: u8) -> Option<u8> {
    match byte {
        b'0' => Some(0x0),
        b'1' => Some(0x1),
        b'2' => Some(0x2),
        b'3' => Some(0x3),
        b'4' => Some(0x4),
        b'5' => Some(0x5),
        b'6' => Some(0x6),
        b'7' => Some(0x7),
        b'8' => Some(0x8),
        b'9' => Some(0x9),
        b'a' => Some(0xa),
        b'b' => Some(0xb),
        b'c' => Some(0xc),
        b'd' => Some(0xd),
        b'e' => Some(0xe),
        b'f' => Some(0xf),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn passed(pass: bool) -> &'static str {
        if pass {
            "(pass)"
        } else {
            "(fail)"
        }
    }

    #[test]
    fn filename_encoder() {
        let fail = [
            ("abc", "abc"),
            (".", "%2e"),
            ("..", "%2e%2e"),
            ("...", "..."),
            ("-hi", "%2dhi"),
            ("--hi", "%2d-hi"),
            ("hi-", "hi-"),
            ("abc\\def/ghi", "abc%5cdef%2fghi"),
            ("abc def", "abc def"),
            ("çbç", "çbç"),
        ]
        .into_iter()
        .any(|(input, expected)| {
            let encoded = FilenameEncoder::new().push(input).encode();
            let decoded_from_encoding = FilenameEncoder::decode(OsStr::new(&encoded));
            let decoded_from_expected = FilenameEncoder::decode(OsStr::new(expected));
            let fail = encoded != expected || decoded_from_expected.as_deref().ok() != Some(input);
            println!("test:            {}", if fail { "fail" } else { "pass" });
            println!("input:           {input}");
            println!("expected:        {expected}");
            println!(
                "encoded:         {} {}",
                encoded,
                passed(encoded == expected)
            );
            println!("decoded");
            match decoded_from_encoding {
                Ok(decoded) => println!(
                    "  from encoded:  {} {}",
                    decoded.as_str(),
                    passed(decoded == input)
                ),
                Err(e) => println!("  from encoded:  Error: {e} (fail)",),
            }
            match decoded_from_expected {
                Ok(decoded) => println!(
                    "  from expected: {} {}",
                    decoded.as_str(),
                    passed(decoded == input)
                ),
                Err(e) => println!("  from expected: Error: {e} (fail)",),
            }
            println!();
            fail
        });

        assert!(!fail, "at least one encoding/decoding failure");
    }
}

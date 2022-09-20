use std::fmt::{self, Write};
use std::ops::Deref;

use crate::encoding::from_hexdigit;

/// An encoding for arbitrary strings to be used as Docker names.
///
/// Docker container, image, and volume names must all begin with
/// `[a-zA-Z0-9]` and must continue with at least one character from
/// `[a-zA-Z0-9_\.-]`. This implies that all Docker names must be at least
/// two characters long and that Docker names cannot contain Unicode.
///
/// The encoding implemented here allows Docker-compatible strings to be
/// used verbatim and it aims for humans to be able to recognize allowable
/// portions of Docker-incompatible strings.
#[derive(Debug, Eq, PartialEq)]
pub struct DockerName {
    decoded: String,
}

impl DockerName {
    pub fn new(decoded: String) -> Self {
        Self { decoded }
    }

    pub fn decoded(&self) -> &str {
        &self.decoded
    }

    pub fn needs_encoding(&self) -> bool {
        if self.decoded.len() < 2 {
            return true;
        }
        let mut iter = self.decoded.bytes();
        if !iter
            .next()
            .expect("non-empty string")
            .is_ascii_alphanumeric()
        {
            return true;
        }
        iter.any(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'_' | b'-'))
    }

    pub fn encoded(&self) -> String {
        if !self.needs_encoding() {
            return self.decoded.clone();
        }

        if self.decoded.is_empty() {
            return String::from("0..");
        }

        let mut buf = String::new();

        let first_allowed = self.decoded.bytes().next().unwrap().is_ascii_alphanumeric();
        if !first_allowed {
            buf.push('0');
        }

        for byte in self.decoded.bytes() {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-') {
                buf.push(char::from(byte));
            } else {
                write!(buf, ".{:02x}", byte).unwrap();
            }
        }

        if self.decoded.len() < 2 || !first_allowed {
            buf.push('.');
        }

        buf
    }

    /// Tries to decode the given string, returning `None` if it could not
    /// have been produced by this encoding.
    pub fn decode(s: &str) -> Option<Self> {
        if s == "0.." {
            return Some(Self::new(String::new()));
        }

        let mut bytes = match s.strip_suffix('.') {
            Some(s) if s.len() == 1 => s,
            Some(s) => s.strip_prefix('0')?,
            None => s,
        }
        .bytes();

        let mut buf: Vec<u8> = Vec::new();
        while let Some(byte) = bytes.next() {
            if byte == b'.' {
                let hi = from_hexdigit(bytes.next()?)?;
                let lo = from_hexdigit(bytes.next()?)?;
                buf.push((hi << 4) | lo);
            } else {
                buf.push(byte);
            }
        }

        let decoded = Self::new(String::from_utf8(buf).ok()?);
        if decoded.encoded() != s {
            return None;
        }
        Some(decoded)
    }
}

impl fmt::Display for DockerName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.needs_encoding() {
            write!(
                f,
                "{:?} (encoded for Docker as {:?})",
                self.decoded,
                self.encoded()
            )
        } else {
            write!(f, "{:?}", self.decoded)
        }
    }
}

macro_rules! name {
    ($name:ident) => {
        #[derive(Debug)]
        pub struct $name(DockerName);

        impl $name {
            pub fn new(s: String) -> Self {
                Self(DockerName::new(s))
            }

            #[allow(dead_code)]
            pub fn decode(s: &str) -> Option<Self> {
                DockerName::decode(s).map(Self)
            }
        }

        impl Deref for $name {
            type Target = DockerName;
            fn deref(&self) -> &DockerName {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

name!(ContainerName);
name!(ImageName);
name!(VolumeName);

#[cfg(test)]
mod tests {
    use super::*;

    fn passed(pass: bool) -> &'static str {
        if pass {
            "(pass)"
        } else {
            "(fail)"
        }
    }

    #[test]
    fn encoding() {
        let fail = [
            ("abc", "abc"),
            ("a_-", "a_-"),
            ("a_-.c", "a_-.2ec"),
            ("ab.", "ab.2e"),
            ("abç", "ab.c3.a7"),
            ("a\n", "a.0a"),
            ("a", "a."),
            ("a.", "a.2e"),
            ("ç", "0.c3.a7."),
            (".", "0.2e."),
            ("", "0.."),
            ("0", "0."),
            ("0.", "0.2e"),
            ("0..", "0.2e.2e"),
            ("0...", "0.2e.2e.2e"),
            ("9", "9."),
            ("9.", "9.2e"),
            ("9..", "9.2e.2e"),
        ]
        .into_iter()
        .any(|(input, expected)| {
            let name = DockerName::new(input.to_owned());
            let encoded = name.encoded();
            let decoded_from_encoding = DockerName::decode(&encoded).map(|n| n.decoded);
            let decoded_from_expected = DockerName::decode(expected).map(|n| n.decoded);
            let fail = encoded != expected
                || decoded_from_expected.as_deref() != Some(input)
                || name.needs_encoding() == (input == expected);
            println!("test:            {}", if fail { "fail" } else { "pass" });
            println!("input:           {input}");
            println!("expected:        {expected}");
            println!(
                "needs encoding:  {} {}",
                name.needs_encoding(),
                passed(name.needs_encoding() == (input != expected))
            );
            println!(
                "encoded:         {} {}",
                encoded,
                passed(encoded == expected)
            );
            println!("decoded");
            println!(
                "  from encoded:  {} {}",
                decoded_from_encoding.as_deref().unwrap_or("(None)"),
                passed(decoded_from_encoding.as_deref() == Some(input))
            );
            println!(
                "  from expected: {} {}",
                decoded_from_expected.as_deref().unwrap_or("(None)"),
                passed(decoded_from_expected.as_deref() == Some(input))
            );
            println!();
            fail
        });

        assert!(!fail, "at least one encoding/decoding failure");
    }

    #[test]
    fn decode() {
        assert_eq!(None, DockerName::decode("0..."), "too many dots");
        assert_eq!(None, DockerName::decode("0.0"), "two hex values per byte");
        assert_eq!(
            None,
            DockerName::decode("0.41"),
            "'A' does not need encoding"
        );
        assert_eq!(
            None,
            DockerName::decode("0.2A"),
            "encoding should be lowercase hex"
        );
        assert_eq!(None, DockerName::decode("0.9f"), "invalid UTF-8");
        assert_eq!(None, DockerName::decode("123."), "not our trailing dot");
    }
}

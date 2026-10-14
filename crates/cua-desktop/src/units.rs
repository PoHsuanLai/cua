//! The small units every cua crate shares: time on cuad's clock, process ids, global logical
//! coordinates and 32-byte digests.

use porter_core::Px;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Ms(pub u32);

/// An instant: milliseconds since cuad's clock origin. Never wall time; the run machine takes it
/// as an argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Stamp(pub u64);

/// A process id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Pid(pub u32);

/// One axis of a global logical position: may be negative (a window left of the primary output).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GlobalCoord(pub i32);

/// A window's frame in the compositor's global logical space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GlobalRect {
    /// Left edge.
    pub x: GlobalCoord,
    /// Top edge.
    pub y: GlobalCoord,
    /// Width in logical pixels.
    pub w: Px,
    /// Height in logical pixels.
    pub h: Px,
}

/// 32 bytes: a hash or digest. Serialises as 64 lowercase hex characters; `Debug` prints the
/// first four bytes only.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Digest32(pub [u8; 32]);

/// Why a string is not a [`Digest32`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a digest is 64 lowercase hex characters")]
pub struct DigestError;

impl TryFrom<String> for Digest32 {
    type Error = DigestError;

    fn try_from(text: String) -> Result<Self, DigestError> {
        let bytes = text.as_bytes();
        if bytes.len() != 64 {
            return Err(DigestError);
        }
        let nibble = |b: u8| match b {
            b'0'..=b'9' => Ok(b - b'0'),
            b'a'..=b'f' => Ok(b - b'a' + 10),
            _ => Err(DigestError),
        };
        let mut out = [0u8; 32];
        let (pairs, _) = bytes.as_chunks::<2>();
        for (slot, [high, low]) in out.iter_mut().zip(pairs) {
            *slot = (nibble(*high)? << 4) | nibble(*low)?;
        }
        Ok(Digest32(out))
    }
}

impl From<Digest32> for String {
    fn from(digest: Digest32) -> String {
        digest.0.iter().map(|b| format!("{b:02x}")).collect()
    }
}

impl fmt::Debug for Digest32 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Digest32({:02x}{:02x}{:02x}{:02x}..)",
            self.0[0], self.0[1], self.0[2], self.0[3]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_is_lowercase_hex_both_ways() {
        let mut raw = [0u8; 32];
        raw[0] = 0xab;
        raw[31] = 0x01;
        let digest = Digest32(raw);
        let text = String::from(digest);
        assert_eq!(text.len(), 64);
        assert!(text.starts_with("ab") && text.ends_with("01"));
        assert_eq!(Digest32::try_from(text), Ok(digest));
    }

    #[test]
    fn digest_refuses_what_is_not_64_lowercase_hex() {
        let cases = [
            ("", "empty"),
            ("ab", "short"),
            (&"AB".repeat(32), "uppercase"),
            (&"zz".repeat(32), "not hex"),
            (&"ab".repeat(33), "long"),
        ];
        for (text, why) in cases {
            assert_eq!(
                Digest32::try_from(text.to_owned()),
                Err(DigestError),
                "{why}"
            );
        }
    }

    #[test]
    fn units_serialise_as_bare_numbers() {
        assert_eq!(serde_json::to_string(&Ms(250)).unwrap(), "250");
        assert_eq!(serde_json::to_string(&Stamp(7)).unwrap(), "7");
        assert_eq!(serde_json::to_string(&GlobalCoord(-4)).unwrap(), "-4");
        let rect = GlobalRect {
            x: GlobalCoord(-10),
            y: GlobalCoord(5),
            w: Px(852),
            h: Px(532),
        };
        assert_eq!(
            serde_json::to_string(&rect).unwrap(),
            r#"{"x":-10,"y":5,"w":852,"h":532}"#
        );
    }
}

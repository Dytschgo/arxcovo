//! Strict encodings for the Compose channel and wiping of controlled buffers.

use std::fmt;
use std::sync::atomic::{Ordering, compiler_fence};

use serde::{Deserialize, Deserializer, Serialize, de};
use sha2::{Digest, Sha256};

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Overwrites bytes with zeros through volatile writes the optimizer keeps.
pub fn wipe_bytes(bytes: &mut [u8]) {
    for byte in bytes.iter_mut() {
        // SAFETY: `byte` is a valid, aligned and exclusive reference.
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    compiler_fence(Ordering::SeqCst);
}

pub(crate) fn wipe(bytes: &mut [u8]) {
    wipe_bytes(bytes);
}

pub(crate) fn wipe_string(text: &mut String) {
    // SAFETY: zero bytes keep the string valid UTF-8.
    wipe(unsafe { text.as_bytes_mut() });
    text.clear();
}

/// Base64 text that carries secret bytes. It is never formatted, and its
/// buffer is wiped when dropped. Deserialization accepts only a borrowed JSON
/// string slice (no escape sequences) so parser scratch buffers never hold
/// secret bytes.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct SecretText(String);

impl SecretText {
    pub fn new(text: String) -> Self {
        Self(text)
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    /// Moves the text out for non-secret payloads (such as manifest chunks)
    /// once the frame tag is known not to be a secret frame.
    pub(crate) fn into_non_secret_string(mut self) -> String {
        std::mem::take(&mut self.0)
    }
}

impl<'de> Deserialize<'de> for SecretText {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct BorrowedSecretVisitor;

        impl<'de> de::Visitor<'de> for BorrowedSecretVisitor {
            type Value = SecretText;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an unescaped base64 JSON string")
            }

            fn visit_borrowed_str<E: de::Error>(self, value: &'de str) -> Result<Self::Value, E> {
                // Allocate once at the exact length so no intermediate buffer
                // escapes the Drop wipe path.
                let mut text = String::with_capacity(value.len());
                text.push_str(value);
                Ok(SecretText(text))
            }

            fn visit_str<E: de::Error>(self, _value: &str) -> Result<Self::Value, E> {
                Err(E::custom("escaped secret text is forbidden"))
            }

            fn visit_string<E: de::Error>(self, mut value: String) -> Result<Self::Value, E> {
                wipe_string(&mut value);
                Err(E::custom("owned secret text is forbidden"))
            }
        }

        deserializer.deserialize_str(BorrowedSecretVisitor)
    }
}

impl Drop for SecretText {
    fn drop(&mut self) {
        wipe_string(&mut self.0);
    }
}

/// Decoded secret bytes, wiped when dropped.
pub(crate) struct SecretBytes(Vec<u8>);

impl SecretBytes {
    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

pub(crate) fn decode_secret(text: &SecretText, max: usize) -> Option<SecretBytes> {
    decode_base64(text.as_str(), max).map(SecretBytes)
}

const fn sextet(byte: u8) -> u8 {
    match byte {
        b'A'..=b'Z' => byte - b'A',
        b'a'..=b'z' => byte - b'a' + 26,
        b'0'..=b'9' => byte - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => 0xff,
    }
}

/// Encoded length of `len` bytes with padding.
pub const fn base64_len(len: usize) -> usize {
    len.div_ceil(3) * 4
}

/// Decodes canonical, padded standard Base64 of 1..=`max` bytes. The output
/// is allocated once at its exact size so no partial copy is left behind.
pub(crate) fn decode_base64(text: &str, max: usize) -> Option<Vec<u8>> {
    let input = text.as_bytes();
    if input.is_empty() || !input.len().is_multiple_of(4) || input.len() > base64_len(max) {
        return None;
    }
    let pad = match input {
        [.., b'=', b'='] => 2,
        [.., b'='] => 1,
        _ => 0,
    };
    let len = input.len() / 4 * 3 - pad;
    if len == 0 || len > max {
        return None;
    }
    let mut out = Vec::with_capacity(len);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for &byte in &input[..input.len() - pad] {
        let value = sextet(byte);
        if value == 0xff {
            wipe(&mut out);
            return None;
        }
        acc = (acc << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    // Canonical encodings leave the unused low bits at zero.
    if acc != 0 || out.len() != len {
        wipe(&mut out);
        return None;
    }
    Some(out)
}

/// Standard padded Base64, used by host peers and fixtures.
pub fn encode_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(base64_len(bytes.len()));
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * index)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Lowercase hexadecimal SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn is_lower_hex(text: &str, len: usize) -> bool {
    text.len() == len
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

pub(crate) fn is_base64_text(text: &str, max_decoded: usize) -> bool {
    !text.is_empty()
        && text.len().is_multiple_of(4)
        && text.len() <= base64_len(max_decoded)
        && text
            .bytes()
            .all(|byte| sextet(byte) != 0xff || byte == b'=')
}

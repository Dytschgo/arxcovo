//! Canonical v1 framing. Structural acceptance alone never authenticates a key.
use super::{Context, Error, Kind};
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};

pub(super) const HEADER: usize = 122;
pub(super) fn install(id: [u8; 16]) -> Vec<u8> {
    [b"ARXCTX01".as_slice(), &[1], &id].concat()
}
pub(super) fn installation(bytes: &[u8]) -> Result<[u8; 16], Error> {
    if bytes.len() != 25 || &bytes[..9] != b"ARXCTX01\x01" {
        return Err(Error::Corrupt);
    }
    bytes[9..].try_into().map_err(|_| Error::Corrupt)
}
pub(super) fn wrapping(bytes: &[u8]) -> Result<[u8; 32], Error> {
    bytes.try_into().map_err(|_| Error::Corrupt)
}
pub(super) fn lock(bytes: &[u8]) -> Result<(), Error> {
    if bytes.is_empty() {
        Ok(())
    } else {
        Err(Error::Corrupt)
    }
}
fn word(bytes: &[u8], at: usize) -> Result<usize, Error> {
    Ok(u32::from_be_bytes(
        bytes
            .get(at..at + 4)
            .ok_or(Error::Corrupt)?
            .try_into()
            .map_err(|_| Error::Corrupt)?,
    ) as usize)
}
pub(super) fn payload(kind: Kind, key: &[u8], cert: &[u8]) -> Result<Vec<u8>, Error> {
    if key.is_empty()
        || key.len() > 4096
        || (kind == Kind::Ca && (cert.is_empty() || cert.len() > 4096))
        || (kind == Kind::Member && !cert.is_empty())
    {
        return Err(Error::Corrupt);
    }
    let mut bytes = (key.len() as u32).to_be_bytes().to_vec();
    bytes.extend_from_slice(key);
    if kind == Kind::Ca {
        bytes.extend_from_slice(&(cert.len() as u32).to_be_bytes());
        bytes.extend_from_slice(cert);
    }
    Ok(bytes)
}
pub(super) fn parts(kind: Kind, bytes: &[u8]) -> Result<(&[u8], &[u8]), Error> {
    let k = word(bytes, 0)?;
    if !(1..=4096).contains(&k) {
        return Err(Error::Corrupt);
    }
    let key = bytes.get(4..4 + k).ok_or(Error::Corrupt)?;
    match kind {
        Kind::Member if bytes.len() == 4 + k => Ok((key, &[])),
        Kind::Ca => {
            let c = word(bytes, 4 + k)?;
            if !(1..=4096).contains(&c) || bytes.len() != 8 + k + c {
                return Err(Error::Corrupt);
            }
            Ok((key, &bytes[8 + k..]))
        }
        _ => Err(Error::Corrupt),
    }
}
pub(super) fn header(ctx: Context, nonce: [u8; 12], len: usize) -> Result<Vec<u8>, Error> {
    if !ctx.kind.bounds().contains(&len) {
        return Err(Error::Corrupt);
    }
    Ok([
        b"ARXKEY01".as_slice(),
        &[1, ctx.kind as u8],
        &ctx.install,
        &ctx.cluster,
        &ctx.root,
        &ctx.spki,
        &nonce,
        &(len as u32).to_be_bytes(),
    ]
    .concat())
}
pub(super) fn frame(bytes: &[u8]) -> Result<(Context, [u8; 12]), Error> {
    if bytes.len() < 138 || bytes.len() > 16384 || &bytes[..9] != b"ARXKEY01\x01" {
        return Err(Error::Corrupt);
    }
    let kind = match bytes[9] {
        1 => Kind::Ca,
        2 => Kind::Member,
        _ => return Err(Error::Corrupt),
    };
    let len = word(bytes, 118)?;
    if !kind.bounds().contains(&len) || len.checked_add(138) != Some(bytes.len()) {
        return Err(Error::Corrupt);
    }
    Ok((
        Context {
            kind,
            install: bytes[10..26].try_into().map_err(|_| Error::Corrupt)?,
            cluster: bytes[26..42].try_into().map_err(|_| Error::Corrupt)?,
            root: bytes[42..74].try_into().map_err(|_| Error::Corrupt)?,
            spki: bytes[74..106].try_into().map_err(|_| Error::Corrupt)?,
        },
        bytes[106..118].try_into().map_err(|_| Error::Corrupt)?,
    ))
}
fn cipher(key: &[u8; 32]) -> Result<LessSafeKey, Error> {
    Ok(LessSafeKey::new(
        UnboundKey::new(&aead::CHACHA20_POLY1305, key).map_err(|_| Error::Unavailable)?,
    ))
}
pub(super) fn seal(key: &[u8; 32], ctx: Context, plaintext: &[u8]) -> Result<Vec<u8>, Error> {
    seal_with_random(key, ctx, plaintext, |nonce| {
        getrandom::fill(nonce).map_err(|_| Error::Unavailable)
    })
}
fn seal_with_random(
    key: &[u8; 32],
    ctx: Context,
    plaintext: &[u8],
    random: impl FnOnce(&mut [u8]) -> Result<(), Error>,
) -> Result<Vec<u8>, Error> {
    parts(ctx.kind, plaintext)?;
    let mut nonce = [0; 12];
    random(&mut nonce)?;
    let mut out = header(ctx, nonce, plaintext.len())?;
    let mut encrypted = plaintext.to_vec();
    cipher(key)?
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(&out),
            &mut encrypted,
        )
        .map_err(|_| Error::Unavailable)?;
    out.extend_from_slice(&encrypted);
    Ok(out)
}
#[cfg(test)]
pub(super) fn failed_random(
    key: &[u8; 32],
    ctx: Context,
    plaintext: &[u8],
) -> Result<Vec<u8>, Error> {
    seal_with_random(key, ctx, plaintext, |_| Err(Error::Unavailable))
}
pub(super) fn open(key: &[u8; 32], expected: Context, bytes: &[u8]) -> Result<Secret, Error> {
    let (ctx, nonce) = frame(bytes)?;
    if ctx != expected {
        return Err(Error::Context);
    }
    let mut out = Secret(bytes[HEADER..].to_vec());
    let len = cipher(key)?
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(&bytes[..HEADER]),
            &mut out.0,
        )
        .map_err(|_| Error::Corrupt)?
        .len();
    out.0.truncate(len);
    parts(ctx.kind, &out.0)?;
    Ok(out)
}
pub(super) struct Secret(pub(super) Vec<u8>);
impl Drop for Secret {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

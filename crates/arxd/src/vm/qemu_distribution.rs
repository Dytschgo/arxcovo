//! Byte pins for reviewed QEMU distribution candidates. Validation of these
//! bytes does not confer authority or prove what a child loaded or opened.

use super::{
    paths::ArtifactPin,
    public_egress::{STOCK_QEMU_1113, STOCK_RECEIPT_1113_SHA256, hex32},
};
use std::{collections::BTreeMap, io::Read};

const MAX_PAYLOAD_FILE: u64 = 512 * 1024 * 1024;

pub const STOCK_ID: &str = "qemu-11.1.3-windows-x64";
pub const WITNESSED_ID: &str = "qemu-11.1.3-arx-witness-1-windows-x64";
pub const WIRE_VERSION: u16 = 1;
pub const OPEN_TABLE_VERSION: u16 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DistributionPin {
    pub receipt: ArtifactPin,
    pub manifest: ArtifactPin,
    pub system: ArtifactPin,
    pub image: ArtifactPin,
    pub payload_files: usize,
}

pub const STOCK: DistributionPin = DistributionPin {
    receipt: ArtifactPin {
        size: 7_438,
        sha256: STOCK_RECEIPT_1113_SHA256,
    },
    manifest: ArtifactPin {
        size: 11_915,
        sha256: hex32("f1cf73c8ecae163e31467a3985fc13749aee1f12a8747756eb136108a1015759"),
    },
    system: STOCK_QEMU_1113,
    image: ArtifactPin {
        size: 2_520_350,
        sha256: hex32("0765417c097a7d9d7ddf5b0bf7cdb4ab033d11767d3839005111e367862a0f9f"),
    },
    payload_files: 136,
};

pub const WITNESSED: DistributionPin = DistributionPin {
    receipt: ArtifactPin {
        size: 2_654,
        sha256: hex32("c973a55e5b95c2566dad90c40c41df0bcb2125bb20b28988d78548c24d4290aa"),
    },
    manifest: ArtifactPin {
        size: 12_688,
        sha256: hex32("e92ea4db73882d24012b78738d92e1ba3322dcdd990b26bdc7724fbe22e7e2a5"),
    },
    system: ArtifactPin {
        size: 26_738_700,
        sha256: hex32("d8f79194626a610a463eb41ca4615b570d00fb56f46ba5904bb6eab818aa5b4d"),
    },
    image: ArtifactPin {
        size: 2_522_341,
        sha256: hex32("69311654a4c66ce4e2c8cc6cec71e7a84b63d15b738b2bfc4a19c85193224fe1"),
    },
    payload_files: 135,
};

pub const fn distribution_pin(id: &str) -> Option<DistributionPin> {
    match id.as_bytes() {
        // Keep identification exact. Production still selects its own profile.
        b"qemu-11.1.3-windows-x64" => Some(STOCK),
        b"qemu-11.1.3-arx-witness-1-windows-x64" => Some(WITNESSED),
        _ => None,
    }
}

/// Validate both metadata files before interpreting manifest rows. Names in
/// this version come exclusively from the pinned manifest. The caller is
/// responsible for a held relative open and exhaustive directory inventory.
pub fn witnessed_payload<'a>(
    receipt: &[u8],
    manifest: &'a [u8],
) -> Option<BTreeMap<&'a str, ArtifactPin>> {
    witness_metadata(WITNESSED, receipt, manifest)
}

fn witness_metadata<'a>(
    pin: DistributionPin,
    receipt: &[u8],
    manifest: &'a [u8],
) -> Option<BTreeMap<&'a str, ArtifactPin>> {
    if !pin.receipt.verify(receipt, pin.receipt.size).ok()?
        || !pin.manifest.verify(manifest, pin.manifest.size).ok()?
    {
        return None;
    }

    let mut files = BTreeMap::new();
    for line in std::str::from_utf8(manifest).ok()?.lines() {
        let (fields, path) = line.split_once("  ")?;
        let (size, digest) = fields.split_once(' ')?;
        let size = size.parse().ok()?;
        if !(1..=MAX_PAYLOAD_FILE).contains(&size) {
            return None;
        }
        let mut sha256 = [0; 32];
        hex::decode_to_slice(digest, &mut sha256).ok()?;
        if files.insert(path, ArtifactPin { size, sha256 }).is_some() {
            return None;
        }
    }
    if files.len() != pin.payload_files
        || files.get("qemu-system-x86_64.exe") != Some(&pin.system)
        || files.get("qemu-img.exe") != Some(&pin.image)
    {
        return None;
    }

    Some(files)
}

/// Verify each manifest member using a caller-supplied held reader.
pub fn verify_payload<R: Read>(
    files: &BTreeMap<&str, ArtifactPin>,
    mut held_file: impl FnMut(&str) -> Option<R>,
) -> bool {
    files.iter().all(|(name, pin)| {
        held_file(name).is_some_and(|file| matches!(pin.verify(file, MAX_PAYLOAD_FILE), Ok(true)))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    fn bytes_pin(bytes: &[u8]) -> ArtifactPin {
        ArtifactPin {
            size: bytes.len() as u64,
            sha256: Sha256::digest(bytes).into(),
        }
    }

    #[test]
    fn metadata_and_payload_require_exact_bytes() {
        let receipt = b"fixture receipt";
        let system = &b"fixture system"[..];
        let image = &b"fixture image"[..];
        let firmware = &b"fixture rom"[..];
        let entries = [
            ("qemu-system-x86_64.exe", system),
            ("qemu-img.exe", image),
            ("share/QEMU,tcx.bin", firmware),
        ];
        let manifest: String = entries
            .iter()
            .map(|(name, bytes)| {
                format!(
                    "{} {}  {name}\n",
                    bytes.len(),
                    hex::encode(Sha256::digest(bytes))
                )
            })
            .collect();
        let manifest = manifest.as_bytes();
        let pin = DistributionPin {
            receipt: bytes_pin(receipt),
            manifest: bytes_pin(manifest),
            system: bytes_pin(system),
            image: bytes_pin(image),
            payload_files: 3,
        };
        let files = witness_metadata(pin, receipt, manifest).unwrap();
        assert!(witness_metadata(STOCK, receipt, manifest).is_none());
        assert!(witness_metadata(pin, b"fixture receipT", manifest).is_none());
        assert!(witness_metadata(pin, &receipt[1..], manifest).is_none());
        assert!(witness_metadata(pin, receipt, &manifest[1..]).is_none());
        let mut wrong = manifest.to_vec();
        wrong[0] ^= 1;
        assert!(witness_metadata(pin, receipt, &wrong).is_none());

        assert!(verify_payload(&files, |name| {
            entries
                .iter()
                .find(|entry| entry.0 == name)
                .map(|entry| entry.1)
        }));
        assert!(!verify_payload(&files, |name| match name {
            "qemu-system-x86_64.exe" => Some(system),
            "qemu-img.exe" => Some(image),
            _ => None,
        }));
        for wrong in [&firmware[1..], &b"fixture roM"[..]] {
            assert!(!verify_payload(&files, |name| match name {
                "qemu-system-x86_64.exe" => Some(system),
                "qemu-img.exe" => Some(image),
                "share/QEMU,tcx.bin" => Some(wrong),
                _ => None,
            }));
        }
    }

    #[test]
    fn stock_witnessed_and_egress_have_separate_identifiers() {
        assert_eq!(distribution_pin(STOCK_ID), Some(STOCK));
        assert_eq!(distribution_pin(WITNESSED_ID), Some(WITNESSED));
        assert_ne!(STOCK.receipt, WITNESSED.receipt);
        assert_ne!(STOCK.system, WITNESSED.system);
        assert_eq!(
            distribution_pin(super::super::public_egress::EGRESS_MANIFEST),
            None
        );
        assert_eq!(distribution_pin("11.1.3"), None);
    }

    #[test]
    fn shipped_receipt_binds_review_artifacts_and_protocol() {
        const RECEIPT: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tools/qemu/witness-receipt.json"
        ));
        const MANIFEST: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tools/qemu/witness-payload.sha256"
        ));
        const PATCH: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tools/qemu/arxcovo-witness.patch"
        ));
        const TOOLCHAIN: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tools/qemu/witness-toolchain.sha256"
        ));
        const PACKAGING: &[u8] = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tools/qemu/package-witness.ps1"
        ));
        let files = witnessed_payload(RECEIPT, MANIFEST).unwrap();
        assert_eq!(files.len(), WITNESSED.payload_files);
        assert!(files.contains_key("share/QEMU,tcx.bin"));
        assert!(files.contains_key("share/bios-256k.bin"));
        assert!(files.contains_key("share/kvmvapic.bin"));
        assert!(witness_metadata(STOCK, RECEIPT, MANIFEST).is_none());

        let receipt: serde_json::Value = serde_json::from_slice(RECEIPT).unwrap();
        assert_eq!(receipt["distribution"], WITNESSED_ID);
        assert_eq!(
            receipt["upstream_version"],
            super::super::qemu_args::QEMU_VERSION
        );
        assert_eq!(receipt["wire_version"], WIRE_VERSION);
        assert_eq!(receipt["open_table_version"], OPEN_TABLE_VERSION);
        assert_eq!(receipt["payload_file_count"], WITNESSED.payload_files);
        for (record, pin) in [
            (&receipt["patch"], bytes_pin(PATCH)),
            (&receipt["packaging"], bytes_pin(PACKAGING)),
            (&receipt["toolchain"]["input_lock"], bytes_pin(TOOLCHAIN)),
            (&receipt["payload_manifest"], WITNESSED.manifest),
            (
                &receipt["executables"]["qemu-system-x86_64.exe"],
                WITNESSED.system,
            ),
            (&receipt["executables"]["qemu-img.exe"], WITNESSED.image),
        ] {
            assert_eq!(record["size"], pin.size);
            assert_eq!(record["sha256"], hex::encode(pin.sha256));
        }
    }
}

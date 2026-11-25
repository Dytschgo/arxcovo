//! The closed schema of every diagnostic file, enforced at the held writer.
//!
//! A diagnostic is accepted only when its bytes are exactly one canonical
//! JSON value of the file's fixed shape: known keys in declaration order,
//! bounded integers, booleans, fixed-length byte arrays and strings drawn
//! from closed enumerations. No shape carries a free-form string, so a raw
//! path, SID, account name or endpoint has no field to travel in. The
//! runner's writer mirrors these shapes, and a runner test checks every
//! encoding it can produce against this module.
//!
//! The writer supplies every free byte array itself (binding and chain).
//! The only arrays a caller's result may carry are the distribution
//! digests, and those must equal the reviewed stock pins.

use super::DiagnosticFile;
use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use std::fmt;

use crate::vm::qualification_record::MAX_TRIAL_MS;
/// The runner's record contract and collector versions.
pub use crate::vm::qualification_record::{
    COLLECTOR as RECORD_COLLECTOR, CONTRACT as RECORD_CONTRACT,
};
const U16: u64 = u16::MAX as u64;
const U32: u64 = u32::MAX as u64;
const U64: u64 = u64::MAX;

#[derive(Clone, Copy)]
enum Shape {
    /// An unsigned integer no larger than the bound.
    Uint(u64),
    /// Exactly this unsigned integer.
    Const(u64),
    Bool,
    /// A fixed-length array of bytes.
    Bytes(usize),
    /// Exactly these 32 bytes.
    Pinned(fn() -> [u8; 32]),
    Enum(&'static [&'static str]),
    /// A stage permitted in this file.
    FileStage,
    Object(&'static [(&'static str, Shape)]),
    /// `null` or the inner shape.
    Optional(&'static Shape),
    /// At most this many items of the inner shape.
    List(&'static Shape, usize),
    /// `"unimplemented"`, `{"failed":D}`, `{"inconclusive":D}` or
    /// `{"passed":O}` with an observation kind permitted in this file.
    StageResult,
    /// An observation tagged by `kind`, then that kind's fields.
    Observation,
}

pub(super) const STAGES: &[&str] = &[
    "inputs",
    "host_baseline",
    "distribution",
    "custody_job_capacity_baseline",
    "activity_reserves_baseline",
    "whpx_capability",
    "disposable_store",
    "suspended_child",
    "qemu_prelaunch_pause",
    "held_bundle_inventory",
    "first_guest_execution",
    "supervisor_loss_release",
    "capacity_trials",
    "reserve_activity_trials",
    "qmp_channel",
    "restricted_test_network",
    "ubuntu_recipe",
    "windows_recipe",
    "restricted_origin_network",
    "static_web_recipe",
    "public_ipv4_network",
    "docker_recipe",
    "installed_guest_endpoint",
    "cleanup",
];

const DIAGNOSTICS: &[&str] = &[
    "owner_active",
    "reserves_short",
    "whpx_absent",
    "observation_invalid",
    "expired",
    "clock_inconsistent",
    "cancelled",
    "stale_live_sample",
    "prerequisite_missing",
    "effect_unconfined",
    "teardown_uncertain",
    "unsupported_platform",
    "distribution_unverified",
    "path_custody_unverified",
    "job_unverified",
    "whpx_unverified",
    "disk_ceiling_unverified",
    "owner_activity_unverified",
    "reserves_unverified",
    "other_refusal",
];

const DEPENDENCIES: &[&str] = &[
    "artifact",
    "host",
    "store",
    "owner_profile",
    "distribution",
    "install_media",
    "recipes",
    "firmware",
    "guest_protocol",
    "network_topology",
];

const RECORD_FILES: &[&str] = &[
    "inputs.json",
    "host-baseline.json",
    "distribution.json",
    "custody-job-capacity.json",
    "activity-reserves.json",
    "whpx-qmp-endpoint.json",
    "network-restricted-test.json",
    "recipe-ubuntu.json",
    "recipe-windows.json",
    "network-restricted-origin.json",
    "recipe-static-web.json",
    "network-public-ipv4.json",
    "recipe-docker.json",
    "cleanup.json",
];

const BINDING: &[(&str, Shape)] = &[
    ("class", Shape::Enum(&["test", "static_origin", "docker"])),
    ("scope", Shape::Bytes(32)),
    ("release", Shape::Bytes(32)),
    ("run", Shape::Bytes(16)),
    ("dependencies", Shape::Bytes(32)),
    ("issued_utc", Shape::Uint(U64)),
    ("expires_utc", Shape::Uint(U64)),
];

/// Only native records reach the native writer.
const NATIVE: Shape = Shape::Enum(&["native"]);

const RECORD: &[(&str, Shape)] = &[
    ("contract", Shape::Const(RECORD_CONTRACT as u64)),
    ("collector", Shape::Const(RECORD_COLLECTOR as u64)),
    ("origin", NATIVE),
    ("binding", Shape::Object(BINDING)),
    ("stage", Shape::FileStage),
    ("started_ms", Shape::Uint(MAX_TRIAL_MS)),
    ("finished_ms", Shape::Uint(MAX_TRIAL_MS)),
    ("prior", Shape::Bytes(32)),
    ("result", Shape::StageResult),
];

const ANY_STAGE: Shape = Shape::Enum(STAGES);

const INCOMPLETE: &[(&str, Shape)] = &[
    ("contract", Shape::Const(RECORD_CONTRACT as u64)),
    ("origin", NATIVE),
    ("binding", Shape::Object(BINDING)),
    ("blocked_at", Shape::Optional(&ANY_STAGE)),
    ("unattempted", Shape::List(&ANY_STAGE, 24)),
    ("unmeasured", Shape::List(&Shape::Enum(DEPENDENCIES), 10)),
    ("dispatched_effects", Shape::Uint(U16)),
    ("charge_retained", Shape::Bool),
    ("late", Shape::Optional(&Shape::Enum(DIAGNOSTICS))),
    ("unwritten", Shape::List(&Shape::Enum(RECORD_FILES), 14)),
];

/// Every observation kind and its fields, in declaration order.
const OBSERVATIONS: &[(&str, &[(&str, Shape)])] = &[
    (
        "input_pins",
        &[
            ("held_roots", Shape::Uint(u8::MAX as u64)),
            ("enrollment_private", Shape::Bool),
            ("evidence_private", Shape::Bool),
            ("trial_root_private", Shape::Bool),
            ("evidence_claimed", Shape::Bool),
        ],
    ),
    (
        "host",
        &[
            ("windows_build", Shape::Uint(U32)),
            ("windows_revision", Shape::Uint(U32)),
        ],
    ),
    (
        "distribution",
        &[
            ("receipt", Shape::Pinned(stock_receipt)),
            ("receipt_bytes", Shape::Uint(U64)),
            ("manifest", Shape::Pinned(stock_manifest)),
            ("manifest_bytes", Shape::Uint(U64)),
            ("payload_files", Shape::Uint(U16)),
            ("payload_bytes", Shape::Uint(U64)),
        ],
    ),
    (
        "capacity",
        &[
            ("inner_caller_free_bytes", Shape::Uint(U64)),
            ("inner_total_free_bytes", Shape::Uint(U64)),
            ("inner_total_bytes", Shape::Uint(U64)),
            ("inner_extent_bytes", Shape::Uint(U64)),
            ("vhd_virtual_bytes", Shape::Uint(U64)),
            ("outer_caller_free_bytes", Shape::Uint(U64)),
            ("outer_total_free_bytes", Shape::Uint(U64)),
            ("backing_allocated_bytes", Shape::Uint(U64)),
            ("backing_length_bytes", Shape::Uint(U64)),
            ("disk_ceiling_bytes", Shape::Uint(U64)),
            ("outer_floor_bytes", Shape::Uint(U64)),
            ("disk_budget_bytes", Shape::Uint(U64)),
            ("empty_job_memory_limit_bytes", Shape::Uint(U64)),
            ("empty_job_cpu_rate", Shape::Uint(U32)),
        ],
    ),
    (
        "activity",
        &[
            ("console_idle", Shape::Bool),
            ("active_cpus", Shape::Uint(U32)),
            ("available_ram_bytes", Shape::Uint(U64)),
            ("owner_ram_reserve_bytes", Shape::Uint(U64)),
            ("owner_cpu_reserve", Shape::Uint(U32)),
        ],
    ),
    ("whpx", &[("present", Shape::Bool)]),
    ("paused_store", &[("validated", Shape::Bool)]),
    (
        "firmware_sec",
        &[
            ("sec_marker_verified", Shape::Bool),
            ("prelaunch_quiet", Shape::Bool),
            ("job_memory_limit_bytes", Shape::Uint(U64)),
            ("job_cpu_rate", Shape::Uint(U32)),
            ("clean_release", Shape::Bool),
        ],
    ),
    (
        "qmp_firmware",
        &[
            ("greeting_verified", Shape::Bool),
            ("whpx_enabled", Shape::Bool),
            ("quit_clean_eof", Shape::Bool),
            ("clean_release", Shape::Bool),
        ],
    ),
    (
        "restricted_network_live",
        &[
            ("job_memory_limit_bytes", Shape::Uint(U64)),
            ("job_cpu_rate", Shape::Uint(U32)),
            ("host_controls_pre", Shape::Bool),
            ("host_controls_post", Shape::Bool),
            ("nic_setup_verified", Shape::Bool),
            ("denied_probes", Shape::Uint(u8::MAX as u64)),
            ("zero_hostfwd", Shape::Bool),
            ("clean_guest_shutdown", Shape::Bool),
            ("seed_released", Shape::Bool),
        ],
    ),
    (
        "supervisor_loss",
        &[
            ("job_memory_limit_bytes", Shape::Uint(U64)),
            ("job_cpu_rate", Shape::Uint(U32)),
            ("active_process_limit", Shape::Uint(U32)),
            ("child_exited", Shape::Bool),
            ("job_empty_notified", Shape::Bool),
            ("dirty_release", Shape::Bool),
        ],
    ),
    (
        "capacity_exercised",
        &[
            ("trial_allocated_bytes", Shape::Uint(U64)),
            ("recipe_allocated_bytes", Shape::Uint(U64)),
            ("denial_checks", Shape::Uint(u8::MAX as u64)),
            ("overlap_excluded", Shape::Bool),
            ("aggregate_cap_checked", Shape::Bool),
            ("trial_released", Shape::Bool),
            ("recipe_released", Shape::Bool),
        ],
    ),
    (
        "reserve_activity",
        &[
            (
                "controlled_faults",
                Shape::List(
                    &Shape::Object(&[
                        (
                            "fault",
                            Shape::Enum(&[
                                "wts_ambiguity",
                                "cpu_reserve",
                                "ram_reserve",
                                "disk_reserve",
                            ]),
                        ),
                        ("sampled_ms", Shape::Uint(MAX_TRIAL_MS)),
                        ("settled_ms", Shape::Uint(MAX_TRIAL_MS)),
                        ("console_idle", Shape::Bool),
                        ("effective_cpus", Shape::Uint(U32)),
                        ("available_ram_bytes", Shape::Uint(U64)),
                        ("inner_free_bytes", Shape::Uint(U64)),
                        ("outer_free_bytes", Shape::Uint(U64)),
                    ]),
                    4,
                ),
            ),
            (
                "console_transitions_ms",
                Shape::List(&Shape::Uint(MAX_TRIAL_MS), 5),
            ),
        ],
    ),
    ("paused_trial", &[("verified", Shape::Bool)]),
    (
        "cleanup",
        &[
            ("dispatched_effects", Shape::Uint(U16)),
            ("released", Shape::Bool),
            ("charge_retained", Shape::Bool),
            ("late", Shape::Bool),
        ],
    ),
];

/// The reviewed stock distribution receipt digest (ADR 0054).
fn stock_receipt() -> [u8; 32] {
    crate::vm::public_egress::STOCK_RECEIPT_1113_SHA256
}

/// The reviewed stock distribution manifest digest (ADR 0054).
pub(super) fn stock_manifest() -> [u8; 32] {
    crate::vm::qualification_record::stock_manifest()
}

/// The shared release names the evidence contract and reviewed distribution.
pub(super) fn release() -> [u8; 32] {
    crate::vm::qualification_record::release()
}

/// The record files, in manifest order.
pub(super) const RECORD_KINDS: [DiagnosticFile; 14] = [
    DiagnosticFile::Inputs,
    DiagnosticFile::HostBaseline,
    DiagnosticFile::Distribution,
    DiagnosticFile::CustodyJobCapacity,
    DiagnosticFile::ActivityReserves,
    DiagnosticFile::WhpxQmpEndpoint,
    DiagnosticFile::NetworkRestrictedTest,
    DiagnosticFile::RecipeUbuntu,
    DiagnosticFile::RecipeWindows,
    DiagnosticFile::NetworkRestrictedOrigin,
    DiagnosticFile::RecipeStaticWeb,
    DiagnosticFile::NetworkPublicIpv4,
    DiagnosticFile::RecipeDocker,
    DiagnosticFile::Cleanup,
];

/// The index in `RECORD_KINDS` of the file that holds a stage, from the
/// stage's JSON spelling.
pub(super) fn record_file(stage: &str) -> Option<usize> {
    let name: String = serde_json::from_str(stage).ok()?;
    RECORD_KINDS
        .iter()
        .position(|file| rules(*file).stages.contains(&name.as_str()))
}

/// What one file may contain.
struct Rules {
    top: Shape,
    stages: &'static [&'static str],
    kinds: &'static [&'static str],
}

const RECORDS: Shape = Shape::List(&Shape::Object(RECORD), 24);

fn records(stages: &'static [&'static str], kinds: &'static [&'static str]) -> Rules {
    Rules {
        top: RECORDS,
        stages,
        kinds,
    }
}

fn rules(file: DiagnosticFile) -> Rules {
    use DiagnosticFile as F;
    match file {
        F::Inputs => records(&["inputs"], &["input_pins"]),
        F::HostBaseline => records(&["host_baseline"], &["host"]),
        F::Distribution => records(&["distribution"], &["distribution"]),
        F::CustodyJobCapacity => records(
            &[
                "custody_job_capacity_baseline",
                "disposable_store",
                "suspended_child",
                "held_bundle_inventory",
                "supervisor_loss_release",
                "capacity_trials",
            ],
            &[
                "capacity",
                "paused_store",
                "paused_trial",
                "supervisor_loss",
                "capacity_exercised",
            ],
        ),
        F::ActivityReserves => records(
            &["activity_reserves_baseline", "reserve_activity_trials"],
            &["activity", "reserve_activity"],
        ),
        F::WhpxQmpEndpoint => records(
            &[
                "whpx_capability",
                "qemu_prelaunch_pause",
                "first_guest_execution",
                "qmp_channel",
                "installed_guest_endpoint",
            ],
            &["whpx", "paused_trial", "firmware_sec", "qmp_firmware"],
        ),
        F::NetworkRestrictedTest => {
            records(&["restricted_test_network"], &["restricted_network_live"])
        }
        // Recipe and non-TEST network collectors remain unavailable in PR 1.
        F::RecipeUbuntu => records(&["ubuntu_recipe"], &[]),
        F::RecipeWindows => records(&["windows_recipe"], &[]),
        F::NetworkRestrictedOrigin => records(&["restricted_origin_network"], &[]),
        F::RecipeStaticWeb => records(&["static_web_recipe"], &[]),
        F::NetworkPublicIpv4 => records(&["public_ipv4_network"], &[]),
        F::RecipeDocker => records(&["docker_recipe"], &[]),
        F::Cleanup => records(&["cleanup"], &["cleanup"]),
        F::Incomplete => Rules {
            top: Shape::Object(INCOMPLETE),
            stages: &[],
            kinds: &[],
        },
    }
}

/// Accept `bytes` only as one canonical value of `file`'s closed shape.
pub(super) fn check(file: DiagnosticFile, bytes: &[u8]) -> bool {
    // Canonical output of these shapes has no whitespace and no escapes.
    if bytes
        .iter()
        .any(|byte| byte.is_ascii_whitespace() || *byte == b'\\')
    {
        return false;
    }
    let rules = rules(file);
    let mut input = serde_json::Deserializer::from_slice(bytes);
    Check {
        shape: &rules.top,
        rules: &rules,
    }
    .deserialize(&mut input)
    .and_then(|()| input.end())
    .is_ok()
}

struct Check<'a> {
    shape: &'a Shape,
    rules: &'a Rules,
}

impl<'a> Check<'a> {
    fn with(&self, shape: &'a Shape) -> Self {
        Self {
            shape,
            rules: self.rules,
        }
    }

    /// A present optional value has its inner shape.
    fn present(&self) -> &'a Shape {
        match self.shape {
            Shape::Optional(inner) => inner,
            shape => shape,
        }
    }
}

fn reject<T, E: de::Error>() -> Result<T, E> {
    Err(E::custom("outside the closed diagnostic schema"))
}

impl<'de> DeserializeSeed<'de> for Check<'_> {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, input: D) -> Result<(), D::Error> {
        input.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Check<'_> {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a closed diagnostic value")
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<(), E> {
        match self.present() {
            Shape::Uint(max) if value <= *max => Ok(()),
            Shape::Const(expected) if value == *expected => Ok(()),
            _ => reject(),
        }
    }

    fn visit_bool<E: de::Error>(self, _value: bool) -> Result<(), E> {
        match self.present() {
            Shape::Bool => Ok(()),
            _ => reject(),
        }
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<(), E> {
        let allowed = match self.present() {
            Shape::Enum(names) => names.contains(&value),
            Shape::FileStage => self.rules.stages.contains(&value),
            Shape::StageResult => value == "unimplemented",
            _ => false,
        };
        if allowed { Ok(()) } else { reject() }
    }

    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        match self.shape {
            Shape::Optional(_) => Ok(()),
            _ => reject(),
        }
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut items: A) -> Result<(), A::Error> {
        if let Shape::Pinned(expected) = self.present() {
            let mut value = [0u8; 32];
            let mut count = 0;
            while let Some(byte) = items.next_element::<u8>()? {
                if count == value.len() {
                    return reject();
                }
                value[count] = byte;
                count += 1;
            }
            return if count == value.len() && value == expected() {
                Ok(())
            } else {
                reject()
            };
        }
        let (item, min, max) = match self.present() {
            Shape::Bytes(length) => (&Shape::Uint(u8::MAX as u64), *length, *length),
            Shape::List(item, max) => (*item, 0, *max),
            _ => return reject(),
        };
        let mut count = 0;
        while count <= max && items.next_element_seed(self.with(item))?.is_some() {
            count += 1;
        }
        if (min..=max).contains(&count) {
            Ok(())
        } else {
            reject()
        }
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let fields: &[(&str, Shape)] = match self.present() {
            Shape::Object(fields) => fields,
            Shape::StageResult => {
                let key: String = map.next_key()?.map_or_else(reject, Ok)?;
                let value = match key.as_str() {
                    "passed" => &Shape::Observation,
                    "failed" | "inconclusive" => &Shape::Enum(DIAGNOSTICS),
                    _ => return reject(),
                };
                map.next_value_seed(self.with(value))?;
                return end(map);
            }
            Shape::Observation => {
                let key: String = map.next_key()?.map_or_else(reject, Ok)?;
                if key != "kind" {
                    return reject();
                }
                let kind: String = map.next_value()?;
                if !self.rules.kinds.contains(&kind.as_str()) {
                    return reject();
                }
                OBSERVATIONS
                    .iter()
                    .find(|(name, _)| *name == kind)
                    .map_or_else(reject, |(_, fields)| Ok(*fields))?
            }
            _ => return reject(),
        };
        for (name, shape) in fields {
            let key: String = map.next_key()?.map_or_else(reject, Ok)?;
            if key != *name {
                return reject();
            }
            map.next_value_seed(self.with(shape))?;
        }
        end(map)
    }
}

/// No key may follow the declared fields; its value is never read.
fn end<'de, A: MapAccess<'de>>(mut map: A) -> Result<(), A::Error> {
    match map.next_key::<IgnoredAny>()? {
        None => Ok(()),
        Some(_) => reject(),
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    fn hash(byte: u8) -> String {
        format!("[{}]", vec![byte.to_string(); 32].join(","))
    }

    pub(in super::super) fn binding() -> String {
        format!(
            r#"{{"class":"test","scope":{s},"release":{r},"run":[{run}],"dependencies":{d},"issued_utc":1800000000,"expires_utc":1800604800}}"#,
            s = hash(1),
            r = hash(2),
            run = vec!["3"; 16].join(","),
            d = hash(4),
        )
    }

    /// One canonical native `inputs.json` record list, as the runner writes it.
    pub(in super::super) fn inputs_record(extra: &str) -> String {
        format!(
            r#"[{{"contract":3,"collector":1,"origin":"native","binding":{b},"stage":"inputs","started_ms":0,"finished_ms":3,"prior":{p},"result":{{"passed":{{"kind":"input_pins","held_roots":5,"enrollment_private":true,"evidence_private":true,"trial_root_private":true,"evidence_claimed":true{extra}}}}}}}]"#,
            b = binding(),
            p = hash(0),
        )
    }

    fn summary(late: &str, unwritten: &str) -> String {
        format!(
            r#"{{"contract":3,"origin":"native","binding":{b},"blocked_at":"disposable_store","unattempted":["suspended_child","cleanup"],"unmeasured":["artifact","owner_profile"],"dispatched_effects":0,"charge_retained":false,"late":{late},"unwritten":{unwritten}}}"#,
            b = binding(),
        )
    }

    #[test]
    fn canonical_records_and_summaries_pass_their_own_file() {
        assert!(check(DiagnosticFile::Inputs, inputs_record("").as_bytes()));
        assert!(check(
            DiagnosticFile::Incomplete,
            summary("null", "[]").as_bytes()
        ));
        assert!(check(
            DiagnosticFile::Incomplete,
            summary(r#""expired""#, r#"["cleanup.json"]"#).as_bytes()
        ));
        let unimplemented = inputs_record("")
            .replace(r#""stage":"inputs""#, r#""stage":"disposable_store""#)
            .replace(
                r#"{"passed":{"kind":"input_pins","held_roots":5,"enrollment_private":true,"evidence_private":true,"trial_root_private":true,"evidence_claimed":true}}"#,
                r#""unimplemented""#,
            );
        assert!(check(
            DiagnosticFile::CustodyJobCapacity,
            unimplemented.as_bytes()
        ));
    }

    fn distribution_record(receipt: [u8; 32], manifest: [u8; 32]) -> String {
        let array = |bytes: [u8; 32]| serde_json::to_string(&bytes).unwrap();
        format!(
            r#"[{{"contract":3,"collector":1,"origin":"native","binding":{b},"stage":"distribution","started_ms":0,"finished_ms":3,"prior":{p},"result":{{"passed":{{"kind":"distribution","receipt":{r},"receipt_bytes":7438,"manifest":{m},"manifest_bytes":11915,"payload_files":136,"payload_bytes":1}}}}}}]"#,
            b = binding(),
            p = hash(0),
            r = array(receipt),
            m = array(manifest),
        )
    }

    #[test]
    fn distribution_digests_are_pinned_to_the_reviewed_stock_values() {
        let (receipt, manifest) = (stock_receipt(), stock_manifest());
        assert!(check(
            DiagnosticFile::Distribution,
            distribution_record(receipt, manifest).as_bytes()
        ));
        for (name, receipt, manifest) in [
            ("chosen receipt", [7; 32], manifest),
            ("chosen manifest", receipt, [7; 32]),
        ] {
            assert!(
                !check(
                    DiagnosticFile::Distribution,
                    distribution_record(receipt, manifest).as_bytes()
                ),
                "{name}"
            );
        }
        let short = distribution_record(receipt, manifest).replacen(
            &serde_json::to_string(&receipt).unwrap(),
            "[1,2]",
            1,
        );
        assert!(!check(DiagnosticFile::Distribution, short.as_bytes()));
    }

    #[test]
    fn stages_map_to_exactly_one_record_file() {
        assert_eq!(record_file(r#""inputs""#), Some(0));
        assert_eq!(record_file(r#""disposable_store""#), Some(3));
        assert_eq!(record_file(r#""installed_guest_endpoint""#), Some(5));
        assert_eq!(record_file(r#""cleanup""#), Some(13));
        for stage in STAGES {
            let spelled = serde_json::to_string(stage).unwrap();
            let holders = RECORD_KINDS
                .iter()
                .filter(|file| rules(**file).stages.contains(stage))
                .count();
            assert_eq!(holders, 1, "{stage}");
            assert!(record_file(&spelled).is_some(), "{stage}");
        }
        for bad in [r#""unknown""#, "inputs", "3", r#"["inputs"]"#] {
            assert_eq!(record_file(bad), None, "{bad}");
        }
    }

    #[test]
    fn misuse_outside_the_closed_shape_is_refused() {
        let good = inputs_record("");
        for (name, file, bytes) in [
            (
                "synthetic endpoint in an unknown field",
                DiagnosticFile::Inputs,
                inputs_record(r#","endpoint":"192.0.2.7:443""#),
            ),
            (
                "unknown numeric field",
                DiagnosticFile::Inputs,
                inputs_record(r#","port":443"#),
            ),
            (
                "record in another file",
                DiagnosticFile::HostBaseline,
                good.clone(),
            ),
            (
                "observation kind foreign to the file",
                DiagnosticFile::Inputs,
                good.replace(r#""kind":"input_pins""#, r#""kind":"whpx""#),
            ),
            (
                "fixture origin",
                DiagnosticFile::Inputs,
                good.replace(r#""origin":"native""#, r#""origin":"fixture""#),
            ),
            (
                "fixture witness kind",
                DiagnosticFile::Inputs,
                good.replace(r#""kind":"input_pins""#, r#""kind":"fixture_effect""#),
            ),
            (
                "string in a numeric field",
                DiagnosticFile::Inputs,
                good.replace(r#""finished_ms":3"#, r#""finished_ms":"D:/x""#),
            ),
            (
                "time past the trial budget",
                DiagnosticFile::Inputs,
                good.replace(r#""finished_ms":3"#, r#""finished_ms":25200001"#),
            ),
            (
                "negative number",
                DiagnosticFile::Inputs,
                good.replace(r#""started_ms":0"#, r#""started_ms":-1"#),
            ),
            (
                "float",
                DiagnosticFile::Inputs,
                good.replace(r#""started_ms":0"#, r#""started_ms":0.5"#),
            ),
            (
                "wrong contract",
                DiagnosticFile::Inputs,
                good.replace(r#""contract":3"#, r#""contract":2"#),
            ),
            (
                "short hash",
                DiagnosticFile::Inputs,
                good.replacen(&format!(r#""prior":{}"#, hash(0)), r#""prior":[0]"#, 1),
            ),
            (
                "reordered fields",
                DiagnosticFile::Inputs,
                good.replace(
                    r#""contract":3,"collector":1"#,
                    r#""collector":1,"contract":3"#,
                ),
            ),
            (
                "duplicate field",
                DiagnosticFile::Inputs,
                good.replace(r#""contract":3,"#, r#""contract":3,"contract":3,"#),
            ),
            (
                "whitespace",
                DiagnosticFile::Inputs,
                good.replace(r#""contract":3"#, r#""contract": 2"#),
            ),
            (
                "escaped spelling",
                DiagnosticFile::Inputs,
                good.replace(r#""stage":"inputs""#, r#""stage":"input\u0073""#),
            ),
            (
                "trailing value",
                DiagnosticFile::Inputs,
                format!("{good}{good}"),
            ),
            (
                "free-form diagnostic",
                DiagnosticFile::Incomplete,
                summary(r#""C:/Users/owner""#, "[]"),
            ),
            (
                "unknown output file",
                DiagnosticFile::Incomplete,
                summary("null", r#"["qualification.json"]"#),
            ),
            (
                "summary as a record file",
                DiagnosticFile::Cleanup,
                summary("null", "[]"),
            ),
        ] {
            assert!(!check(file, bytes.as_bytes()), "{name}");
        }
    }
}

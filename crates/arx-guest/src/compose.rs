//! Guest half of Compose hosting on the static Ubuntu VM (ADR 0064, leg E2a).
//!
//! A guest that serves Compose accepts a pinned ComposeHello beside the legacy
//! Hello. Legacy frames keep their exact behavior. The host stages a strict
//! JSON project manifest in 16 KiB chunks, then deploys an exact revision. The
//! broker renders canonical Compose YAML into fixed root-owned directories and
//! drives Docker only through fixed invocations behind [`CommandRunner`].
//! Secrets arrive in a private request and are written to a dedicated tmpfs.
//!
//! The real Docker runner and Linux store live in the Linux platform module
//! and are **unverified on real hardware**. Tests use fakes.

mod codec;
mod engine;
mod layout;
mod manifest;
mod render;
mod service;
#[cfg(test)]
mod tests;
mod wire;

pub(crate) use codec::wipe;
pub use codec::{SecretText, base64_len, encode_base64, sha256_hex, wipe_bytes};
pub use engine::{
    APPLY_TIMEOUT, Clock, CommandRunner, DOCKER, ENGINE_CWD, ENGINE_ENV, HEALTH_ATTEMPTS,
    HEALTH_INTERVAL, Invocation, MAX_CAPTURE, PULL_TIMEOUT, QUERY_TIMEOUT, RunResult, STOP_TIMEOUT,
    compose_down, compose_ps, compose_recreate, compose_recreate_file, compose_stop, compose_up,
    container_inspect_mounts, container_rm, container_stop, image_inspect, image_pull,
    network_list, network_rm, parse_ps, volume_create, volume_label, volume_list,
    volume_mountpoint,
};
pub use layout::{
    DOCKER_CONFIG, DOCKER_VOLUME_ROOT, LAYOUT_VERSION, LOOPBACK_BASE, SECRET_ROOT, STATE_ROOT,
    compose_file, compose_project, loopback_port, network_name, revision_dir,
    revision_dir_relative, rotation_compose_file, secret_file, secret_instance, state_file,
    volume_name, volume_token,
};
pub use manifest::{
    EnvVar, Limits, MANIFEST_SCHEMA, MAX_ENV, MAX_MOUNTS, MAX_PROJECTS, MAX_SECRET_SLOTS,
    MAX_SERVICES, MAX_VOLUMES, ManifestRule, Mount, PLATFORM, Project, ProjectManifest,
    ServiceSpec, VolumeSpec, is_image_ref, is_project_id, is_slot_name,
};
pub use render::{PROJECT_LABEL, REVISION_LABEL, VOLUME_LABEL, render};
pub use service::{
    ComposeService, ComposeStore, MAX_JOURNAL, MAX_LOG, MAX_SECRET_BINDINGS, MAX_STAGED,
    MAX_STATE_BYTES, MAX_TOMBSTONES, SecretFileWitness, SecretMatch, StoreError,
};
pub(crate) use wire::is_compose_tag;
pub use wire::{
    COMPOSE_CAPABILITIES, COMPOSE_PROTOCOL_VERSION, ComposeRequest, ComposeResponse, Desired,
    FailureCode, HealthState, LogEntry, LogEvent, MAX_LOG_ENTRIES, MAX_LOG_PAGE, MAX_SECRET,
    Observation, OperationKind, OperationStage, OperationState, OperationView, ProjectView,
    RejectCode, STAGE_CHUNK, STAGE_TOTAL, ServiceState, ServiceView,
};

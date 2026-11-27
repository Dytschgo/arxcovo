//! VM admission and confined runtime components. Native issuance is closed.
pub(crate) mod compose;
pub(crate) mod compose_backup;
#[doc(hidden)]
pub use compose_backup::backup_retire_cli;
pub(crate) mod compose_secrets;
pub mod console;
mod controller;
pub mod guest;
mod model;
pub mod paths;
mod policy;
pub mod public_egress;
pub(crate) mod publish;
pub mod qemu_args;
pub mod qemu_distribution;
pub mod qemu_img;
pub mod qmp;
#[doc(hidden)]
pub mod qualification_record;
pub(crate) mod reaper;
pub mod runtime;
pub(crate) mod selection_epoch;
pub mod snapshot;
pub mod static_web;
mod template;
#[cfg(all(windows, target_arch = "x86_64"))]
pub(crate) mod test_issuer;
#[cfg(all(windows, target_arch = "x86_64"))]
pub(crate) mod windows;

#[cfg(all(windows, target_arch = "x86_64"))]
#[doc(hidden)]
pub use windows::qualification;

pub(crate) use crate::store::vm::{DashboardCatalog, DashboardVm};
pub use controller::VmController;
pub(crate) use controller::{
    ActivityReceipt, CatalogEntry, ForegroundSubject, Precheck, VaultSweepPriority,
    VaultTestCleanup,
};
#[cfg(all(windows, target_arch = "x86_64"))]
pub(crate) use controller::{AdmittedEffect, OwnedStop};
#[cfg(all(windows, target_arch = "x86_64"))]
pub(crate) use controller::{ClonePublication, SealedGolden};
pub use model::*;
pub use policy::{Capacity, HostObservation, Resources, VmPolicy};
pub use reaper::{MonotonicReaperTicks, ReaperTick};
pub(crate) use template::WEB_RECIPE_ID;
pub use template::{AssetPin, RecipeError, UbuntuAssets, UbuntuRecipe};
pub use template::{ubuntu_compose, ubuntu_web};

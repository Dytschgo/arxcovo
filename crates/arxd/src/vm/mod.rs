//! VM admission and confined runtime components. Native issuance is closed.
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
pub(crate) mod reaper;
pub mod runtime;
pub mod snapshot;
pub mod static_web;
mod template;
#[cfg(all(windows, target_arch = "x86_64"))]
mod windows;

#[cfg(all(windows, target_arch = "x86_64"))]
#[doc(hidden)]
pub use windows::qualification;

pub(crate) use crate::store::vm::{DashboardCatalog, DashboardVm};
pub use controller::VmController;
pub(crate) use controller::{ActivityReceipt, CatalogEntry, ForegroundSubject, Precheck};
pub use model::*;
pub use policy::{Capacity, HostObservation, Resources, VmPolicy};
pub use reaper::{MonotonicReaperTicks, ReaperTick};
pub use template::ubuntu_web;
pub use template::{AssetPin, RecipeError, UbuntuAssets, UbuntuRecipe};

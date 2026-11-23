//! VM admission and confined runtime components. Native issuance is closed.
pub mod console;
mod controller;
pub mod guest;
mod model;
pub mod paths;
mod policy;
mod publish;
pub mod qemu_args;
pub mod qemu_img;
pub mod qmp;
pub(crate) mod reaper;
pub mod runtime;
pub mod static_web;
mod template;
#[cfg(all(windows, target_arch = "x86_64"))]
mod windows;

pub(crate) use crate::store::vm::{DashboardCatalog, DashboardVm};
pub use controller::VmController;
pub use model::*;
pub use policy::{Capacity, HostObservation, Resources, VmPolicy};
pub use reaper::{MonotonicReaperTicks, ReaperTick};
pub use template::ubuntu_web;
pub use template::{AssetPin, RecipeError, UbuntuAssets, UbuntuRecipe};

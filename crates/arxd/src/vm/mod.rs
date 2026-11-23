//! VM admission and confined runtime components. Native issuance is closed.
mod controller;
pub mod guest;
mod model;
pub mod paths;
mod policy;
pub mod qemu_args;
pub mod qemu_img;
pub mod qmp;
pub(crate) mod reaper;
pub mod runtime;
mod template;
#[cfg(all(windows, target_arch = "x86_64"))]
mod windows;

pub use controller::VmController;
pub use model::*;
pub use policy::{Capacity, HostObservation, Resources, VmPolicy};
pub use reaper::{MonotonicReaperTicks, ReaperTick};
pub use template::{AssetPin, RecipeError, UbuntuAssets, UbuntuRecipe};

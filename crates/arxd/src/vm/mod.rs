//! VM admission and confined runtime components. Native issuance is closed.
mod controller;
pub mod guest;
mod model;
pub mod paths;
mod policy;
mod publish;
pub mod qemu_args;
pub mod qemu_img;
pub mod qmp;
pub mod runtime;
pub mod static_web;
mod template;
#[cfg(all(windows, target_arch = "x86_64"))]
mod windows;

pub use controller::VmController;
pub use model::*;
pub use policy::{Capacity, HostObservation, Resources, VmPolicy};
pub use template::ubuntu_web;
pub use template::{AssetPin, RecipeError, UbuntuAssets, UbuntuRecipe};

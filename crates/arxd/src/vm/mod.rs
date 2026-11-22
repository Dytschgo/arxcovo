//! Portable VM metadata and admission policy. Native runtime integration is a later slice.
mod controller;
mod model;
mod policy;
mod template;

pub use controller::VmController;
pub use model::*;
pub use policy::{Capacity, HostObservation, Resources, VmPolicy};
pub use template::{AssetPin, RecipeError, UbuntuAssets, UbuntuRecipe};

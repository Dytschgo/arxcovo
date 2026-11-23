#[cfg(target_os = "linux")]
mod linux;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub(super) use linux::serve_virtio;
#[cfg(windows)]
pub(super) use windows::serve_service;

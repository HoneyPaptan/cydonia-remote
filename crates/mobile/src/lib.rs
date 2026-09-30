pub mod apply;
pub mod seed;
pub mod write;

#[cfg(target_family = "wasm")]
mod client;
#[cfg(target_family = "wasm")]
mod net;
#[cfg(target_family = "wasm")]
mod laptop;
#[cfg(target_family = "wasm")]
mod fonts;
#[cfg(target_family = "wasm")]
mod hosts;
#[cfg(target_family = "wasm")]
mod keeper;
#[cfg(target_family = "wasm")]
mod pick;
#[cfg(target_family = "wasm")]
mod pictures;
#[cfg(target_family = "wasm")]
mod wallpaper;

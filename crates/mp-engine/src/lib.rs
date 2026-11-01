//! The audio engine.
//!
//! [`core::EngineCore`] is the one place audio is produced, for both playback and export. The
//! real-time wrapper in [`output`] drives it from the sound card; [`core::render_offline`] drives it
//! from a loop. The UI talks to it only through immutable [`graph::EngineGraph`] snapshots.

pub mod compile;
pub mod core;
pub mod device;
pub mod devices;
pub mod graph;
pub mod output;
pub mod render_cache;

pub use compile::{GraphCompiler, SourceBank};
pub use core::{render_offline, EngineCore};
pub use device::{Curve, Device, ParamInfo, ProcessContext, Unit};
pub use graph::{EngineGraph, Voice};
pub use output::{list_hosts, list_outputs, system_output_name, AudioConfig, AudioEngine, OutputInfo, Shared, MAX_METERS};
pub use render_cache::RenderCache;

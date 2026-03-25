// Re-export internal modules for benchmarks and integration tests.
// The binary entry point remains in main.rs.

pub mod engine;
pub mod ipc;
// Renderer is fully implemented but not wired into IPC yet (M3).
#[allow(dead_code)]
pub mod render;
pub mod shell;
// Spec types define the full withfig schema; many fields are not yet consumed.
#[allow(dead_code)]
pub mod spec;

// Daemon is only used by the binary, not exported.
// pub mod daemon;

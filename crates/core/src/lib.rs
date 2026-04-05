// =============================================================================
// SECURITY AUDIT CHECKLIST — core/src/lib.rs
// [✓] No unsafe code in this module
// [✓] RICH engine and GNN stub are pure functions — no side effects
// [✓] All path results validated before leaving this crate
// [✓] No panics
// =============================================================================

#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub mod gnn_stub;
pub mod path_finder;
pub mod price_matrix;
pub mod rich_engine;

pub use gnn_stub::GnnOracle;
pub use path_finder::PathFinder;
pub use price_matrix::MatrixBuilder;
pub use rich_engine::RichEngine;

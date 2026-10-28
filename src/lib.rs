//! Agent Globe library: everything that does not need a renderer.
//!
//! `data` is pure parsing, modelling and layout. `bridge` runs the worker threads (file tailing,
//! HTTP ingest) and exposes channels the app drains once per frame.

pub mod bridge;
pub mod data;

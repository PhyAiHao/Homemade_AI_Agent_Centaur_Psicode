//! Remote sessions — WebSocket-based remote agent control.
//!
//! Mirrors `src/remote/` (4 files). Provides session management,
//! WebSocket protocol, and permission bridging for remote execution.
#![allow(dead_code)]

pub mod manager;
pub mod websocket;
pub mod permission_bridge;


//! This crate provides a performant implementation of the Esperto input system, a powerful
//! and robust system for key combinations. The implementation is generic, so that it can
//! be easily plugged into new and existing systems, regardless of their needs.
//!
//! The main functionalities are provided by the [`combo::ComboHandler`] struct.
//!
//! The crate also provides a SDL3 based demo in the examples section, that prints recognized
//! key combinations on a window.
//!
//! ## Esperto input system
//!
//! /TODO markdown qui

/// Module with the implementation of the combo algorithm.
///
/// Exports the [`combo::ComboHandler`] struct, which is the main entrypoint
/// of the library.
pub mod combo;

/// Configuration types
pub mod config;

/// Utility types
pub mod types;

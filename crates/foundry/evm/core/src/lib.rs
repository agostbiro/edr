//! # foundry-evm-core
//!
//! Core EVM abstractions.

#![warn(unused_crate_dependencies)]

use auto_impl::auto_impl;
use revm::{inspector::NoOpInspector, interpreter::CreateInputs, Database, Inspector};
use revm_inspectors::access_list::AccessListInspector;

#[macro_use]
extern crate tracing;

mod ic;

pub mod abi;
pub mod backend;
pub mod constants;
pub mod contracts;
pub mod decode;
pub mod evm;
pub mod evm_env;
pub mod fork;
pub mod opcodes;
pub mod opts;
pub mod precompiles;
pub mod snapshot;
pub mod utils;

/// An extension trait that allows us to add additional hooks to Inspector for
/// later use in handlers.
#[auto_impl(&mut, Box)]
pub trait InspectorExt<DB: Database>: Inspector<DB> {
    // Simulates `console.log` invocation.
    fn console_log(&mut self, _input: String) {}
}

impl<DB: Database> InspectorExt<DB> for NoOpInspector {}
impl<DB: Database> InspectorExt<DB> for AccessListInspector {}

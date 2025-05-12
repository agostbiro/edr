//! # foundry-evm-core
//!
//! Core EVM abstractions.

#![warn(unused_crate_dependencies)]

use auto_impl::auto_impl;
use revm::{
    context::{Block, CfgEnv},
    context_interface::Transaction,
    inspector::NoOpInspector,
    primitives::hardfork::SpecId,
    Context, Database, Inspector, Journal,
};
use revm_inspectors::access_list::AccessListInspector;

#[macro_use]
extern crate tracing;

mod ic;

pub mod abi;
pub mod backend;
pub mod constants;
pub mod contracts;
pub mod decode;
pub mod evm_context;
pub mod fork;
pub mod opcodes;
pub mod opts;
pub mod precompiles;
pub mod snapshot;
pub mod utils;

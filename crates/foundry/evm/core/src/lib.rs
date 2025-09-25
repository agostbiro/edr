//! # foundry-evm-core
//!
//! Core EVM abstractions.

#![cfg_attr(not(test), warn(unused_crate_dependencies))]
// TODO https://github.com/NomicFoundation/edr/issues/1076
#![allow(clippy::indexing_slicing)]

// TODO
#![allow(clippy::all, clippy::pedantic, clippy::restriction)]

use auto_impl::auto_impl;
use revm::context::CfgEnv;
use revm::{Inspector, Journal};
use revm::context::result::HaltReasonTr;
use revm::inspector::{NoOpInspector};
use revm_inspectors::access_list::AccessListInspector;

#[macro_use]
extern crate tracing;

pub mod abi;

pub mod env;

pub use env::*;
use crate::backend::CheatcodeBackend;
use crate::evm_context::{BlockEnvTr, ChainContextTr, EvmBuilderTrait, HardforkTr, TransactionEnvTr, TransactionErrorTrait};

pub mod backend;
pub mod buffer;
pub mod constants;
pub mod contracts;
pub mod decode;
pub mod evm_context;
pub mod fork;
pub mod ic;
pub mod opts;
pub mod precompiles;
pub mod state_snapshot;
pub mod utils;

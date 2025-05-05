//! # foundry-evm-core
//!
//! Core EVM abstractions.

#![warn(unused_crate_dependencies)]

use auto_impl::auto_impl;
use revm::{
    context::{Block, CfgEnv},
    context_interface::Transaction,
    inspector::NoOpInspector,
    interpreter::{interpreter::EthInterpreter, CreateInputs},
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
pub trait InspectorExt<BlockT, TxT, SpecT, DatabaseT, ChainContextT>:
    Inspector<Context<BlockT, TxT, CfgEnv<SpecT>, DatabaseT, Journal<DatabaseT>, ChainContextT>>
where
    BlockT: Block,
    TxT: Transaction,
    SpecT: Into<SpecId> + Copy,
    DatabaseT: Database,
{
    // Simulates `console.log` invocation.
    fn console_log(&mut self, _input: String) {}
}

impl<BlockT, TxT, SpecT, DatabaseT, ChainContextT>
    InspectorExt<BlockT, TxT, SpecT, DatabaseT, ChainContextT> for NoOpInspector
where
    BlockT: Block,
    TxT: Transaction,
    SpecT: Into<SpecId> + Copy,
    DatabaseT: Database,
{
}

impl<BlockT, TxT, SpecT, DatabaseT, ChainContextT>
    InspectorExt<BlockT, TxT, SpecT, DatabaseT, ChainContextT> for AccessListInspector
where
    BlockT: Block,
    TxT: Transaction,
    SpecT: Into<SpecId> + Copy,
    DatabaseT: Database,
{
}

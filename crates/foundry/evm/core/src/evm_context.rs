use alloy_primitives::{Address, B256, U256};
use revm::{
    context::{BlockEnv, CfgEnv, Evm, JournalInner, TxEnv},
    context_interface::{Block, JournalTr, Transaction},
    handler::{instructions::EthInstructions, EthPrecompiles},
    interpreter::interpreter::EthInterpreter,
    primitives::hardfork::SpecId,
    Database, Inspector, Journal, JournalEntry,
};
use yansi::Paint;

use crate::{
    backend::CheatcodeBackend,
    opts::{BlockEnvOpts, TxEnvOpts},
};

pub trait HardforkTr:
    'static + Copy + std::fmt::Debug + Default + Into<SpecId> + Send + Sync + Unpin
{
}

impl<T> HardforkTr for T where
    T: 'static + Copy + std::fmt::Debug + Default + Into<SpecId> + Send + Sync + Unpin
{
}

// Into and from `BlockEnv` are temporarily needed for compatibility with
// foundry-fork-db
pub trait BlockEnvTr:
    'static
    + Clone
    + Default
    + From<BlockEnvOpts>
    + From<BlockEnv>
    + Into<BlockEnv>
    + Block
    + BlockEnvMut
    + Send
    + Sync
    + Unpin
{
}

impl<T> BlockEnvTr for T where
    T: 'static
        + Clone
        + Default
        + From<BlockEnvOpts>
        + From<BlockEnv>
        + Into<BlockEnv>
        + Block
        + BlockEnvMut
        + Send
        + Sync
        + Unpin
{
}

pub trait TransactionEnvTr:
    'static + Clone + Default + From<TxEnvOpts> + Transaction + TransactionEnvMut + Send + Sync + Unpin
{
}

impl<T> TransactionEnvTr for T where
    T: 'static
        + Clone
        + Default
        + From<TxEnvOpts>
        + Transaction
        + TransactionEnvMut
        + Send
        + Sync
        + Unpin
{
}

pub trait ChainContextTr: Clone {}

impl<T> ChainContextTr for T where T: Clone {}

pub trait TransactionEnvMut {
    fn set_blob_versioned_hashes(&mut self, blob_hashes: Vec<B256>);
    fn set_caller(&mut self, caller: Address);
    fn set_chain_id(&mut self, chain_id: Option<u64>);
    fn set_gas_price(&mut self, gas_price: u128);
}

impl TransactionEnvMut for TxEnv {
    fn set_blob_versioned_hashes(&mut self, blob_hashes: Vec<B256>) {
        self.blob_hashes = blob_hashes;
    }

    fn set_caller(&mut self, caller: Address) {
        self.caller = caller;
    }

    fn set_chain_id(&mut self, chain_id: Option<u64>) {
        self.chain_id = chain_id;
    }

    fn set_gas_price(&mut self, gas_price: u128) {
        self.gas_price = gas_price;
    }
}

pub trait BlockEnvMut {
    fn set_basefee(&mut self, basefee: u64);
    fn set_beneficiary(&mut self, beneficiary: Address);
    fn set_block_number(&mut self, block_number: u64);
    fn set_blob_excess_gas_and_price(&mut self, excess_blob_gas: u64, is_prague: bool);
    fn set_difficulty(&mut self, difficulty: U256);
    fn set_prevrandao(&mut self, prevrandao: B256);
    fn set_timestamp(&mut self, timestamp: u64);
}

impl BlockEnvMut for BlockEnv {
    fn set_basefee(&mut self, basefee: u64) {
        self.basefee = basefee;
    }

    fn set_blob_excess_gas_and_price(&mut self, excess_blob_gas: u64, is_prague: bool) {
        self.set_blob_excess_gas_and_price(excess_blob_gas, is_prague);
    }

    fn set_beneficiary(&mut self, coinbase: Address) {
        self.beneficiary = coinbase;
    }

    fn set_difficulty(&mut self, difficulty: U256) {
        self.difficulty = difficulty;
    }

    fn set_prevrandao(&mut self, prevrandao: B256) {
        self.prevrandao = Some(prevrandao);
    }

    fn set_block_number(&mut self, block_number: u64) {
        self.number = block_number;
    }

    fn set_timestamp(&mut self, timestamp: u64) {
        self.timestamp = timestamp;
    }
}

/// Split the database from EVM execution context so that a mutable method can
/// be called on the database with arguments from the execution context.
pub fn split_context<'a, BlockT, TxT, HardforkT, DatabaseT, ChainContextT>(
    context: &'a mut revm::context::Context<
        BlockT,
        TxT,
        CfgEnv<HardforkT>,
        DatabaseT,
        Journal<DatabaseT>,
        ChainContextT,
    >,
) -> (
    &'a mut DatabaseT,
    EvmContext<'a, BlockT, TxT, HardforkT, ChainContextT>,
)
where
    BlockT: BlockEnvTr,
    TxT: TransactionEnvTr,
    HardforkT: HardforkTr,
    ChainContextT: ChainContextTr,
    DatabaseT: CheatcodeBackend<BlockT, TxT, HardforkT, ChainContextT>,
{
    let evm_context = EvmContext {
        block: &mut context.block,
        tx: &mut context.tx,
        cfg: &mut context.cfg,
        journaled_state: &mut context.journaled_state.inner,
        chain_context: &mut context.chain,
    };

    (&mut context.journaled_state.database, evm_context)
}

pub struct EvmContext<'a, BlockT, TxT, HardforkT, ChainContextT> {
    pub block: &'a mut BlockT,
    pub tx: &'a mut TxT,
    pub cfg: &'a mut CfgEnv<HardforkT>,
    pub journaled_state: &'a mut JournalInner<JournalEntry>,
    pub chain_context: &'a mut ChainContextT,
}

impl<'a, BlockT, TxT, HardforkT, ChainContextT, DatabaseT>
    From<
        &'a mut revm::context::Context<
            BlockT,
            TxT,
            CfgEnv<HardforkT>,
            DatabaseT,
            Journal<DatabaseT>,
            ChainContextT,
        >,
    > for EvmContext<'a, BlockT, TxT, HardforkT, ChainContextT>
where
    BlockT: BlockEnvTr,
    TxT: TransactionEnvTr,
    HardforkT: HardforkTr,
    ChainContextT: ChainContextTr,
    DatabaseT: CheatcodeBackend<BlockT, TxT, HardforkT, ChainContextT>,
{
    fn from(
        value: &'a mut revm::context::Context<
            BlockT,
            TxT,
            CfgEnv<HardforkT>,
            DatabaseT,
            Journal<DatabaseT>,
            ChainContextT,
        >,
    ) -> Self {
        Self {
            block: &mut value.block,
            tx: &mut value.tx,
            cfg: &mut value.cfg,
            journaled_state: &mut value.journaled_state,
            chain_context: &mut value.chain,
        }
    }
}

impl<'a, BlockT, TxT, HardforkT, ChainContextT>
    EvmContext<'a, BlockT, TxT, HardforkT, ChainContextT>
where
    BlockT: BlockEnvTr,
    TxT: TransactionEnvTr,
    HardforkT: HardforkTr,
{
    pub fn to_owned_env(&self) -> EvmEnv<BlockT, TxT, HardforkT> {
        EvmEnv {
            block: self.block.clone(),
            tx: self.tx.clone(),
            cfg: self.cfg.clone(),
        }
    }
}

/// EVM execution environment
#[derive(Clone, Debug, Default)]
pub struct EvmEnv<BlockT, TxT, HardforkT> {
    pub block: BlockT,
    pub tx: TxT,
    pub cfg: CfgEnv<HardforkT>,
}

impl<BlockT, TxT, HardforkT, DatabaseT, JournalT, ChainT>
    From<revm::context::Context<BlockT, TxT, CfgEnv<HardforkT>, DatabaseT, JournalT, ChainT>>
    for EvmEnv<BlockT, TxT, HardforkT>
where
    DatabaseT: Database,
    JournalT: JournalTr<Database = DatabaseT>,
{
    fn from(
        value: revm::context::Context<BlockT, TxT, CfgEnv<HardforkT>, DatabaseT, JournalT, ChainT>,
    ) -> Self {
        Self {
            block: value.block,
            tx: value.tx,
            cfg: value.cfg,
        }
    }
}

impl<BlockT, TxT, HardforkT: HardforkTr> EvmEnv<BlockT, TxT, HardforkT> {
    pub fn new_with_spec_id(mut env: EvmEnv<BlockT, TxT, HardforkT>, spec_id: HardforkT) -> Self {
        env.cfg.spec = spec_id;
        env
    }
}

// `Env` implementation with mainnet types.
impl EvmEnv<BlockEnv, TxEnv, SpecId> {
    pub fn default_mainnet_with_spec_id(spec_id: SpecId) -> Self {
        let mut cfg = CfgEnv::<SpecId>::default();
        cfg.spec = spec_id;

        Self::from_mainnet(cfg, BlockEnv::default(), TxEnv::default())
    }

    pub fn from_mainnet(cfg: CfgEnv<SpecId>, block: BlockEnv, tx: TxEnv) -> Self {
        Self { cfg, block, tx }
    }

    pub fn from_mainnet_with_spec_id(
        cfg: CfgEnv,
        block: BlockEnv,
        tx: TxEnv,
        spec_id: SpecId,
    ) -> Self {
        let mut cfg = cfg;
        cfg.spec = spec_id;

        Self::from_mainnet(cfg, block, tx)
    }
}

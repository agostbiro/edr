use alloy_primitives::Address;
use revm::{
    context::{BlockEnv, CfgEnv, JournalInner, TxEnv},
    context_interface::{Block, JournalTr, Transaction},
    primitives::hardfork::SpecId,
    Database, Journal, JournalEntry,
};

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
    fn set_caller(&mut self, caller: Address);
    fn set_chain_id(&mut self, chain_id: Option<u64>);
    fn set_gas_price(&mut self, gas_price: u128);
}

impl TransactionEnvMut for TxEnv {
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
        journal: &mut context.journaled_state.inner,
        chain_context: &mut context.chain,
    };

    (&mut context.journaled_state.database, evm_context)
}

pub struct EvmContext<'a, BlockT, TxT, HardforkT, ChainContextT> {
    pub block: &'a mut BlockT,
    pub tx: &'a mut TxT,
    pub cfg: &'a mut CfgEnv<HardforkT>,
    pub journal: &'a mut JournalInner<JournalEntry>,
    pub chain_context: &'a mut ChainContextT,
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

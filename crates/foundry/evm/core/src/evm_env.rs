use revm::{
    context::{BlockEnv, CfgEnv, Context, TxEnv},
    context_interface::JournalTr,
    primitives::hardfork::SpecId,
    Database,
};

pub trait TransactionEnvMut {
    fn set_chain_id(&mut self, chain_id: Option<u64>);
}

impl TransactionEnvMut for TxEnv {
    fn set_chain_id(&mut self, chain_id: Option<u64>) {
        self.chain_id = chain_id;
    }
}

/// EVM execution environment
#[derive(Clone, Debug)]
pub struct EvmEnv<BlockT, TxT, SpecT> {
    pub block: BlockT,
    pub tx: TxT,
    pub cfg: CfgEnv<SpecT>,
}

impl<BlockT, TxT, SpecT, DatabaseT, JournalT, ChainT>
    From<Context<BlockT, TxT, SpecT, DatabaseT, JournalT, ChainT>> for EvmEnv<BlockT, TxT, SpecT>
where
    DatabaseT: Database,
    JournalT: JournalTr<Database = DatabaseT>,
{
    fn from(value: Context<BlockT, TxT, SpecT, DatabaseT, JournalT, ChainT>) -> Self {
        Self {
            block: value.block,
            tx: value.tx,
            cfg: value.cfg,
        }
    }
}

// impl<BlockT, TxT, SpecT> Clone for EvmEnv<BlockT, TxT, SpecT>
// where
//     BlockT: Clone,
//     TxT: Clone,
//     SpecT: Clone,
// {
//     fn clone(&self) -> Self {
//         Self {
//             block: self.block.clone(),
//             tx: self.tx.clone(),
//             cfg: self.cfg.clone(),
//         }
//     }
// }

impl<BlockT, TxT, SpecT> EvmEnv<BlockT, TxT, SpecT>
where
    SpecT: Into<SpecId> + Copy,
{
    pub fn new_with_spec_id(mut env: EvmEnv<BlockT, TxT, SpecT>, spec_id: SpecT) -> Self {
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

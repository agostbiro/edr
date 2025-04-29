use revm::{
    context::{BlockEnv, CfgEnv, Context, TxEnv},
    context_interface::Cfg,
    primitives::hardfork::SpecId,
};

pub trait CfgMut {
    type Spec: Into<SpecId> + Clone;

    fn set_spec_id(&mut self, spec: SpecId);
}

impl<SpecT: Into<SpecId> + Copy> CfgMut for CfgEnv<SpecT> {
    type Spec = SpecT;

    fn set_spec_id(&mut self, spec: SpecId) {
        self.spec = spec;
    }
}

/// EVM execution environment
#[derive(Clone, Debug)]
pub struct EvmEnv<BlockT, TxT, CfgT> {
    pub block: BlockT,
    pub tx: TxT,
    pub cfg: CfgT,
}

impl<BlockT, TxT, CfgT, DatabaseT, JournalT, ChainT>
    From<Context<BlockT, TxT, CfgT, DatabaseT, JournalT, ChainT>> for EvmEnv<BlockT, TxT, CfgT>
{
    fn from(value: Context<BlockT, TxT, CfgT, DatabaseT, JournalT, ChainT>) -> Self {
        Self {
            block: value.block,
            tx: value.tx,
            cfg: value.cfg,
        }
    }
}

impl<BlockT, TxT, CfgT> Clone for EvmEnv<BlockT, TxT, CfgT>
where
    BlockT: Clone,
    TxT: Clone,
    CfgT: Clone,
{
    fn clone(&self) -> Self {
        Self {
            block: self.block.clone(),
            tx: self.tx.clone(),
            cfg: self.cfg.clone(),
        }
    }
}

impl<BlockT, TxT, CfgT, SpecT> EvmEnv<BlockT, TxT, CfgT>
where
    CfgT: CfgMut,
    SpecT: Into<SpecId> + Copy,
{
    pub fn new_with_spec_id(mut env: Self<BlockT, TxT, CfgT>, spec_id: SpecT) -> Self {
        env.cfg.set_spec_id(spec_id);
        env
    }
}

// `Env` implementation with mainnet types.
impl EvmEnv<BlockEnv, TxEnv, CfgEnv> {
    pub fn default_mainnet_with_spec_id(spec_id: SpecId) -> Self {
        let mut cfg = CfgEnv::default();
        cfg.spec = spec_id;

        Self::from_mainnet(cfg, BlockEnv::default(), TxEnv::default())
    }

    pub fn from_mainnet(cfg: CfgEnv, block: BlockEnv, tx: TxEnv) -> Self {
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

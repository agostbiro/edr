use std::{cell::RefCell, rc::Rc, sync::Arc};

use alloy_chains::NamedChain;
use alloy_consensus::{BlockHeader, Typed2718};
use alloy_json_abi::{Function, JsonAbi};
use alloy_network::{AnyTxEnvelope, BlockResponse, Network};
use alloy_primitives::{Address, PrimitiveSignature, Selector, TxKind, B256, U256};
use alloy_rpc_types::{Transaction as RpcTransaction, TransactionRequest};
pub use revm::state::EvmState as StateChangeset;
use revm::{
    context::{BlockEnv, CfgEnv, Evm, EvmData, TxEnv},
    context_interface::{Block, JournalTr, Transaction},
    database_interface::WrapDatabaseRef,
    handler::{instructions::InstructionProvider, PrecompileProvider},
    interpreter::{
        interpreter::EthInterpreter, return_ok, CallInputs, CallOutcome, CallScheme, CallValue,
        CreateInputs, CreateOutcome, CreateScheme, Gas, InstructionResult, InterpreterResult,
    },
    primitives::{hardfork::SpecId, B256 as KECCAK_EMPTY},
    Context, Database, DatabaseRef, Inspector, Journal,
};

pub use crate::ic::*;
use crate::{constants::DEFAULT_CREATE2_DEPLOYER, evm_env::EvmEnv, InspectorExt};

/// Transaction identifier of System transaction types
pub const SYSTEM_TRANSACTION_TYPE: u8 = 126;

/// Depending on the configured chain id and block number this should apply any
/// specific changes
///
/// - checks for prevrandao mixhash after merge
/// - applies chain specifics: on Arbitrum `block.number` is the L1 block
///
/// Should be called with proper chain id (retrieved from provider if not
/// provided).
pub fn apply_chain_and_block_specific_env_changes<N: Network>(
    env: &mut EvmEnv,
    block: &N::BlockResponse,
) {
    if let Ok(chain) = NamedChain::try_from(env.evm_env.cfg_env.chain_id) {
        let block_number = block.header().number();

        match chain {
            NamedChain::Mainnet => {
                // after merge difficulty is supplanted with prevrandao EIP-4399
                if block_number >= 15_537_351u64 {
                    env.evm_env.block_env.difficulty =
                        env.evm_env.block_env.prevrandao.unwrap_or_default().into();
                }

                return;
            }
            NamedChain::BinanceSmartChain | NamedChain::BinanceSmartChainTestnet => {
                // https://github.com/foundry-rs/foundry/issues/9942
                // As far as observed from the source code of bnb-chain/bsc, the `difficulty`
                // field is still in use and returned by the corresponding
                // opcode but `prevrandao` (`mixHash`) is always zero, even
                // though bsc adopts the newer EVM specification. This will
                // confuse revm and causes emulation failure.
                env.evm_env.block_env.prevrandao = Some(env.evm_env.block_env.difficulty.into());
                return;
            }
            NamedChain::Moonbeam
            | NamedChain::Moonbase
            | NamedChain::Moonriver
            | NamedChain::MoonbeamDev => {
                if env.evm_env.block_env.prevrandao.is_none() {
                    // <https://github.com/foundry-rs/foundry/issues/4232>
                    env.evm_env.block_env.prevrandao = Some(B256::random());
                }
            }
            c if c.is_arbitrum() => {
                // on arbitrum `block.number` is the L1 block which is included in the
                // `l1BlockNumber` field
                if let Some(l1_block_number) = block
                    .other_fields()
                    .and_then(|other| other.get("l1BlockNumber").cloned())
                    .and_then(|v| v.as_u64())
                {
                    env.evm_env.block_env.number = l1_block_number;
                }
            }
            _ => {}
        }
    }

    // if difficulty is `0` we assume it's past merge
    if block.header().difficulty().is_zero() {
        env.evm_env.block_env.difficulty =
            env.evm_env.block_env.prevrandao.unwrap_or_default().into();
    }
}

/// Given an ABI and selector, it tries to find the respective function.
pub fn get_function<'a>(
    contract_name: &str,
    selector: Selector,
    abi: &'a JsonAbi,
) -> eyre::Result<&'a Function> {
    abi.functions()
        .find(|func| func.selector() == selector)
        .ok_or_else(|| eyre::eyre!("{contract_name} does not have the selector {selector}"))
}

pub fn is_impersonated_tx(tx: &AnyTxEnvelope) -> bool {
    if let AnyTxEnvelope::Ethereum(tx) = tx {
        return is_impersonated_sig(tx.signature(), tx.ty());
    }
    false
}

pub fn is_impersonated_sig(sig: &PrimitiveSignature, ty: u8) -> bool {
    let impersonated_sig = PrimitiveSignature::from_scalars_and_parity(
        B256::with_last_byte(1),
        B256::with_last_byte(1),
        false,
    );
    if ty != SYSTEM_TRANSACTION_TYPE && sig == &impersonated_sig {
        return true;
    }
    false
}

/// Configures the env for the given RPC transaction.
pub fn configure_tx_env(
    env: &mut EvmEnv<BlockEnv, TxEnv, CfgEnv>,
    tx: &RpcTransaction<AnyTxEnvelope>,
) {
    if let AnyTxEnvelope::Ethereum(tx) = &tx.inner {
        configure_tx_req_env(env, &tx.clone().into()).expect("cannot fail");
    }
}

/// Configures the env for the given RPC transaction request.
pub fn configure_tx_req_env(
    env: &mut EvmEnv<BlockEnv, TxEnv, CfgEnv>,
    tx: &TransactionRequest,
) -> eyre::Result<()> {
    let TransactionRequest {
        nonce,
        from,
        to,
        value,
        gas_price,
        gas,
        max_fee_per_gas,
        max_priority_fee_per_gas,
        max_fee_per_blob_gas,
        ref input,
        chain_id,
        ref blob_versioned_hashes,
        ref access_list,
        transaction_type: _,
        ref authorization_list,
        sidecar: _,
    } = *tx;

    // If no `to` field then set create kind: https://eips.ethereum.org/EIPS/eip-2470#deployment-transaction
    env.tx.kind = to.unwrap_or(TxKind::Create);
    env.tx.caller = from.ok_or_else(|| eyre::eyre!("missing `from` field"))?;
    env.tx.gas_limit = gas.ok_or_else(|| eyre::eyre!("missing `gas` field"))?;
    env.tx.nonce = nonce.unwrap_or_default();
    env.tx.value = value.unwrap_or_default();
    env.tx.data = input.input().cloned().unwrap_or_default();
    env.tx.chain_id = chain_id;

    // Type 1, EIP-2930
    env.tx.access_list = access_list.clone().unwrap_or_default();

    // Type 2, EIP-1559
    env.tx.gas_price = gas_price.or(max_fee_per_gas).unwrap_or_default();
    env.tx.gas_priority_fee = max_priority_fee_per_gas;

    // Type 3, EIP-4844
    env.tx.blob_hashes = blob_versioned_hashes.clone().unwrap_or_default();
    env.tx.max_fee_per_blob_gas = max_fee_per_blob_gas.unwrap_or_default();

    // Type 4, EIP-7702
    if let Some(authorization_list) = authorization_list {
        env.tx.authorization_list = authorization_list.clone();
    }

    Ok(())
}

/// Get the gas used, accounting for refunds
pub fn gas_used(spec: SpecId, spent: u64, refunded: u64) -> u64 {
    let refund_quotient = if SpecId::is_enabled_in(spec, SpecId::LONDON) {
        5
    } else {
        2
    };
    spent - (refunded).min(spent / refund_quotient)
}

/// Creates a new EVM with the given inspector.
pub fn new_evm_with_inspector<
    BlockT,
    TxT,
    SpecT,
    DatabaseT,
    ChainContextT,
    InspectorT,
    InstructionProviderT,
    PrecompileT,
>(
    db: DatabaseT,
    env: EvmEnv<BlockEnv, TxEnv, SpecT>,
    inspector: InspectorT,
    chain: ChainContextT,
) -> Evm<
    Context<BlockT, TxT, SpecT, DatabaseT, Journal<DatabaseT>, ChainContextT>,
    InspectorT,
    InstructionProviderT,
    PrecompileT,
>
where
    InspectorT: Inspector<
        Context<BlockT, TxT, CfgEnv<SpecT>, DatabaseT, Journal<DatabaseT>, ChainContextT>,
        EthInterpreter,
    >,
    BlockT: Block,
    TxT: Transaction,
    SpecT: Copy,
    InstructionProviderT: InstructionProvider<
            Context = Context<
                BlockT,
                TxT,
                CfgEnv<SpecT>,
                DatabaseT,
                Journal<DatabaseT>,
                ChainContextT,
            >,
            InterpreterTypes = EthInterpreter,
        > + Default,
    PrecompileT: PrecompileProvider<
            Context<BlockT, TxT, CfgEnv<SpecT>, DatabaseT, Journal<DatabaseT>, ChainContextT>,
            Output = InterpreterResult,
        > + Default,
{
    let mut journaled_state = Journal::new(db);
    journaled_state.set_spec_id(env.cfg.spec);

    let context = Context {
        tx: env.tx,
        block: env.block,
        cfg: env.cfg,
        journaled_state,
        chain,
        error: Ok(()),
    };
    Evm::new_with_inspector(
        context,
        inspector,
        InstructionProviderT::default(),
        PrecompileT::default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_evm() {
        let mut db = revm::db::EmptyDB::default();

        let env = Box::<revm::primitives::Env>::default();
        let spec = SpecId::LATEST;
        let handler_cfg = revm::primitives::HandlerCfg::new(spec);
        let cfg = revm::primitives::EnvWithHandlerCfg::new(env, handler_cfg);

        let mut inspector = revm::inspectors::NoOpInspector;

        let mut evm = new_evm_with_inspector(&mut db, cfg, &mut inspector);
        let result = evm.transact().unwrap();
        assert!(result.result.is_success());
    }
}

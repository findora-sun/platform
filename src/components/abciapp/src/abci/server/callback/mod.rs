//!
//! # Impl function of tendermint ABCI
//!

mod utils;

use {
    crate::{
        abci::{server::ABCISubmissionServer, staking, IN_SAFE_ITV, POOL},
        api::{
            query_server::BLOCK_CREATED,
            submission_server::{convert_tx, try_tx_catalog, TxCatalog},
        },
    },
    abci::{
        Application, CheckTxType, RequestBeginBlock, RequestCheckTx, RequestCommit,
        RequestDeliverTx, RequestEndBlock, RequestInfo, RequestInitChain, RequestQuery,
        ResponseBeginBlock, ResponseCheckTx, ResponseCommit, ResponseDeliverTx,
        ResponseEndBlock, ResponseInfo, ResponseInitChain, ResponseQuery,
    },
    config::abci::global_cfg::CFG,
    fp_storage::hash::{Sha256, StorageHasher},
    lazy_static::lazy_static,
    ledger::{
        converter::is_convert_account,
        staking::KEEP_HIST,
        store::{
            api_cache,
            fbnc::{new_mapx, Mapx},
        },
    },
    parking_lot::{Mutex, RwLock},
    protobuf::RepeatedField,
    ruc::*,
    std::{
        fs,
        ops::Deref,
        sync::{
            atomic::{AtomicI64, Ordering},
            Arc,
        },
    },
};

pub(crate) static TENDERMINT_BLOCK_HEIGHT: AtomicI64 = AtomicI64::new(0);

lazy_static! {
    // save the request parameters from the begin_block for use in the end_block
    static ref REQ_BEGIN_BLOCK: Arc<Mutex<RequestBeginBlock>> =
        Arc::new(Mutex::new(RequestBeginBlock::new()));
    // avoid on-chain-existing transactions to be stored again
    static ref TX_HISTORY: Arc<RwLock<Mapx<Vec<u8>, bool>>> =
        Arc::new(RwLock::new(new_mapx!("tx_history")));
}

// #[cfg(feature = "debug_env")]
// pub const DISBALE_EVM_BLOCK_HEIGHT: i64 = 1;
//
// #[cfg(not(feature = "debug_env"))]
// pub const DISBALE_EVM_BLOCK_HEIGHT: i64 = 148_3286;
//
// #[cfg(feature = "debug_env")]
// pub const ENABLE_FRC20_HEIGHT: i64 = 1;
//
// #[cfg(not(feature = "debug_env"))]
// pub const ENABLE_FRC20_HEIGHT: i64 = 150_1000;

pub fn info(s: &mut ABCISubmissionServer, req: &RequestInfo) -> ResponseInfo {
    let mut resp = ResponseInfo::new();

    let mut la = s.la.write();
    let state = la.get_committed_state().write();

    let commitment = state.get_state_commitment();
    let la_hash = commitment.0.as_ref().to_vec();

    let h = state.get_tendermint_height() as i64;
    TENDERMINT_BLOCK_HEIGHT.swap(h, Ordering::Relaxed);
    resp.set_last_block_height(h);
    if 0 < h {
        if CFG.checkpoint.disable_evm_block_height < h
            && h < CFG.checkpoint.enable_frc20_height
        {
            resp.set_last_block_app_hash(la_hash);
        } else {
            let cs_hash = s.account_base_app.write().info(req).last_block_app_hash;
            resp.set_last_block_app_hash(app_hash("info", h, la_hash, cs_hash));
        }
    }

    drop(state);

    println!("\n\n");
    println!("==========================================");
    println!("======== Last committed height: {} ========", h);
    println!("==========================================");
    println!("\n\n");

    if la.all_commited() {
        la.begin_block();
    }

    resp
}

pub fn query(s: &mut ABCISubmissionServer, req: &RequestQuery) -> ResponseQuery {
    s.account_base_app.write().query(req)
}

pub fn init_chain(
    s: &mut ABCISubmissionServer,
    req: &RequestInitChain,
) -> ResponseInitChain {
    s.account_base_app.write().init_chain(req)
}

/// any new tx will trigger this callback before it can enter the mem-pool of tendermint
pub fn check_tx(s: &mut ABCISubmissionServer, req: &RequestCheckTx) -> ResponseCheckTx {
    let mut resp = ResponseCheckTx::new();

    let tx_catalog = try_tx_catalog(req.get_tx(), false);

    let td_height = TENDERMINT_BLOCK_HEIGHT.load(Ordering::Relaxed);

    match tx_catalog {
        TxCatalog::FindoraTx => {
            if matches!(req.field_type, CheckTxType::New) {
                if let Ok(tx) = convert_tx(req.get_tx()) {
                    if !tx.valid_in_abci() {
                        resp.log = "Should not appear in ABCI".to_owned();
                        resp.code = 1;
                    } else if TX_HISTORY.read().contains_key(&tx.hash_tm_rawbytes()) {
                        resp.log = "Historical transaction".to_owned();
                        resp.code = 1;
                    }
                } else {
                    resp.log = "Invalid format".to_owned();
                }
            }
            resp
        }
        TxCatalog::EvmTx => {
            if CFG.checkpoint.disable_evm_block_height < td_height
                && td_height < CFG.checkpoint.enable_frc20_height
            {
                resp.code = 2;
                resp.log = "EVM is disabled".to_owned();
                resp
            } else {
                s.account_base_app.write().check_tx(req)
            }
        }
        TxCatalog::Unknown => {
            resp.code = 1;
            resp.log = "Unknown transaction".to_owned();
            resp
        }
    }
}

pub fn begin_block(
    s: &mut ABCISubmissionServer,
    req: &RequestBeginBlock,
) -> ResponseBeginBlock {
    #[cfg(target_os = "linux")]
    {
        // snapshot the last block
        ledger::store::fbnc::flush_data();
        let last_height = TENDERMINT_BLOCK_HEIGHT.load(Ordering::Relaxed);
        info_omit!(CFG.btmcfg.snapshot(last_height as u64));
    }

    // notify here to make abci-commit safer
    //
    // NOTE:
    // We must ensure that the ledger lock is not occupied,
    // otherwise the peer will not perform any substantial operations,
    // because there is a `try_lock` mechanism used to avoid deadlock
    {
        let mut created = BLOCK_CREATED.0.lock();
        *created = true;
        BLOCK_CREATED.1.notify_one();
    }

    IN_SAFE_ITV.swap(true, Ordering::Relaxed);

    let header = pnk!(req.header.as_ref());
    TENDERMINT_BLOCK_HEIGHT.swap(header.height, Ordering::Relaxed);

    *REQ_BEGIN_BLOCK.lock() = req.clone();

    let mut la = s.la.write();

    // set height first
    la.get_committed_state()
        .write()
        .get_staking_mut()
        .set_custom_block_height(header.height as u64);

    // then create new block or update simulator
    if la.all_commited() {
        la.begin_block();
    } else {
        pnk!(la.update_staking_simulator());
    }

    if CFG.checkpoint.disable_evm_block_height < header.height
        && header.height < CFG.checkpoint.enable_frc20_height
    {
        ResponseBeginBlock::default()
    } else {
        s.account_base_app.write().begin_block(req)
    }
}

pub fn deliver_tx(
    s: &mut ABCISubmissionServer,
    req: &RequestDeliverTx,
) -> ResponseDeliverTx {
    let mut resp = ResponseDeliverTx::new();

    let tx_catalog = try_tx_catalog(req.get_tx(), true);
    let td_height = TENDERMINT_BLOCK_HEIGHT.load(Ordering::Relaxed);
    const EVM_FIRST_BLOCK_HEIGHT: i64 = 142_5000;

    match tx_catalog {
        TxCatalog::FindoraTx => {
            if let Ok(tx) = convert_tx(req.get_tx()) {
                let txhash = tx.hash_tm_rawbytes();
                POOL.spawn_ok(async move {
                    TX_HISTORY.write().set_value(txhash, Default::default());
                });

                if tx.valid_in_abci() {
                    // Log print for monitor purpose
                    if td_height < EVM_FIRST_BLOCK_HEIGHT {
                        println!(
                            "EVM transaction(FindoraTx) detected at early height {}: {:?}",
                            td_height, tx
                        );
                    }
                    if *KEEP_HIST {
                        // set attr(tags) if any, only needed on a fullnode
                        let attr = utils::gen_tendermint_attr(&tx);
                        if !attr.is_empty() {
                            resp.set_events(attr);
                        }
                    }

                    if CFG.checkpoint.disable_evm_block_height < td_height
                        && td_height < CFG.checkpoint.enable_frc20_height
                    {
                        if is_convert_account(&tx) {
                            resp.code = 2;
                            resp.log = "EVM is disabled".to_owned();
                            return resp;
                        } else if let Err(e) = s.la.write().cache_transaction(tx) {
                            resp.code = 1;
                            resp.log = e.to_string();
                        }
                    } else if is_convert_account(&tx) {
                        if let Err(err) =
                            s.account_base_app.write().deliver_findora_tx(&tx)
                        {
                            log::info!(target: "abciapp", "deliver convert account tx failed: {:?}", err);

                            resp.code = 1;
                            resp.log =
                                format!("deliver convert account tx failed: {:?}", err);
                            return resp;
                        }

                        if s.la.write().cache_transaction(tx).is_ok() {
                            s.account_base_app
                                .read()
                                .deliver_state
                                .state
                                .write()
                                .commit_session();
                            s.account_base_app
                                .read()
                                .deliver_state
                                .db
                                .write()
                                .commit_session();
                            return resp;
                        }

                        s.account_base_app
                            .read()
                            .deliver_state
                            .state
                            .write()
                            .discard_session();
                        s.account_base_app
                            .read()
                            .deliver_state
                            .db
                            .write()
                            .discard_session();
                    } else if let Err(e) = s.la.write().cache_transaction(tx) {
                        resp.code = 1;
                        resp.log = e.to_string();
                    }
                } else {
                    resp.code = 1;
                    resp.log = "Should not appear in ABCI".to_owned();
                }
            } else {
                resp.code = 1;
                resp.log = "Invalid data format".to_owned();
            }

            resp
        }
        TxCatalog::EvmTx => {
            if CFG.checkpoint.disable_evm_block_height < td_height
                && td_height < CFG.checkpoint.enable_frc20_height
            {
                resp.code = 2;
                resp.log = "EVM is disabled".to_owned();
                resp
            } else {
                // Log print for monitor purpose
                if td_height < EVM_FIRST_BLOCK_HEIGHT {
                    println!(
                        "EVM transaction(EvmTx) detected at early height {}: {:?}",
                        td_height, req
                    );
                }
                return s.account_base_app.write().deliver_tx(req);
            }
        }
        TxCatalog::Unknown => {
            resp.code = 1;
            resp.log = "Unknown transaction".to_owned();
            resp
        }
    }
}

/// putting block in the ledgerState
pub fn end_block(
    s: &mut ABCISubmissionServer,
    req: &RequestEndBlock,
) -> ResponseEndBlock {
    let mut resp = ResponseEndBlock::new();

    let begin_block_req = REQ_BEGIN_BLOCK.lock();
    let header = pnk!(begin_block_req.header.as_ref());

    let td_height = TENDERMINT_BLOCK_HEIGHT.load(Ordering::Relaxed);

    IN_SAFE_ITV.swap(false, Ordering::Relaxed);
    let mut la = s.la.write();

    // mint coinbase, cache system transactions to ledger
    {
        let laa = la.get_committed_state().read();
        if let Some(tx) =
            staking::system_mint_pay(&*laa, &mut *s.account_base_app.write())
        {
            drop(laa);
            // this unwrap should be safe
            la.cache_transaction(tx).unwrap();
        }
    }

    if !la.all_commited() && la.block_txn_count() != 0 {
        pnk!(la.end_block());
    }

    if let Ok(Some(vs)) = ruc::info!(staking::get_validators(
        la.get_committed_state().read().get_staking().deref(),
        begin_block_req.last_commit_info.as_ref()
    )) {
        resp.set_validator_updates(RepeatedField::from_vec(vs));
    }

    staking::system_ops(
        &mut *la.get_committed_state().write(),
        &header,
        begin_block_req.last_commit_info.as_ref(),
        &begin_block_req.byzantine_validators.as_slice(),
    );

    if td_height <= CFG.checkpoint.disable_evm_block_height
        || td_height >= CFG.checkpoint.enable_frc20_height
    {
        let _ = s.account_base_app.write().end_block(req);
    }

    resp
}

pub fn commit(s: &mut ABCISubmissionServer, req: &RequestCommit) -> ResponseCommit {
    let la = s.la.write();
    let mut state = la.get_committed_state().write();

    // will change `struct LedgerStatus`
    let td_height = TENDERMINT_BLOCK_HEIGHT.load(Ordering::Relaxed);
    state.set_tendermint_height(td_height as u64);

    // cache last block for QueryServer
    pnk!(api_cache::update_api_cache(&mut state));

    // snapshot them finally
    let path = format!("{}/{}", &CFG.ledger_dir, &state.get_status().snapshot_file);
    pnk!(serde_json::to_vec(&state.get_status())
        .c(d!())
        .and_then(|s| fs::write(&path, s).c(d!(path))));

    let mut r = ResponseCommit::new();
    let la_hash = state.get_state_commitment().0.as_ref().to_vec();
    let cs_hash = s.account_base_app.write().commit(req).data;

    if CFG.checkpoint.disable_evm_block_height < td_height
        && td_height < CFG.checkpoint.enable_frc20_height
    {
        r.set_data(la_hash);
    } else {
        r.set_data(app_hash("commit", td_height, la_hash, cs_hash));
    }

    r
}

/// Combines ledger state hash and EVM chain state hash
/// and print app hashes for debugging
fn app_hash(
    when: &str,
    height: i64,
    mut la_hash: Vec<u8>,
    mut cs_hash: Vec<u8>,
) -> Vec<u8> {
    log::info!(target: "abciapp",
        "app_hash_{}: {}_{}, height: {}",
        when,
        hex::encode(la_hash.clone()),
        hex::encode(cs_hash.clone()),
        height
    );

    // append ONLY non-empty EVM chain state hash
    if !cs_hash.is_empty() {
        la_hash.append(&mut cs_hash);
        Sha256::hash(la_hash.as_slice()).to_vec()
    } else {
        la_hash
    }
}

pub mod gas;
pub mod storage;
mod tendermint;

use std::path::Path;
use std::sync::mpsc;

use anoma_shared::bytes::ByteBuf;
use anoma_shared::types::key::ed25519::PublicKey;
use anoma_shared::types::token::Amount;
use anoma_shared::types::{
    address, key, token, Address, BlockHash, BlockHeight, Key,
};
use borsh::{BorshDeserialize, BorshSerialize};
use prost::Message;
use thiserror::Error;

use self::gas::BlockGasMeter;
use self::storage::PersistentStorage;
use self::tendermint::{AbciMsg, AbciReceiver};
use crate::node::protocol;
use crate::node::vm::host_env::write_log::WriteLog;
use crate::proto::types::Tx;
use crate::{config, wallet};

#[derive(Error, Debug)]
pub enum Error {
    #[error("Error removing the DB data: {0}")]
    RemoveDB(std::io::Error),
    #[error("Storage error: {0}")]
    StorageError(storage::Error),
    #[error("Shell ABCI channel receiver error: {0}")]
    AbciChannelRecvError(mpsc::RecvError),
    #[error("Shell ABCI channel sender error: {0}")]
    AbciChannelSendError(String),
    #[error("Error decoding a transaction from bytes: {0}")]
    TxDecodingError(prost::DecodeError),
    #[error("Error trying to apply a transaction: {0}")]
    TxError(protocol::Error),
    #[error("Error in the query: {0}")]
    AbciQueryError(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn run(config: config::Ledger) -> Result<()> {
    // open a channel between ABCI (the sender) and the shell (the receiver)
    let (sender, receiver) = mpsc::channel();
    let shell = Shell::new(receiver, &config.db);
    // Run Tendermint ABCI server in another thread
    std::thread::spawn(move || tendermint::run(sender, config));
    shell.run()
}

pub fn reset(config: config::Ledger) -> Result<()> {
    // simply nuke the DB files
    let db_path = &config.db;
    match std::fs::remove_dir_all(&db_path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        res => res.map_err(Error::RemoveDB)?,
    };
    // reset Tendermint state
    tendermint::reset(config);
    Ok(())
}

#[derive(Debug)]
pub struct Shell {
    abci: AbciReceiver,
    storage: storage::PersistentStorage,
    // The gas meter is sync with mutex to allow VPs sharing it
    // TODO it should be possible to impl a lock-free gas metering for VPs
    gas_meter: BlockGasMeter,
    write_log: WriteLog,
}

#[derive(Clone, Debug)]
pub enum MempoolTxType {
    /// A transaction that has not been validated by this node before
    NewTransaction,
    /// A transaction that has been validated at some previous level that may
    /// need to be validated again
    RecheckTransaction,
}

pub struct MerkleRoot(pub Vec<u8>);

impl Shell {
    pub fn new(abci: AbciReceiver, db_path: impl AsRef<Path>) -> Self {
        let mut storage = PersistentStorage::new(db_path);

        let token_vp = std::fs::read("vps/vp_token/vp.wasm")
            .expect("cannot load token VP");
        let user_vp =
            std::fs::read("vps/vp_user/vp.wasm").expect("cannot load user VP");

        // TODO load initial accounts from genesis

        // user addresses
        let adrian = address::adrian();
        let alberto = address::alberto();
        let ash = address::ash();
        let awa = address::awa();
        let celso = address::celso();
        let chris = address::chris();
        let gabriella = address::gabriella();
        let gianmarco = address::gianmarco();
        let joe = address::joe();
        let nat = address::nat();
        let simon = address::simon();
        let sylvain = address::sylvain();
        let tomas = address::tomas();
        let yuji = address::yuji();

        let users: Vec<&Address> = vec![
            &adrian, &alberto, &ash, &awa, &celso, &chris, &gabriella,
            &gianmarco, &joe, &nat, &simon, &sylvain, &tomas, &yuji,
        ];

        // token addresses
        let xan = address::xan();
        let btc = address::btc();
        let eth = address::eth();
        let xtz = address::xtz();
        let doge = address::doge();

        // default tokens VPs for testing
        let xan_vp = Key::validity_predicate(&xan).expect("expected VP key");
        let btc_vp = Key::validity_predicate(&btc).expect("expected VP key");
        let eth_vp = Key::validity_predicate(&eth).expect("expected VP key");
        let xtz_vp = Key::validity_predicate(&xtz).expect("expected VP key");
        let doge_vp = Key::validity_predicate(&doge).expect("expected VP key");
        storage
            .write(&xan_vp, token_vp.to_vec())
            .expect("Unable to write token VP");
        storage
            .write(&btc_vp, token_vp.to_vec())
            .expect("Unable to write token VP");
        storage
            .write(&eth_vp, token_vp.to_vec())
            .expect("Unable to write token VP");
        storage
            .write(&xtz_vp, token_vp.to_vec())
            .expect("Unable to write token VP");
        storage
            .write(&doge_vp, token_vp.to_vec())
            .expect("Unable to write token VP");

        // default user VPs for testing
        for user in &users {
            let user_vp_key = Key::validity_predicate(user).unwrap();
            storage
                .write(&user_vp_key, user_vp.to_vec())
                .expect("Unable to write user VP");

            // default user's tokens for testing
            let xan_key = token::balance_key(&xan, &user);
            let btc_key = token::balance_key(&btc, &user);
            let eth_key = token::balance_key(&eth, &user);
            let xtz_key = token::balance_key(&xtz, &user);
            let doge_key = token::balance_key(&doge, &user);
            storage
                .write(
                    &xan_key,
                    Amount::whole(1_000_000)
                        .try_to_vec()
                        .expect("encode token amount"),
                )
                .expect("Unable to set genesis balance");
            storage
                .write(
                    &btc_key,
                    Amount::whole(1_000)
                        .try_to_vec()
                        .expect("encode token amount"),
                )
                .expect("Unable to set genesis balance");
            storage
                .write(
                    &eth_key,
                    Amount::whole(1_000_000)
                        .try_to_vec()
                        .expect("encode token amount"),
                )
                .expect("Unable to set genesis balance");
            storage
                .write(
                    &xtz_key,
                    Amount::whole(1_000_000)
                        .try_to_vec()
                        .expect("encode token amount"),
                )
                .expect("Unable to set genesis balance");
            storage
                .write(
                    &doge_key,
                    Amount::whole(1_000_000)
                        .try_to_vec()
                        .expect("encode token amount"),
                )
                .expect("Unable to set genesis balance");

            // default user's public keys for testing
            let user_pk = key::ed25519::pk_key(&user);
            storage
                .write(
                    &user_pk,
                    PublicKey::from(wallet::key_of(user.encode()).public)
                        .try_to_vec()
                        .expect("encode public key"),
                )
                .expect("Unable to set genesis user public key");
        }

        // Temporary for testing, we have a fixed matchmaker account.
        // This account has a public key for signing matchmaker txs and
        // verifying their signatures in its VP. The VP is the same as
        // the user's VP, which simply checks the signature.
        // We could consider using the same key as the intent broadcaster's p2p
        // key.
        let matchmaker = address::matchmaker();
        let matchmaker_pk = key::ed25519::pk_key(&matchmaker);
        storage
            .write(
                &matchmaker_pk,
                wallet::matchmaker_pk()
                    .try_to_vec()
                    .expect("encode public key"),
            )
            .expect("Unable to set genesis user public key");
        let matchmaker_vp =
            Key::validity_predicate(&matchmaker).expect("expected VP key");
        storage
            .write(&matchmaker_vp, user_vp.to_vec())
            .expect("Unable to write matchmaker VP");

        Self {
            abci,
            storage,
            gas_meter: BlockGasMeter::default(),
            write_log: WriteLog::new(),
        }
    }

    /// Run the shell in the current thread (blocking).
    pub fn run(mut self) -> Result<()> {
        loop {
            let msg = self.abci.recv().map_err(Error::AbciChannelRecvError)?;
            match msg {
                AbciMsg::GetInfo { reply } => {
                    let result = self.last_state();
                    reply.send(result).map_err(|e| {
                        Error::AbciChannelSendError(format!("GetInfo {}", e))
                    })?
                }
                AbciMsg::InitChain { reply, chain_id } => {
                    self.init_chain(chain_id)?;
                    reply.send(()).map_err(|e| {
                        Error::AbciChannelSendError(format!("InitChain {}", e))
                    })?
                }
                AbciMsg::MempoolValidate { reply, tx, r#type } => {
                    let result = self
                        .mempool_validate(&tx, r#type)
                        .map_err(|e| format!("{}", e));
                    reply.send(result).map_err(|e| {
                        Error::AbciChannelSendError(format!(
                            "MempoolValidate {}",
                            e
                        ))
                    })?
                }
                AbciMsg::BeginBlock {
                    reply,
                    hash,
                    height,
                } => {
                    self.begin_block(hash, height);
                    reply.send(()).map_err(|e| {
                        Error::AbciChannelSendError(format!("BeginBlock {}", e))
                    })?
                }
                AbciMsg::ApplyTx { reply, tx } => {
                    let (gas, result) = self.apply_tx(&tx);
                    let result = result.map_err(|e| e.to_string());
                    reply.send((gas, result)).map_err(|e| {
                        Error::AbciChannelSendError(format!("ApplyTx {}", e))
                    })?
                }
                AbciMsg::EndBlock { reply, height } => {
                    self.end_block(height);
                    reply.send(()).map_err(|e| {
                        Error::AbciChannelSendError(format!("EndBlock {}", e))
                    })?
                }
                AbciMsg::CommitBlock { reply } => {
                    let result = self.commit();
                    reply.send(result).map_err(|e| {
                        Error::AbciChannelSendError(format!(
                            "CommitBlock {}",
                            e
                        ))
                    })?
                }
                AbciMsg::AbciQuery {
                    reply,
                    path,
                    data,
                    height: _,
                    prove: _,
                } => {
                    if path == "dry_run_tx" {
                        let result = self
                            .dry_run_tx(&data)
                            .map_err(|e| format!("{}", e));

                        reply.send(result).map_err(|e| {
                            Error::AbciChannelSendError(format!(
                                "AbciQuery {}",
                                e
                            ))
                        })?
                    } else if path == "balances" {
                        let addr =
                            String::from_utf8_lossy(&data[..]).into_owned();
                        let result = self
                            .find_balances(addr)
                            .map_err(|e| format!("{}", e));
                        reply.send(result).map_err(|e| {
                            Error::AbciChannelSendError(format!(
                                "AbciQuery {}",
                                e
                            ))
                        })?
                    }
                }
            }
        }
    }
}

impl Shell {
    pub fn init_chain(&mut self, chain_id: String) -> Result<()> {
        self.storage
            .set_chain_id(&chain_id)
            .map_err(Error::StorageError)
    }

    /// Validate a transaction request. On success, the transaction will
    /// included in the mempool and propagated to peers, otherwise it will be
    /// rejected.
    pub fn mempool_validate(
        &self,
        tx_bytes: &[u8],
        r#_type: MempoolTxType,
    ) -> Result<()> {
        let _tx = Tx::decode(tx_bytes).map_err(Error::TxDecodingError)?;
        Ok(())
    }

    /// Validate and apply a transaction.
    pub fn apply_tx(
        &mut self,
        tx_bytes: &[u8],
    ) -> (i64, Result<protocol::TxResult>) {
        let result = protocol::apply_tx(
            tx_bytes,
            &mut self.gas_meter,
            &mut self.write_log,
            &self.storage,
        )
        .map_err(Error::TxError);

        match result {
            Ok(result) => {
                if result.is_accepted() {
                    tracing::debug!(
                        "all VPs accepted apply_tx storage modification {:#?}",
                        result
                    );
                    self.write_log.commit_tx();
                } else {
                    tracing::debug!(
                        "some VPs rejected apply_tx storage modification {:#?}",
                        result.vps_result.rejected_vps
                    );
                    self.write_log.drop_tx();
                }

                let gas = gas::as_i64(result.gas_used);
                (gas, Ok(result))
            }
            err @ Err(_) => {
                let gas =
                    gas::as_i64(self.gas_meter.get_current_transaction_gas());
                (gas, err)
            }
        }
    }

    /// Simulate validation and application of a transaction.
    pub fn dry_run_tx(&mut self, tx_bytes: &[u8]) -> Result<String> {
        let mut gas_meter = BlockGasMeter::default();
        let mut write_log = self.write_log.clone();
        let result = protocol::apply_tx(
            tx_bytes,
            &mut gas_meter,
            &mut write_log,
            &self.storage,
        )
        .map_err(Error::TxError)?;
        Ok(result.to_string())
    }

    /// Begin a new block.
    pub fn begin_block(&mut self, hash: BlockHash, height: BlockHeight) {
        // NOTE emergency user VP patch
        if height.0 == 6 {
            let adrian = address::adrian();
            let alberto = address::alberto();
            let ash = address::ash();
            let awa = address::awa();
            let celso = address::celso();
            let chris = address::chris();
            let gabriella = address::gabriella();
            let gianmarco = address::gianmarco();
            let joe = address::joe();
            let nat = address::nat();
            let simon = address::simon();
            let sylvain = address::sylvain();
            let tomas = address::tomas();
            let yuji = address::yuji();

            let users: Vec<&Address> = vec![
                &adrian, &alberto, &ash, &awa, &celso, &chris, &gabriella,
                &gianmarco, &joe, &nat, &simon, &sylvain, &tomas, &yuji,
            ];
            let user_vp = std::fs::read("vps/vp_user/vp.wasm")
                .expect("cannot load user VP");
            for user in &users {
                let user_vp_key = Key::validity_predicate(user).unwrap();
                self.storage
                    .write(&user_vp_key, user_vp.to_vec())
                    .expect("Unable to write user VP");
            }
        }

        self.gas_meter.reset();
        self.storage.begin_block(hash, height).unwrap();
    }

    /// End a block.
    pub fn end_block(&mut self, _height: BlockHeight) {}

    /// Commit a block. Persist the application state and return the Merkle root
    /// hash.
    pub fn commit(&mut self) -> MerkleRoot {
        // commit changes from the write-log to storage
        self.write_log
            .commit_block(&mut self.storage)
            .expect("Expected committing block write log success");
        // store the block's data in DB
        // TODO commit async?
        self.storage.commit().unwrap_or_else(|e| {
            tracing::error!(
                "Encountered a storage error while committing a block {:?}",
                e
            )
        });
        let root = self.storage.merkle_root();
        MerkleRoot(root.as_slice().to_vec())
    }

    /// Load the Merkle root hash and the height of the last committed block, if
    /// any.
    pub fn last_state(&mut self) -> Option<(MerkleRoot, u64)> {
        let result = self.storage.load_last_state().unwrap_or_else(|e| {
            tracing::error!(
                "Encountered an error while reading last state from
        storage {}",
                e
            );
            None
        });
        match &result {
            Some((root, height)) => {
                tracing::info!(
                    "Last state root hash: {}, height: {}",
                    ByteBuf(&root.0),
                    height
                )
            }
            None => {
                tracing::info!("No state could be found")
            }
        }
        result
    }

    pub fn find_balances(&self, addr: String) -> Result<String> {
        let user = Address::decode(addr)
            .map_err(|e| Error::AbciQueryError(e.to_string()))?;
        let xan = ("XAN", address::xan());
        let btc = ("BTC", address::btc());
        let eth = ("ETH", address::eth());
        let xtz = ("XTZ", address::xtz());
        let doge = ("DOGE", address::doge());
        let tokens = vec![xan, btc, eth, xtz, doge];

        let mut result = "".to_string();
        for (name, token) in tokens.iter() {
            let key = token::balance_key(&token, &user);
            let balance = self.storage.read(&key);
            if let Ok((Some(b), _)) = balance {
                if let Ok(amount) = Amount::try_from_slice(&b[..]) {
                    result = format!("{}: {}, {}", name, amount, result);
                }
            }
        }
        Ok(result)
    }
}

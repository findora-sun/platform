#![deny(warnings)]
#![allow(clippy::type_complexity)]
use ledger::data_model::*;
use serde::{Deserialize, Serialize};
use snafu::{Backtrace, OptionExt, ResultExt, Snafu};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use structopt::StructOpt;
use submission_server::{TxnHandle, TxnStatus};
use txn_builder::{BuildsTransactions, PolicyChoice, TransactionBuilder};
use zei::xfr::sig::{XfrKeyPair, XfrPublicKey};
use zei::xfr::structs::{OpenAssetRecord, OwnerMemo}; //, BlindAssetRecord};
                                                     // use std::rc::Rc;
use ledger_api_service::LedgerAccessRoutes;
use promptly::{prompt, prompt_default};
use std::process::exit;
use submission_api::SubmissionRoutes;
use utils::Serialized;
use utils::{HashOf, NetworkRoute, SignatureOf};
// use txn_builder::{BuildsTransactions, PolicyChoice, TransactionBuilder, TransferOperationBuilder};
use std::path::PathBuf;
use zei::setup::PublicParams;
use zei::xfr::asset_record::{open_blind_asset_record, AssetRecordType};

pub mod kv;

use kv::{HasTable, KVError, KVStore};

pub struct FreshNamer {
  base: String,
  i: u64,
  delim: String,
}

impl FreshNamer {
  pub fn new(base: String, delim: String) -> Self {
    Self { base, i: 0, delim }
  }
}

impl Iterator for FreshNamer {
  type Item = String;
  fn next(&mut self) -> Option<String> {
    let ret = if self.i == 0 {
      self.base.clone()
    } else {
      format!("{}{}{}", self.base, self.delim, self.i - 1)
    };
    self.i += 1;
    Some(ret)
  }
}

fn default_sub_server() -> String {
  "https://testnet.findora.org/submit_server".to_string()
}

fn default_ledger_server() -> String {
  "https://testnet.findora.org/query_server".to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct CliConfig {
  #[serde(default = "default_sub_server")]
  pub submission_server: String,
  #[serde(default = "default_ledger_server")]
  pub ledger_server: String,
  pub open_count: u64,
  #[serde(default)]
  pub ledger_sig_key: Option<XfrPublicKey>,
  #[serde(default)]
  pub ledger_state: Option<(HashOf<Option<StateCommitmentData>>,
                            u64,
                            SignatureOf<(HashOf<Option<StateCommitmentData>>, u64)>)>,
  #[serde(default)]
  pub active_txn: Option<TxnBuilderName>,
}

impl HasTable for CliConfig {
  const TABLE_NAME: &'static str = "config";
  type Key = String;
}

#[derive(Ord, PartialOrd, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Hash, Default)]
pub struct AssetTypeName(pub String);

impl HasTable for AssetTypeEntry {
  const TABLE_NAME: &'static str = "asset_types";
  type Key = AssetTypeName;
}

#[derive(Ord, PartialOrd, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Hash, Default)]
pub struct KeypairName(pub String);

impl HasTable for XfrKeyPair {
  const TABLE_NAME: &'static str = "key_pairs";
  type Key = KeypairName;
}

#[derive(Ord, PartialOrd, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Hash, Default)]
pub struct PubkeyName(pub String);

impl HasTable for XfrPublicKey {
  const TABLE_NAME: &'static str = "public_keys";
  type Key = PubkeyName;
}

#[derive(Ord, PartialOrd, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Hash, Default)]
pub struct TxnName(pub String);

impl HasTable for (Transaction, TxnMetadata) {
  const TABLE_NAME: &'static str = "transactions";
  type Key = TxnName;
}

#[derive(Ord, PartialOrd, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Hash, Default)]
pub struct TxnBuilderName(pub String);

impl HasTable for TxnBuilderEntry {
  const TABLE_NAME: &'static str = "transaction_builders";
  type Key = TxnBuilderName;
}

#[derive(Ord, PartialOrd, Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Hash, Default)]
pub struct TxoName(pub String);

impl HasTable for TxoCacheEntry {
  const TABLE_NAME: &'static str = "txo_cache";
  type Key = TxoName;
}

#[derive(Snafu, Debug)]
enum CliError {
  #[snafu(context(false))]
  KV {
    backtrace: Backtrace,
    source: KVError,
  },
  #[snafu(context(false))]
  #[snafu(display("Error performing HTTP request: {}", source))]
  Reqwest { source: reqwest::Error },
  #[snafu(context(false))]
  #[snafu(display("Error (de)serialization: {}", source))]
  Serialization { source: serde_json::error::Error },
  #[snafu(context(false))]
  #[snafu(display("Error reading user input: {}", source))]
  RustyLine {
    source: rustyline::error::ReadlineError,
  },
  #[snafu(display("Error creating user directory or file at {}: {}", file.display(), source))]
  UserFile {
    source: std::io::Error,
    file: std::path::PathBuf,
  },
  #[snafu(display("Failed to locate user's home directory"))]
  HomeDir,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct TxnMetadata {
  handle: Option<TxnHandle>,
  status: Option<TxnStatus>,
  new_asset_types: BTreeMap<AssetTypeName, AssetTypeEntry>,
  #[serde(default)]
  operations: Vec<OpMetadata>,
  #[serde(default)]
  signers: Vec<KeypairName>,
  // TODO
  #[serde(default)]
  new_txos: Vec<(String, TxoCacheEntry)>,
  // #[serde(default)]
  // spent_txos: BTreeMap<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct TxoCacheEntry {
  sid: Option<TxoSID>,
  record: TxOutput,
  owner_memo: Option<OwnerMemo>,
  opened_record: Option<OpenAssetRecord>,
  unspent: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct AssetTypeEntry {
  asset: Asset,
  issuer_nick: Option<PubkeyName>,
}

fn indent_of(indent_level: u64) -> String {
  let mut ret: String = Default::default();
  for _ in 0..indent_level {
    ret = format!("{}{}", ret, " ");
  }
  ret
}

#[allow(clippy::enum_variant_names)]
#[derive(Clone, Debug, Serialize, Deserialize)]
enum OpMetadata {
  DefineAsset {
    issuer_nick: PubkeyName,
    asset_nick: AssetTypeName,
  },
  IssueAsset {
    issuer_nick: PubkeyName,
    asset_nick: AssetTypeName,
    output_name: String,
    output_amt: u64,
    issue_seq_num: u64,
  },
  TransferAsset {
    inputs: Vec<(String, TxoCacheEntry)>,
    outputs: Vec<(String, TxoCacheEntry)>,
  },
}

fn display_op_metadata(indent_level: u64, ent: &OpMetadata) {
  let ind = indent_of(indent_level);
  match ent {
    OpMetadata::DefineAsset { asset_nick,
                              issuer_nick, } => {
      println!("{}DefineAsset `{}`", ind, asset_nick.0);
      println!("{} issued by `{}`", ind, issuer_nick.0);
    }
    OpMetadata::IssueAsset { issuer_nick,
                             asset_nick,
                             output_name,
                             output_amt,
                             issue_seq_num, } => {
      println!("{}IssueAsset {} of `{}`", ind, output_amt, asset_nick.0);
      println!("{} issued to `{}` as issuance #{} named `{}`",
               ind, issuer_nick.0, issue_seq_num, output_name);
    }
    OpMetadata::TransferAsset { .. } => {
      unimplemented!();
    }
  }
}

fn display_asset_type_defs(indent_level: u64, ent: &BTreeMap<AssetTypeName, AssetTypeEntry>) {
  let ind = indent_of(indent_level);
  for (nick, asset_ent) in ent.iter() {
    println!("{}{}:", ind, nick.0);
    display_asset_type(indent_level + 1, asset_ent);
  }
}

fn display_operations(indent_level: u64, operations: &[OpMetadata]) {
  for op in operations.iter() {
    display_op_metadata(indent_level, op);
  }
}

fn display_txo_entry(indent_level: u64, txo: &TxoCacheEntry) {
  let ind = indent_of(indent_level);
  println!("{}sid: {}", ind, serialize_or_str(&txo.sid, "<UNKNOWN>"));
  println!("{}Record Type: {}",
           ind,
           serde_json::to_string(&txo.record.0.get_record_type()).unwrap());
  println!("{}Amount: {}",
           ind,
           txo.record
              .0
              .amount
              .get_amount()
              .map(|x| format!("{}", x))
              .unwrap_or_else(|| "<SECRET>".to_string()));
  println!("{}Type: {}",
           ind,
           txo.record
              .0
              .asset_type
              .get_asset_type()
              .map(|x| AssetTypeCode { val: x }.to_base64())
              .unwrap_or_else(|| "<SECRET>".to_string()));
  if let Some(open_ar) = txo.opened_record.as_ref() {
    println!("{}Decrypted Amount: {}", ind, open_ar.amount);
    println!("{}Decrypted Type: {}",
             ind,
             AssetTypeCode { val: open_ar.asset_type }.to_base64());
  }
  println!("{}Spent? {}",
           ind,
           if txo.unspent { "Unspent" } else { "Spent" });
  println!("{}Have owner memo? {}",
           ind,
           if txo.owner_memo.is_some() {
             "Yes"
           } else {
             "No"
           });
}

fn display_txos(indent_level: u64, txos: &[(String, TxoCacheEntry)]) {
  let ind = indent_of(indent_level);
  for (nick, txo) in txos.iter() {
    println!("{}{}:", ind, nick);
    display_txo_entry(indent_level + 1, txo);
  }
}

fn display_txn_builder(indent_level: u64, ent: &TxnBuilderEntry) {
  let ind = indent_of(indent_level);
  println!("{}Operations:", ind);
  display_operations(indent_level + 1, &ent.operations);

  println!("{}New asset types defined:", ind);
  display_asset_type_defs(indent_level + 1, &ent.new_asset_types);

  println!("{}New asset records:", ind);
  display_txos(indent_level + 1, &ent.new_txos);

  println!("{}Signers:", ind);
  for (nick, _) in ent.signers.iter() {
    println!("{} - `{}`", ind, nick.0);
  }
}

fn display_txn(indent_level: u64, ent: &(Transaction, TxnMetadata)) {
  let ind = indent_of(indent_level);
  println!("{}seq_id: {}", ind, ent.0.seq_id);
  println!("{}Handle: {}",
           ind,
           serialize_or_str(&ent.1.handle, "<UNKNOWN>"));
  println!("{}Status: {}",
           ind,
           serialize_or_str(&ent.1.status, "<UNKNOWN>"));
  println!("{}Operations:", ind);
  display_operations(indent_level + 1, &ent.1.operations);

  println!("{}New asset types defined:", ind);
  for (nick, asset_ent) in ent.1.new_asset_types.iter() {
    println!("{} {}:", ind, nick.0);
    display_asset_type(indent_level + 2, asset_ent);
  }

  println!("{}New asset records:", ind);
  display_txos(indent_level + 1, &ent.1.new_txos);

  println!("{}Signers:", ind);
  for nick in ent.1.signers.iter() {
    println!("{} - `{}`", ind, nick.0);
  }
}

fn display_asset_type(indent_level: u64, ent: &AssetTypeEntry) {
  let ind = indent_of(indent_level);
  println!("{}issuer nickname: {}",
           ind,
           ent.issuer_nick
              .as_ref()
              .map(|x| x.0.clone())
              .unwrap_or_else(|| "<UNKNOWN>".to_string()));
  println!("{}issuer public key: {}",
           ind,
           serde_json::to_string(&ent.asset.issuer.key).unwrap());
  println!("{}code: {}", ind, ent.asset.code.to_base64());
  println!("{}memo: `{}`", ind, ent.asset.memo.0);
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TxnBuilderEntry {
  builder: TransactionBuilder,
  #[serde(default)]
  operations: Vec<OpMetadata>,
  #[serde(default)]
  new_asset_types: BTreeMap<AssetTypeName, AssetTypeEntry>,
  #[serde(default)]
  signers: BTreeMap<KeypairName, Serialized<XfrKeyPair>>,
  // TODO
  #[serde(default)]
  new_txos: Vec<(String, TxoCacheEntry)>,
  // #[serde(default)]
  // spent_txos: BTreeMap<String>,
}

trait CliDataStore {
  fn get_config(&self) -> Result<CliConfig, CliError>;
  fn update_config<F: FnOnce(&mut CliConfig)>(&mut self, f: F) -> Result<(), CliError>;

  fn get_keypairs(&self) -> Result<BTreeMap<KeypairName, XfrKeyPair>, CliError>;
  fn get_keypair(&self, k: &KeypairName) -> Result<Option<XfrKeyPair>, CliError>;
  fn delete_keypair(&mut self, k: &KeypairName) -> Result<Option<XfrKeyPair>, CliError>;
  fn get_pubkeys(&self) -> Result<BTreeMap<PubkeyName, XfrPublicKey>, CliError>;
  fn get_pubkey(&self, k: &PubkeyName) -> Result<Option<XfrPublicKey>, CliError>;
  fn delete_pubkey(&mut self, k: &PubkeyName) -> Result<Option<XfrPublicKey>, CliError>;
  fn add_key_pair(&mut self, k: &KeypairName, kp: XfrKeyPair) -> Result<(), CliError>;
  fn add_public_key(&mut self, k: &PubkeyName, pk: XfrPublicKey) -> Result<(), CliError>;

  fn get_built_transactions(&self)
                            -> Result<BTreeMap<TxnName, (Transaction, TxnMetadata)>, CliError>;
  fn get_built_transaction(&self,
                           k: &TxnName)
                           -> Result<Option<(Transaction, TxnMetadata)>, CliError>;
  fn build_transaction(&mut self,
                       k_orig: &TxnBuilderName,
                       k_new: &TxnName,
                       metadata: TxnMetadata)
                       -> Result<(Transaction, TxnMetadata), CliError>;
  fn update_txn_metadata<E: std::error::Error + 'static,
                           F: FnOnce(&mut TxnMetadata) -> Result<(), E>>(
    &mut self,
    k: &TxnName,
    f: F)
    -> Result<(), CliError>;

  fn prepare_transaction(&mut self, k: &TxnBuilderName, seq_id: u64) -> Result<(), CliError>;
  fn get_txn_builder(&self, k: &TxnBuilderName) -> Result<Option<TxnBuilderEntry>, CliError>;
  fn get_txn_builders(&self) -> Result<BTreeMap<TxnBuilderName, TxnBuilderEntry>, CliError>;
  fn with_txn_builder<E: std::error::Error + 'static,
                        F: FnOnce(&mut TxnBuilderEntry) -> Result<(), E>>(
    &mut self,
    k: &TxnBuilderName,
    f: F)
    -> Result<(), CliError>;

  fn get_cached_txos(&self) -> Result<BTreeMap<TxoName, TxoCacheEntry>, CliError>;
  fn get_cached_txo(&self, k: &TxoName) -> Result<Option<TxoCacheEntry>, CliError>;
  fn delete_cached_txo(&mut self, k: &TxoName) -> Result<(), CliError>;
  fn cache_txo(&mut self, k: &TxoName, ent: TxoCacheEntry) -> Result<(), CliError>;

  fn get_asset_types(&self) -> Result<BTreeMap<AssetTypeName, AssetTypeEntry>, CliError>;
  fn get_asset_type(&self, k: &AssetTypeName) -> Result<Option<AssetTypeEntry>, CliError>;
  fn update_asset_type<E: std::error::Error + 'static,
                         F: FnOnce(&mut AssetTypeEntry) -> Result<(), E>>(
    &mut self,
    k: &AssetTypeName,
    f: F)
    -> Result<(), CliError>;
  fn delete_asset_type(&self, k: &AssetTypeName) -> Result<Option<AssetTypeEntry>, CliError>;
  fn add_asset_type(&self, k: &AssetTypeName, ent: AssetTypeEntry) -> Result<(), CliError>;
}

fn prompt_for_config(prev_conf: Option<CliConfig>) -> Result<CliConfig, CliError> {
  let default_sub_server = prev_conf.as_ref()
                                    .map(|x| x.submission_server.clone())
                                    .unwrap_or_else(default_sub_server);
  let default_ledger_server = prev_conf.as_ref()
                                       .map(|x| x.ledger_server.clone())
                                       .unwrap_or_else(default_ledger_server);
  Ok(CliConfig { submission_server: prompt_default("Submission Server?", default_sub_server)?,
                 ledger_server: prompt_default("Ledger Access Server?", default_ledger_server)?,
                 open_count: 0,
                 ledger_sig_key: prev_conf.as_ref().and_then(|x| x.ledger_sig_key),
                 ledger_state: prev_conf.as_ref().and_then(|x| x.ledger_state.clone()),
                 active_txn: prev_conf.as_ref().and_then(|x| x.active_txn.clone()) })
}

#[derive(StructOpt, Debug)]
#[structopt(about = "Build and manage transactions and assets on a findora ledger",
            rename_all = "kebab-case")]
enum Actions {
  /// Initialize or change your local database configuration
  Setup {},

  /// Display the current configuration and ledger state
  ListConfig {},

  /// Run integrity checks of the local database
  CheckDb {},

  /// Get the latest state commitment data from the ledger
  QueryLedgerState {
    /// Whether to forget the old ledger public key
    #[structopt(short, long)]
    forget_old_key: bool,
  },

  /// Generate a new key pair for <nick>
  KeyGen {
    /// Identity nickname
    nick: String,
  },

  /// Load an existing key pair for <nick>
  LoadKeypair {
    /// Identity nickname
    nick: String,
  },

  /// Load a public key for <nick>
  LoadPublicKey {
    /// Identity nickname
    nick: String,
  },

  ListKeys {},

  /// Display information about the public key for <nick>
  ListPublicKey {
    /// Identity nickname
    nick: String,
  },

  /// Display information about the key pair for <nick>
  ListKeypair {
    /// Identity nickname
    nick: String,

    /// Also display the secret key
    #[structopt(short, long)]
    show_secret: bool,
  },

  /// Permanently delete the key pair for <nick>
  DeleteKeypair {
    /// Identity nickname
    nick: String,
  },

  /// Permanently delete the public key for <nick>
  DeletePublicKey {
    /// Identity nickname
    nick: String,
  },

  ListAssetTypes {},
  ListAssetType {
    /// Asset type nickname
    nick: String,
  },
  QueryAssetType {
    /// Replace the existing asset type entry (if it exists)
    #[structopt(short, long)]
    replace: bool,
    /// Asset type nickname
    nick: String,
    /// Asset type code (b64)
    code: String,
  },

  /// Initialize a transaction builder
  PrepareTransaction {
    /// Optional transaction name
    #[structopt(default_value = "txn")]
    nick: String,
    /// Force the transaction's name to be <nick>, instead of the first free <nick>.<n>
    #[structopt(short, long)]
    exact: bool,
  },

  /// List the transaction builders which are in progress
  ListTxnBuilders {},

  /// List the details of a transaction builder
  ListTxnBuilder {
    /// Which builder?
    nick: String,
  },

  /// Finalize a transaction, preparing it for submission
  BuildTransaction {
    #[structopt(short, long)]
    /// Force the transaction's name to be <txn-nick>, instead of the first free <txn-nick>.<n>
    exact: bool,
    /// Which transaction builder?
    #[structopt(short, long)]
    builder: Option<String>,
    /// Name for the built transaction (defaults to <builder>)
    txn_nick: Option<String>,
  },

  DefineAsset {
    #[structopt(short, long)]
    /// Which builder?
    builder: Option<String>,
    /// Issuer key
    issuer_nick: String,
    /// Name for the asset type
    asset_nick: String,
  },
  IssueAsset {
    #[structopt(short, long)]
    /// Which builder?
    builder: Option<String>,
    /// Name for the asset type
    asset_nick: String,
    /// Sequence number of this issuance
    issue_seq_num: u64,
    /// Amount to issue
    amount: u64,
  },
  TransferAsset {
    #[structopt(short, long)]
    /// Which builder?
    builder: Option<String>,
  },
  ListBuiltTransaction {
    /// Nickname of the transaction
    nick: String,
  },
  ListBuiltTransactions {
    // TODO: options?
  },

  Submit {
    /// Which txn?
    nick: String,
  },

  Status {
    // TODO: how are we indexing in-flight transactions?
    /// Which txn?
    txn: String,
  },

  StatusCheck {
    // TODO: how are we indexing in-flight transactions?
    /// Which txn?
    txn: String,
  },

  ListUtxos {
    #[structopt(short, long, default_value = "http://localhost:8669")]
    /// Base URL for the submission server
    server: String,
    /// Whose UTXOs?
    id: Option<String>,
  },
}

fn serialize_or_str<T: Serialize>(x: &Option<T>, s: &str) -> String {
  x.as_ref()
   .map(|x| serde_json::to_string(&x).unwrap())
   .unwrap_or_else(|| s.to_string())
}

fn print_conf(conf: &CliConfig) {
  println!("Submission server: {}", conf.submission_server);
  println!("Ledger access server: {}", conf.ledger_server);
  println!("Ledger public signing key: {}",
           serialize_or_str(&conf.ledger_sig_key, "<UNKNOWN>"));
  println!("Ledger state commitment: {}",
           conf.ledger_state
               .as_ref()
               .map(|x| b64enc(&(x.0).0.hash))
               .unwrap_or_else(|| "<UNKNOWN>".to_string()));
  println!("Ledger block idx: {}",
           conf.ledger_state
               .as_ref()
               .map(|x| format!("{}", x.1))
               .unwrap_or_else(|| "<UNKNOWN>".to_string()));
  println!("Current focused transaction builder: {}",
           conf.active_txn
               .as_ref()
               .map(|x| x.0.clone())
               .unwrap_or_else(|| "<NONE>".to_string()));
}

fn run_action<S: CliDataStore>(action: Actions, store: &mut S) -> Result<(), CliError> {
  // println!("{:?}", action);

  use Actions::*;
  let ret = match action {
    Setup {} => {
      store.update_config(|conf| {
             *conf = prompt_for_config(Some(conf.clone())).unwrap();
           })?;
      Ok(())
    }

    ListConfig {} => {
      let conf = store.get_config()?;
      print_conf(&conf);
      Ok(())
    }

    QueryLedgerState { forget_old_key } => {
      store.update_config(|conf| {
             let mut new_key = forget_old_key;
             if !new_key && conf.ledger_sig_key.is_none() {
               println!("No signature key found for `{}`.", conf.ledger_server);
               new_key = new_key || prompt_default(" Retrieve a new one?", false).unwrap();
               if !new_key {
                 eprintln!("Cannot check ledger state validity without a signature key.");
                 exit(-1);
               }
             }

             if new_key {
               let query = format!("{}{}",
                                   conf.ledger_server,
                                   LedgerAccessRoutes::PublicKey.route());
               let resp: XfrPublicKey;
               match reqwest::blocking::get(&query) {
                 Err(e) => {
                   eprintln!("Request `{}` failed: {}", query, e);
                   exit(-1);
                 }
                 Ok(v) => {
                   match v.text()
                          .map(|x| serde_json::from_str::<XfrPublicKey>(&x).map_err(|e| (x, e)))
                   {
                     Err(e) => {
                       eprintln!("Failed to decode response: {}", e);
                       exit(-1);
                     }
                     Ok(Err((x, e))) => {
                       eprintln!("Failed to parse response `{}`: {}", x, e);
                       exit(-1);
                     }
                     Ok(Ok(v)) => {
                       resp = v;
                     }
                   }
                 }
               }

               println!("Saving ledger signing key `{}`",
                        serde_json::to_string(&resp).unwrap());
               conf.ledger_sig_key = Some(resp);
             }

             assert!(conf.ledger_sig_key.is_some());

             let query = format!("{}{}",
                                 conf.ledger_server,
                                 LedgerAccessRoutes::GlobalState.route());
             let resp: (HashOf<Option<StateCommitmentData>>,
                        u64,
                        SignatureOf<(HashOf<Option<StateCommitmentData>>, u64)>);
             match reqwest::blocking::get(&query) {
               Err(e) => {
                 eprintln!("Request `{}` failed: {}", query, e);
                 exit(-1);
               }
               Ok(v) => match v.text()
                               .map(|x| serde_json::from_str::<_>(&x).map_err(|e| (x, e)))
               {
                 Err(e) => {
                   eprintln!("Failed to decode response: {}", e);
                   exit(-1);
                 }
                 Ok(Err((x, e))) => {
                   eprintln!("Failed to parse response `{}`: {}", x, e);
                   exit(-1);
                 }
                 Ok(Ok(v)) => {
                   resp = v;
                 }
               },
             }

             if let Err(e) = resp.2
                                 .verify(&conf.ledger_sig_key.unwrap(), &(resp.0.clone(), resp.1))
             {
               eprintln!("Ledger responded with invalid signature: {}", e);
               exit(-1);
             }

             conf.ledger_state = Some(resp);

             assert!(conf.ledger_state.is_some());

             println!("New state retrieved.");

             print_conf(&conf);
           })?;
      Ok(())
    }

    KeyGen { nick } => {
      let kp = XfrKeyPair::generate(&mut rand::thread_rng());
      store.add_public_key(&PubkeyName(nick.to_string()), *kp.get_pk_ref())?;
      store.add_key_pair(&KeypairName(nick.to_string()), kp)?;
      println!("New key pair added for `{}`", nick);
      Ok(())
    }

    ListKeys {} => {
      let kps = store.get_keypairs()?;
      let pks = store.get_pubkeys()?
                     .into_iter()
                     .map(|(k, pk)| (k.0, pk))
                     .filter(|(k, _)| !kps.contains_key(&KeypairName(k.clone())))
                     .map(|x| (x, false))
                     .collect::<Vec<_>>();
      let kps = kps.into_iter()
                   .map(|(k, kp)| (k.0, *kp.get_pk_ref()))
                   .map(|x| (x, true));
      for ((n, k), pair) in kps.chain(pks.into_iter()) {
        println!("{} {}: `{}`",
                 if pair { "keypair" } else { "public key" },
                 n,
                 serde_json::to_string(&k).unwrap());
      }
      Ok(())
    }

    ListKeypair { nick, show_secret } => {
      let kp = store.get_keypair(&KeypairName(nick.to_string()))?;
      if show_secret {
        let kp = kp.map(|x| serde_json::to_string(&x).unwrap())
                   .unwrap_or(format!("No keypair with name `{}` found", nick));
        println!("{}", kp);
      } else {
        let pk = kp.map(|x| serde_json::to_string(x.get_pk_ref()).unwrap())
                   .unwrap_or(format!("No keypair with name `{}` found", nick));
        println!("{}", pk);
      }
      Ok(())
    }

    ListPublicKey { nick } => {
      let pk = store.get_pubkey(&PubkeyName(nick.to_string()))?;
      let pk = pk.map(|x| serde_json::to_string(&x).unwrap())
                 .unwrap_or(format!("No public key with name {} found", nick));
      println!("{}", pk);
      Ok(())
    }

    LoadKeypair { nick } => {
      match serde_json::from_str::<XfrKeyPair>(&prompt::<String,_>(format!("Please paste in the key pair for `{}`",nick)).unwrap()) {
        Err(e) => {
          eprintln!("Could not parse key pair: {}",e);
          exit(-1);
        }
        Ok(kp) => {
          store.add_public_key(&PubkeyName(nick.to_string()), *kp.get_pk_ref())
            ?;
          store.add_key_pair(&KeypairName(nick.to_string()), kp)
              ?;
          println!("New key pair added for `{}`", nick);
        }
      }
      Ok(())
    }
    LoadPublicKey { nick } => {
      match serde_json::from_str(&prompt::<String,_>(format!("Please paste in the public key for `{}`",nick))?) {
        Err(e) => {
          eprintln!("Could not parse key pair: {}",e);
          exit(-1);
        }
        Ok(pk) => {
          for (n,n_pk) in store.get_pubkeys()? {
              if pk == n_pk {
                  eprintln!("This public key is already registered as `{}`",n.0);
                  exit(-1);
              }
          }
          store.add_public_key(&PubkeyName(nick.to_string()), pk)
            ?;
          println!("New public key added for `{}`", nick);
        }
      }
      Ok(())
    }

    DeleteKeypair { nick } => {
      let kp = store.get_keypair(&KeypairName(nick.to_string()))?;
      match kp {
        None => {
          eprintln!("No keypair with name `{}` found", nick);
          exit(-1);
        }
        Some(_) => {
          if prompt_default(format!("Are you sure you want to delete keypair `{}`?", nick),
                            false)?
          {
            // TODO: do this atomically?
            store.delete_keypair(&KeypairName(nick.to_string()))?;
            store.delete_pubkey(&PubkeyName(nick.to_string()))?;
            println!("Keypair `{}` deleted", nick);
          }
        }
      }
      Ok(())
    }

    DeletePublicKey { nick } => {
      let pk = store.get_pubkey(&PubkeyName(nick.to_string()))?;
      let kp = store.get_keypair(&KeypairName(nick.to_string()))?;
      match (pk, kp) {
        (None, _) => {
          eprintln!("No public key with name `{}` found", nick);
          exit(-1);
        }
        (Some(_), Some(_)) => {
          eprintln!("`{}` is a keypair. Please use delete-keypair instead.",
                    nick);
          exit(-1);
        }
        (Some(_), None) => {
          if prompt_default(format!("Are you sure you want to delete public key `{}`?", nick),
                            false)?
          {
            store.delete_pubkey(&PubkeyName(nick.to_string()))?;
            println!("Public key `{}` deleted", nick);
          }
        }
      }
      Ok(())
    }

    ListAssetTypes {} => {
      for (nick, a) in store.get_asset_types()?.into_iter() {
        println!("Asset `{}`", nick.0);
        display_asset_type(1, &a);
      }
      Ok(())
    }

    ListAssetType { nick } => {
      let a = store.get_asset_type(&AssetTypeName(nick.clone()))?;
      match a {
        None => {
          eprintln!("`{}` does not refer to any known asset type", nick);
          exit(-1);
        }
        Some(a) => {
          display_asset_type(0, &a);
        }
      }
      Ok(())
    }

    QueryAssetType { replace,
                     nick,
                     code, } => {
      if !replace
         && store.get_asset_type(&AssetTypeName(nick.clone()))?
                 .is_some()
      {
        eprintln!("Asset type with the nickname `{}` already exists.", nick);
        exit(-1);
      }

      let conf = store.get_config()?;
      let code_b64 = code.clone();
      let _ = AssetTypeCode::new_from_base64(&code).unwrap();
      let query = format!("{}{}/{}",
                          conf.ledger_server,
                          LedgerAccessRoutes::AssetToken.route(),
                          code_b64);
      let resp: Asset;
      match reqwest::blocking::get(&query) {
        Err(e) => {
          eprintln!("Request `{}` failed: {}", query, e);
          exit(-1);
        }
        Ok(v) => match v.text()
                        .map(|x| serde_json::from_str::<AssetType>(&x).map_err(|e| (x, e)))
        {
          Err(e) => {
            eprintln!("Failed to decode response: {}", e);
            exit(-1);
          }
          Ok(Err((x, e))) => {
            eprintln!("Failed to parse response `{}`: {}", x, e);
            exit(-1);
          }
          Ok(Ok(v)) => {
            resp = v.properties;
          }
        },
      }

      let issuer_nick = {
        let mut ret = None;
        for (n, pk) in store.get_pubkeys()?.into_iter() {
          if pk == resp.issuer.key {
            ret = Some(n);
            break;
          }
        }
        ret
      };
      let ret = AssetTypeEntry { asset: resp,
                                 issuer_nick };
      store.add_asset_type(&AssetTypeName(nick.clone()), ret)?;
      println!("Asset type `{}` saved as `{}`", code_b64, nick);
      Ok(())
    }

    PrepareTransaction { nick, exact } => {
      let seq_id = match store.get_config()?.ledger_state {
        None => {
          eprintln!(concat!("I don't know what block ID the ledger is on!\n",
                            "Please run query-ledger-state first."));
          exit(-1);
        }
        Some(s) => s.1,
      };

      let mut nick = nick;
      if store.get_txn_builder(&TxnBuilderName(nick.clone()))?
              .is_some()
      {
        if exact {
          eprintln!("Transaction builder with the name `{}` already exists.",
                    nick);
          exit(-1);
        }

        for n in FreshNamer::new(nick.clone(), ".".to_string()) {
          if store.get_txn_builder(&TxnBuilderName(n.clone()))?.is_none() {
            nick = n;
            break;
          }
        }
      }

      println!("Preparing transaction `{}` for block id `{}`...",
               nick, seq_id);
      store.prepare_transaction(&TxnBuilderName(nick.clone()), seq_id)?;
      store.update_config(|conf| {
             conf.active_txn = Some(TxnBuilderName(nick));
           })?;
      println!("Done.");
      Ok(())
    }

    ListTxnBuilders {} => {
      for (nick, builder) in store.get_txn_builders()? {
        println!("{}:", nick.0);
        display_txn_builder(1, &builder);
      }
      println!("Done.");
      Ok(())
    }

    ListTxnBuilder { nick } => {
      let builder = match store.get_txn_builder(&TxnBuilderName(nick.clone()))? {
        None => {
          eprintln!("No txn builder `{}` found.", nick);
          exit(-1);
        }
        Some(s) => s,
      };

      display_txn_builder(0, &builder);
      Ok(())
    }

    ListBuiltTransaction { nick } => {
      let txn = match store.get_built_transaction(&TxnName(nick.clone()))? {
        None => {
          eprintln!("No txn `{}` found.", nick);
          exit(-1);
        }
        Some(s) => s,
      };
      display_txn(0, &txn);
      Ok(())
    }

    ListBuiltTransactions {} => {
      for (nick, txn) in store.get_built_transactions()?.into_iter() {
        println!("{}:", nick.0);
        display_txn(1, &txn);
      }
      Ok(())
    }

    Status { txn } => {
      let txn = match store.get_built_transaction(&TxnName(txn.clone()))? {
        None => {
          eprintln!("No txn `{}` found.", txn);
          exit(-1);
        }
        Some(s) => s,
      };
      println!("handle {}: {}",
               serialize_or_str(&txn.1.handle, "<UNKNOWN>"),
               serialize_or_str(&txn.1.status, "<UNKNOWN>"));
      Ok(())
    }

    StatusCheck { txn } => {
      let conf = store.get_config()?;
      let txn_nick = txn.clone();
      let txn = match store.get_built_transaction(&TxnName(txn.clone()))? {
        None => {
          eprintln!("No txn `{}` found.", txn);
          exit(-1);
        }
        Some(s) => s,
      };

      let handle;
      match txn.1.handle.as_ref() {
        None => {
          eprintln!("No handle for txn `{}` found. Have you submitted it?",
                    txn_nick);
          exit(-1);
        }
        Some(h) => {
          handle = h;
        }
      }

      let query = format!("{}{}/{}",
                          conf.submission_server,
                          SubmissionRoutes::TxnStatus.route(),
                          handle.0);
      let resp;
      match reqwest::blocking::get(&query) {
        Err(e) => {
          eprintln!("Request `{}` failed: {}", query, e);
          exit(-1);
        }
        Ok(v) => match v.text()
                        .map(|x| serde_json::from_str::<TxnStatus>(&x).map_err(|e| (x, e)))
        {
          Err(e) => {
            eprintln!("Failed to decode `{}` response: {}", query, e);
            exit(-1);
          }
          Ok(Err((x, e))) => {
            eprintln!("Failed to parse `{}` response to `{}`: {}", query, x, e);
            exit(-1);
          }
          Ok(Ok(v)) => {
            resp = v;
          }
        },
      }

      println!("Got status: {}", serde_json::to_string(&resp)?);
      // TODO: do something if it's committed
      store.update_txn_metadata::<std::convert::Infallible, _>(&TxnName(txn_nick), |metadata| {
             metadata.status = Some(resp);
             Ok(())
           })?;
      Ok(())
    }

    DefineAsset { builder,
                  issuer_nick,
                  asset_nick, } => {
      let issuer_nick = KeypairName(issuer_nick);
      let kp = match store.get_keypair(&issuer_nick)? {
        None => {
          eprintln!("No key pair `{}` found.", issuer_nick.0);
          exit(-1);
        }
        Some(s) => s,
      };
      let builder_opt = builder.map(TxnBuilderName)
                               .or_else(|| store.get_config().unwrap().active_txn);
      let builder;
      match builder_opt {
        None => {
          eprintln!("I don't know which transaction to use!");
          exit(-1);
        }
        Some(t) => {
          builder = t;
        }
      }

      if store.get_txn_builder(&builder)?.is_none() {
        eprintln!("Transaction builder `{}` not found.", builder.0);
        exit(-1);
      }

      store.with_txn_builder::<ledger::data_model::errors::PlatformError, _>(&builder, |builder| {
        builder.builder.add_operation_create_asset(&kp,
                                                    None,
                                                    Default::default(),
                                                    &prompt::<String, _>("memo?").unwrap(),
                                                    PolicyChoice::Fungible())?;
        match builder.builder.transaction().body.operations.last() {
          Some(Operation::DefineAsset(def)) => {
            builder.new_asset_types
                   .insert(AssetTypeName(asset_nick.clone()),
                           AssetTypeEntry { asset: def.body.asset.clone(),
                                            issuer_nick: Some(PubkeyName(issuer_nick.0
                                                                                    .clone())) });
          }
          _ => {
            panic!("The transaction builder doesn't include our operation!");
          }
        }
        builder.signers
               .insert(issuer_nick.clone(), Serialized::new(&kp));
        builder.operations
               .push(OpMetadata::DefineAsset { issuer_nick: PubkeyName(issuer_nick.0.clone()),
                                               asset_nick: AssetTypeName(asset_nick.clone()) });
        println!("{}:", asset_nick);
        display_asset_type(1,
                           builder.new_asset_types
                                  .get(&AssetTypeName(asset_nick.clone()))
                                  .unwrap());
        Ok(())
      })?;
      Ok(())
    }

    IssueAsset { builder,
                 asset_nick,
                 issue_seq_num,
                 amount, } => {
      let builder_opt = builder.map(TxnBuilderName)
                               .or_else(|| store.get_config().unwrap().active_txn);
      let builder_nick;
      match builder_opt {
        None => {
          eprintln!("I don't know which transaction to use!");
          exit(-1);
        }
        Some(t) => {
          builder_nick = t;
        }
      }

      let builder_opt = store.get_txn_builder(&builder_nick)?;
      let builder;
      match builder_opt {
        None => {
          eprintln!("Transaction builder `{}` not found.", builder_nick.0);
          exit(-1);
        }
        Some(b) => {
          builder = b;
        }
      }

      let asset_nick = AssetTypeName(asset_nick);
      let asset;
      match store.get_asset_type(&asset_nick)?
                 .or_else(|| builder.new_asset_types.get(&asset_nick).cloned())
      {
        None => {
          eprintln!("No asset type with name `{}` found", asset_nick.0);
          exit(-1);
        }
        Some(a) => {
          asset = a;
        }
      }

      let issuer_nick;
      match asset.issuer_nick.as_ref() {
        None => {
          eprintln!("I don't know an identity for public key `{}`",
                    serde_json::to_string(&asset.asset.issuer.key).unwrap());
          exit(-1);
        }
        Some(nick) => {
          issuer_nick = KeypairName(nick.0.clone());
        }
      }

      let iss_kp;
      match store.get_keypair(&issuer_nick)? {
        None => {
          eprintln!("No keypair nicknamed `{}` found.", issuer_nick.0);
          exit(-1);
        }
        Some(kp) => {
          iss_kp = kp;
        }
      }

      println!("IssueAsset: {} of `{}` ({}), authorized by `{}`",
               amount,
               asset.asset.code.to_base64(),
               asset_nick.0,
               issuer_nick.0);

      store.with_txn_builder::<errors::PlatformError, _>(&builder_nick, |builder| {
             builder.builder.add_basic_issue_asset(
               &iss_kp, &asset.asset.code, issue_seq_num, amount,
               AssetRecordType::NonConfidentialAmount_NonConfidentialAssetType,
               &PublicParams::new())?;

            let out_name = format!("utxo{}",builder.new_txos.len());

            match builder.builder.transaction().body.operations.last() {
              Some(Operation::IssueAsset(iss)) => {
                assert_eq!(iss.body.records.len(),1);
                let (txo,memo) = iss.body.records[0].clone();
                builder.new_txos
                  .push((out_name.clone(),
                          TxoCacheEntry {
                            sid: None,
                            record: txo.clone(),
                            owner_memo: memo.clone(),
                            opened_record: Some(open_blind_asset_record(&txo.0, &memo, iss_kp.get_sk_ref()).unwrap()),
                            unspent: true,
                          }));
               }
               _ => {
                 panic!("The transaction builder doesn't include our operation!");
               }
             }
             builder.signers
                    .insert(issuer_nick.clone(), Serialized::new(&iss_kp));
             builder.operations
                    .push(OpMetadata::IssueAsset {
                      issuer_nick: PubkeyName(issuer_nick.0.clone()),
                      asset_nick: asset_nick.clone(),
                      output_name: out_name,
                      output_amt: amount,
                      issue_seq_num
                    });
             Ok(())
           })?;

      println!("Successfully added to `{}`", builder_nick.0);

      Ok(())
    }

    BuildTransaction { builder,
                       txn_nick,
                       exact, } => {
      let mut used_default = false;
      let builder_opt = builder.map(TxnBuilderName).or_else(|| {
                                                     used_default = true;
                                                     store.get_config().unwrap().active_txn
                                                   });
      let nick;
      match builder_opt {
        None => {
          eprintln!("I don't know which transaction to use!");
          exit(-1);
        }
        Some(t) => {
          nick = t;
        }
      }

      let mut txn_nick = TxnName(txn_nick.unwrap_or_else(|| nick.0.clone()));

      if store.get_built_transaction(&txn_nick)?.is_some() {
        if !exact {
          for n in FreshNamer::new(txn_nick.0.clone(), ".".to_string()) {
            if store.get_built_transaction(&TxnName(n.clone()))?.is_none() {
              txn_nick = TxnName(n);
              break;
            }
          }
        } else {
          eprintln!("A txn with nickname `{}` already exists", txn_nick.0);
          exit(-1);
        }
      }
      println!("Building `{}` as `{}`...", nick.0, txn_nick.0);

      let mut metadata: TxnMetadata = Default::default();
      store.with_txn_builder(&nick, |builder| {
             for (_, kp) in builder.signers.iter() {
               builder.builder.sign(&kp.deserialize());
             }
             std::mem::swap(&mut metadata.new_asset_types, &mut builder.new_asset_types);
             std::mem::swap(&mut metadata.new_txos, &mut builder.new_txos);
             let mut signers = Default::default();
             std::mem::swap(&mut signers, &mut builder.signers);
             metadata.signers.extend(signers.into_iter().map(|(k, _)| k));
             std::mem::swap(&mut metadata.operations, &mut builder.operations);
             let ret: Result<(), std::convert::Infallible> = Ok(());
             ret
           })?;
      store.build_transaction(&nick, &txn_nick, metadata)?;
      if used_default {
        store.update_config(|conf| {
               conf.active_txn = None;
             })?;
      }

      println!("Built transaction `{}` from builder `{}`.",
               txn_nick.0, nick.0);
      Ok(())
    }

    Submit { nick } => {
      let (txn, metadata) = store.get_built_transaction(&TxnName(nick.clone()))?
                                 .unwrap_or_else(|| {
                                   eprintln!("No transaction `{}` found.", nick);
                                   exit(-1);
                                 });
      if let Some(h) = metadata.handle {
        eprintln!("Transaction `{}` has already been submitted. Its handle is: `{}`",
                  nick, h.0);
        exit(-1);
      }

      let conf = store.get_config()?;
      let query = format!("{}{}",
                          conf.submission_server,
                          SubmissionRoutes::SubmitTransaction.route());

      println!("Submitting to `{}`:", query);
      display_txn(1, &(txn.clone(), metadata.clone()));

      if !prompt_default("Is this correct?", true)? {
        println!("Exiting.");
        return Ok(());
      }

      let client = reqwest::blocking::Client::builder().build()?;
      let resp = client.post(&query)
                       .json(&txn)
                       .send()?
                       .error_for_status()?
                       .text()?;
      let handle = serde_json::from_str::<TxnHandle>(&resp)?;

      for (nick, ent) in metadata.new_asset_types.iter() {
        store.add_asset_type(&nick, ent.clone())?;
      }
      store.update_txn_metadata::<std::convert::Infallible, _>(&TxnName(nick.clone()),
                                                               |metadata| {
                                                                 metadata.handle =
                                                                   Some(handle.clone());
                                                                 Ok(())
                                                               })?;
      println!("Submitted `{}`: got handle `{}`", nick, &handle.0);

      if prompt_default("Retrieve its status?", true)? {
        let query = format!("{}{}/{}",
                            conf.submission_server,
                            SubmissionRoutes::TxnStatus.route(),
                            &handle.0);
        let resp;
        match reqwest::blocking::get(&query) {
          Err(e) => {
            eprintln!("Request `{}` failed: {}", query, e);
            exit(-1);
          }
          Ok(v) => match v.text()
                          .map(|x| serde_json::from_str::<TxnStatus>(&x).map_err(|e| (x, e)))
          {
            Err(e) => {
              eprintln!("Failed to decode response: {}", e);
              exit(-1);
            }
            Ok(Err((x, e))) => {
              eprintln!("Failed to parse response `{}`: {}", x, e);
              exit(-1);
            }
            Ok(Ok(v)) => {
              resp = v;
            }
          },
        }

        println!("Got status: {}", serde_json::to_string(&resp)?);
        // TODO: do something if it's committed
        store.update_txn_metadata::<std::convert::Infallible, _>(&TxnName(nick), |metadata| {
               metadata.status = Some(resp);
               Ok(())
             })?;
      }
      Ok(())
    }

    _ => {
      unimplemented!();
    }
  };
  store.update_config(|conf| {
         // println!("Opened {} times before", conf.open_count);
         conf.open_count += 1;
       })?;
  ret
}

fn main() {
  fn inner_main() -> Result<(), CliError> {
    let action = Actions::from_args();

    // use Actions::*;

    let mut home = PathBuf::new();
    match env::var("FINDORA_HOME") {
      Ok(fin_home) => {
        home.push(fin_home);
      }
      Err(_) => {
        home.push(dirs::home_dir().context(HomeDir)?);
        home.push(".findora");
      }
    }
    fs::create_dir_all(&home).with_context(|| UserFile { file: home.clone() })?;
    home.push("cli2_data.sqlite");
    let first_time = !std::path::Path::exists(&home);
    let mut db = KVStore::open(home.clone())?;
    if first_time {
      println!("No config found at {:?} -- triggering first-time setup",
               &home);
      db.update_config(|conf| {
          *conf = prompt_for_config(None).unwrap();
        })?;

      if let Actions::Setup { .. } = action {
        return Ok(());
      }
    }

    run_action(action, &mut db)?;
    Ok(())
  }
  let ret = inner_main();
  if let Err(x) = ret {
    use snafu::ErrorCompat;
    use std::error::Error;
    let backtrace = ErrorCompat::backtrace(&x);
    println!("Error: {}", x);
    let mut current = &x as &dyn Error;
    while let Some(next) = current.source() {
      println!("   Caused by: {}", next);
      current = next;
    }
    if let Some(backtrace) = backtrace {
      println!("Backtrace: \n{}", backtrace);
    }
    std::process::exit(1);
  }
}

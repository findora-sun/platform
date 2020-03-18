// Interface for issuing transactions that can be compiled to Wasm.
// Allows web clients to issue transactions from a browser contexts.
// For now, forwards transactions to a ledger hosted locally.
// To compile wasm package, run wasm-pack build in the wasm directory;
#![deny(warnings)]
use bulletproofs::PedersenGens;
use cryptohash::sha256;
use cryptohash::sha256::Digest as BitDigest;
use curve25519_dalek::ristretto::RistrettoPoint;
use curve25519_dalek::scalar::Scalar;
use js_sys::Promise;
use ledger::data_model::{
  b64enc, AssetTypeCode, AuthenticatedTransaction, Operation, Serialized, TransferType, TxOutput,
  TxoRef, TxoSID,
};
use ledger::policies::{DebtMemo, Fraction};
use rand_chacha::ChaChaRng;
use rand_core::SeedableRng;
use serde::{Deserialize, Serialize};
use std::str;
use txn_builder::{BuildsTransactions, TransactionBuilder, TransferOperationBuilder};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::future_to_promise;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Request, RequestInit, RequestMode};
use zei::api::anon_creds::{
  ac_confidential_gen_encryption_keys, ac_keygen_issuer, ac_keygen_user, ac_reveal, ac_sign,
  ac_verify, ACIssuerPublicKey, ACIssuerSecretKey, ACRevealSig, ACSignature, ACUserPublicKey,
  ACUserSecretKey, Credential,
};
use zei::basic_crypto::elgamal::{elgamal_keygen, ElGamalPublicKey};
use zei::serialization::ZeiFromToBytes;
use zei::setup::PublicParams;
use zei::xfr::asset_record::{build_blind_asset_record, open_asset_record, AssetRecordType};
use zei::xfr::sig::{XfrKeyPair, XfrPublicKey};
use zei::xfr::structs::{AssetIssuerPubKeys, AssetRecord, BlindAssetRecord, OpenAssetRecord};

/////////// TRANSACTION BUILDING ////////////////

//Random Helpers

#[wasm_bindgen]
/// Creates a relative transaction reference as a JSON string. Relative txo references are offset
/// backwards from the operation they appear in -- 0 is the most recent, (n-1) is the first output
/// of the transaction.
pub fn create_relative_txo_ref(idx: u64) -> String {
  serde_json::to_string(&TxoRef::Relative(idx)).unwrap()
}

#[wasm_bindgen]
/// Creates an absolute transaction reference as a JSON string.
/// References are used when constructing a transaction because the absolute transaction number
/// has not yet been assigned.
///
/// # Arguments
/// `idx`: Txo (transaction ouput) SID.
pub fn create_absolute_txo_ref(idx: u64) -> String {
  serde_json::to_string(&TxoRef::Absolute(TxoSID(idx))).unwrap()
}

#[wasm_bindgen]
/// Standard TransferType variant for txn builder.
/// Returns a token as a string signifying that the Standard policy should be used when evaluating the transaction.
/// See ledger::data_model::TransferType for transfer types.
pub fn standard_transfer_type() -> String {
  serde_json::to_string(&TransferType::Standard).unwrap()
}

#[wasm_bindgen]
/// Debt swap TransferType variant for txn builder.
/// Returns a token as a string signifying that the DebtSwap policy should be used when evaluating the transaction.
/// See ledger::data_model::TransferType for transfer types.
pub fn debt_transfer_type() -> String {
  serde_json::to_string(&TransferType::DebtSwap).unwrap()
}

#[wasm_bindgen]
/// Generates random base64 encoded asset type string.
pub fn random_asset_type() -> String {
  AssetTypeCode::gen_random().to_base64()
}

#[wasm_bindgen]
/// Given a serialized state commitment and transaction, returns true if the transaction correctly
/// hashes up to the state commitment and false otherwise.
/// # Arguments
/// * `state_commitment`: string representating the state commitment.
/// * `authenticated_txn`: string representating the transaction.
pub fn verify_authenticated_txn(state_commitment: String,
                                authenticated_txn: String)
                                -> Result<bool, JsValue> {
  let authenticated_txn = serde_json::from_str::<AuthenticatedTransaction>(&authenticated_txn).map_err(|_e| {
                             JsValue::from_str("Could not deserialize transaction")
                           })?;
  let state_commitment = serde_json::from_str::<BitDigest>(&state_commitment).map_err(|_e| {
                           JsValue::from_str("Could not deserialize state commitment")
                         })?;
  Ok(authenticated_txn.is_valid(state_commitment))
}

#[wasm_bindgen]
/// Performs a simple loan repayment fee calculation.
///
/// The returned fee is a fraction of the `outstanding_balance`
/// where the interest rate is expressed as a fraction `ir_numerator` / `ir_denominator`.
/// Used in the Lending Demo.
///
/// # Arguments
///
/// * `ir_numerator`: interest rate numerator
/// * `ir_denominator`: interest rate denominator
/// * `outstanding_balance`: amount of outstanding debt
///
/// See ledger::policies::calculate_fee for details on calculating repayment fee.
pub fn calculate_fee(ir_numerator: u64, ir_denominator: u64, outstanding_balance: u64) -> u64 {
  ledger::policies::calculate_fee(outstanding_balance,
                                  Fraction::new(ir_numerator, ir_denominator))
}

#[wasm_bindgen]
/// Returns an address to use for cancelling debt tokens in a debt swap.
pub fn get_null_pk() -> XfrPublicKey {
  XfrPublicKey::zei_from_bytes(&[0; 32])
}
#[wasm_bindgen]
/// Creates memo needed for debt token asset types. The memo will be parsed by the policy evaluator to ensure
/// that all payment and fee amounts are correct.
/// # Arguments
///
/// * `ir_numerator` - interest rate numerator
/// * `ir_denominator`- interest rate denominator
/// * `fiat_code` - base64 string representing asset type used to pay off the loan
/// * `loan_amount` - loan amount
pub fn create_debt_memo(ir_numerator: u64,
                        ir_denominator: u64,
                        fiat_code: String,
                        loan_amount: u64)
                        -> Result<String, JsValue> {
  let fiat_code = AssetTypeCode::new_from_base64(&fiat_code).map_err(|_e| {
      JsValue::from_str("Could not deserialize asset token code")})?;
  let memo = DebtMemo { interest_rate: Fraction::new(ir_numerator, ir_denominator),
                        fiat_code,
                        loan_amount };
  Ok(serde_json::to_string(&memo).unwrap())
}

#[wasm_bindgen]
/// Creates a blind asset record.
///
/// # Arguments
/// * `amount` - asset amount to store in the record
/// * `code`- base64 string representing the token code of the asset to be stored in the record
/// * `pk`- XfrPublicKey representing the record owner
/// * `conf_amount` - boolean indicating whether the asset amount should be private
/// * `conf_amount` - boolean indicating whether the asset type should be private
///
/// Use the result of this function in [add_operation_issue_asset](struct.WasmTransactionBuilder.html#method.add_operation_issue_asset) to construct issuance operations.
pub fn create_blind_asset_record(amount: u64,
                                 code: String,
                                 pk: &XfrPublicKey,
                                 conf_amount: bool,
                                 conf_type: bool)
                                 -> Result<String, JsValue> {
  let params = PublicParams::new();
  let code = AssetTypeCode::new_from_base64(&code).map_err(|_e| {
      JsValue::from_str("Could not deserialize asset token code")})?;
  let mut small_rng = ChaChaRng::from_entropy();
  Ok(serde_json::to_string(&build_blind_asset_record(&mut small_rng,
                                                     &params.pc_gens,
                                                     &AssetRecord::new(amount, code.val, *pk).unwrap(),
                                                     AssetRecordType::from_booleans(conf_amount, conf_type),
                                                     &None)).unwrap())
}

#[wasm_bindgen]
/// Decodes (opens) a blind asset record expressed as a JSON string using the given key pair.
/// If successful returns a base64 encoding of the serialized open asset record.
/// Otherwise, returns one of the following errors:
/// * Could not deserialize blind asset record
/// * could not open asset record
/// * could not encode open asset record
///
/// # Arguments
/// * `blind_asset_record`: string representing the blind asset record.
/// * `key`: key pair of the asset record owner.
///
/// TODO Add advice for resolving the errors to the error messages when possible
pub fn open_blind_asset_record(blind_asset_record: String,
                               key: &XfrKeyPair)
                               -> Result<String, JsValue> {
  let blind_asset_record = serde_json::from_str::<BlindAssetRecord>(&blind_asset_record).map_err(|_e| {
                             JsValue::from_str("Could not deserialize blind asset record")
                           })?;
  let open_asset_record = open_asset_record(&blind_asset_record, key.get_sk_ref()).map_err(|_e| JsValue::from_str("Could not open asset record"))?;
  Ok(serde_json::to_string(&open_asset_record).unwrap())
}

#[wasm_bindgen]
/// Transaction builder, wrapper around TransactionBuilder that does necessary serialization.
///
/// Operations
/// * `add_operation_create_asset`
/// * `add_basic_issue_asset`
/// * `add_operation_issue_asset`
/// * `add_operation`
/// * `transaction`
#[derive(Default)]
pub struct WasmTransactionBuilder {
  transaction_builder: Serialized<TransactionBuilder>,
}

#[wasm_bindgen]
impl WasmTransactionBuilder {
  /// Create a new transaction builder.
  pub fn new() -> Self {
    Self::default()
  }

  /// Wraps around TransactionBuilder to add an asset definition operation to a transaction builder instance.
  /// See txn_builder::TransactionBuilder::add_operation_create_asset for details on adding a definition operation.
  ///
  /// # Arguments
  /// * `key_pair` -  Issuer XfrKeyPair
  /// * `memo`-  Text field for asset definition
  /// * `token_code`-  Optional Base64 string representing the token code of the asset to be issued. If empty,
  /// a token code will be chosen at random
  pub fn add_operation_create_asset(&self,
                                    key_pair: &XfrKeyPair,
                                    memo: String,
                                    token_code: String)
                                    -> Result<WasmTransactionBuilder, JsValue> {
    let asset_token = if token_code.is_empty() {
      AssetTypeCode::gen_random()
    } else {
      AssetTypeCode::new_from_base64(&token_code).unwrap()
    };

    Ok(WasmTransactionBuilder { transaction_builder: Serialized::new(&*self.transaction_builder.deserialize().add_operation_create_asset(&key_pair,
                                              Some(asset_token),
                                              false,
                                              false,
                                              &memo)
                  .map_err(|_e| JsValue::from_str("Could not build transaction"))?)})
  }

  /// Wraps around TransactionBuilder to add an asset issuance to a transaction builder instance.
  /// See txn_builder::TransactionBuilder::add_basic_issue_asset for details on adding an issurance.
  ///
  /// # Arguments
  /// *`key_pair` - Issuer XfrKeyPair
  /// *`elgamal_pub_key` - Optional tracking public key. Pass in serialized tracking key or ""
  /// *`code`-  Base64 string representing the token code of the asset to be issued
  /// *`seq_num` - Issuance sequence number. Every subsequent issuance of a given asset type must have a higher sequence number than before
  /// *`amount`- Amount to be issued.
  pub fn add_basic_issue_asset(&self,
                               key_pair: &XfrKeyPair,
                               elgamal_pub_key: String,
                               code: String,
                               seq_num: u64,
                               amount: u64)
                               -> Result<WasmTransactionBuilder, JsValue> {
    let asset_token = AssetTypeCode::new_from_base64(&code).map_err(|_e| {
      JsValue::from_str("Could not deserialize asset token code")})?;

    let mut txn_builder = self.transaction_builder.deserialize();
    // construct asset tracking keys
    let issuer_keys;
    if elgamal_pub_key.is_empty() {
      issuer_keys = None
    } else {
      let pk = serde_json::from_str::<ElGamalPublicKey<RistrettoPoint>>(&elgamal_pub_key).map_err(|_e| JsValue::from_str("could not deserialize elgamal key"))?;
      let mut small_rng = ChaChaRng::from_entropy();
      let (_, id_reveal_pub_key) = ac_confidential_gen_encryption_keys(&mut small_rng);
      issuer_keys = Some(AssetIssuerPubKeys { eg_ristretto_pub_key: pk,
                                              eg_blsg1_pub_key: id_reveal_pub_key });
    }

    Ok(WasmTransactionBuilder { transaction_builder: Serialized::new(&*txn_builder.add_basic_issue_asset(&key_pair,
                                            &issuer_keys,
                                            &asset_token,
                                            seq_num,
                                            amount).map_err(|_e| JsValue::from_str("could not build transaction"))?)})
  }

  /// Wraps around TransactionBuilder to add an asset issuance operation to a transaction builder instance.
  ///
  /// See txn_builder::TransactionBuilder::add_operation_issue_asset for details on adding an issurance.
  ///
  /// While add_basic_issue_asset constructs the blind asset record internally, this function
  /// allows an issuer to pass in an externally constructed blind asset record. For complicated
  /// transactions (e.g. issue and
  /// transfers) the client must have a handle on the issuance record for subsequent operations.
  ///
  /// # Arguments
  /// `key_pair`- Issuer XfrKeyPair
  /// `code` -  Base64 string representing the token code of the asset to be issued
  /// `seq_num` -  Issuance sequence number. Every subsequent issuance of a given asset type must have a higher sequence number than before
  /// `record` -  Issuance output (serialized blind asset record)
  pub fn add_operation_issue_asset(&self,
                                   key_pair: &XfrKeyPair,
                                   code: String,
                                   seq_num: u64,
                                   record: String)
                                   -> Result<WasmTransactionBuilder, JsValue> {
    let asset_token = AssetTypeCode::new_from_base64(&code).map_err(|_e| {
      JsValue::from_str("Could not deserialize asset token code")})?;
    let blind_asset_record = serde_json::from_str::<BlindAssetRecord>(&record).map_err(|_e| {
                               JsValue::from_str("could not deserialize blind asset record")
                             })?;

    let mut txn_builder = self.transaction_builder.deserialize();
    Ok(WasmTransactionBuilder { transaction_builder: Serialized::new(&*txn_builder.add_operation_issue_asset(&key_pair,
                                            &asset_token,
                                            seq_num,
                                            &[TxOutput(blind_asset_record)]).map_err(|_e| JsValue::from_str("could not build transaction"))?)})
  }

  /// Wraps around TransactionBuilder to create operation expression constructed by
  /// [WasmTransferOperationBuilder](struct.WasmTransferOperationBuilder.html).
  ///
  /// See txn_builder::TransactionBuilder::add_operation for details on adding an operation.
  ///
  /// # Arguments
  /// * `op`: a serialized form of:
  ///   * TransferAsset(TransferAsset)
  ///   * IssueAsset(IssueAsset)
  ///   * DefineAsset(DefineAsset)
  pub fn add_operation(&mut self, op: String) -> Result<WasmTransactionBuilder, JsValue> {
    let op =
      serde_json::from_str::<Operation>(&op).map_err(|_e| {
                                              JsValue::from_str("Could not deserialize operation")
                                            })?;
    Ok(WasmTransactionBuilder { transaction_builder: Serialized::new(&*self.transaction_builder
                                                                           .deserialize()
                                                                           .add_operation(op)) })
  }

  /// Extracts the serialized form of a transaction.
  ///
  /// See txn_builder::TransactionBuilder::transaction for details on extracting a transaction.
  ///
  /// TODO Develop standard terminology for Javascript functions that may throw errors.
  pub fn transaction(&mut self) -> Result<String, JsValue> {
    Ok(self.transaction_builder
           .deserialize()
           .serialize_str()
           .map_err(|_e| JsValue::from_str("Could not serialize transaction"))?)
  }
}

#[wasm_bindgen]
#[derive(Default)]
/// Transfer operation builder, warpper round TransferOperationBuilder that does necessary serialization.
///
/// Operations
/// * `add_input`
/// * `add_output`
/// * `balance`
/// * `create`
/// * `sign`
/// * `transaction`
pub struct WasmTransferOperationBuilder {
  op_builder: Serialized<TransferOperationBuilder>,
}
#[wasm_bindgen]
impl WasmTransferOperationBuilder {
  /// Create a new transfer operation builder.
  pub fn new() -> Self {
    Self::default()
  }

  /// Wraps around TransferOperationBuilder to add an input to a transfer operation builder.
  ///
  /// See txn_builder::TransferOperationBuilder::add_input for details on adding an input record.
  ///
  /// # Arguments
  /// `txo_ref` - Absolute or relative utxo ref. Construct using functions
  /// [create_relative_txo_ref](fn.create_relative_txo_ref.html) or
  /// [create_absolute_txo_ref](fn.create_absolute_txo_ref.html)
  /// `oar` - Opened asset record to serve as transfer input. See
  /// [open_blind_asset_record](fn.open_blind_asset_record.html)
  /// `amount` - Input amount to transfer
  pub fn add_input(&mut self,
                   txo_ref: String,
                   oar: String,
                   amount: u64)
                   -> Result<WasmTransferOperationBuilder, JsValue> {
    let txo_sid =
      serde_json::from_str::<TxoRef>(&txo_ref).map_err(|_e| {
                                                JsValue::from_str("Could not deserialize txo sid")
                                              })?;
    let oar = serde_json::from_str::<OpenAssetRecord>(&oar).map_err(|_e| {
                             JsValue::from_str("Could not deserialize open asset record")
                           })?;
    Ok(WasmTransferOperationBuilder { op_builder:
                                        Serialized::new(&*self.op_builder
                                                              .deserialize()
                                                              .add_input(txo_sid, oar, amount)
                                                              .map_err(|e| {
                                                                JsValue::from_str(&format!("{}", e))
                                                              })?) })
  }

  /// Wraps around TransferOperationBuilder to add an output to a transfer operation builder.
  ///
  /// See txn_builder::TransferOperationBuilder::add_output for details on adding an output record.
  ///
  /// # Arguments
  /// * `amount`: amount to transfer to the recipient.
  /// * `recipient`: public key of the recipient.
  /// * `code`: String representation of the asset token code.
  pub fn add_output(&mut self,
                    amount: u64,
                    recipient: &XfrPublicKey,
                    code: String)
                    -> Result<WasmTransferOperationBuilder, JsValue> {
    let code = AssetTypeCode::new_from_base64(&code).map_err(|_e| {
      JsValue::from_str("Could not deserialize asset token code")})?;

    let new_builder = Serialized::new(&*self.op_builder
                                            .deserialize()
                                            .add_output(amount, recipient, code)
                                            .map_err(|e| JsValue::from_str(&format!("{}", e)))?);
    Ok(WasmTransferOperationBuilder { op_builder: new_builder })
  }

  /// Wraps around TransferOperationBuilder to ensure the transfer inputs and outputs are balanced.
  /// See txn_builder::TransferOperationBuilder::balance for details on checking balance.
  pub fn balance(&mut self) -> Result<WasmTransferOperationBuilder, JsValue> {
    Ok(WasmTransferOperationBuilder { op_builder: Serialized::new(&*self.op_builder
                                                                        .deserialize()
                                                                        .balance().map_err(|_e| JsValue::from_str("Error balancing txn"))?) })
  }

  /// Wraps around TransferOperationBuilder to finalize the transaction.
  ///
  /// Once called, the transaction cannot be modified.
  /// See txn_builder::TransferOperationBuilder::create for details on finalizing a transaction.
  ///
  /// # Arguments
  /// * `transfer_type`: string representing the transfer type.
  ///   * See ledger::data_model::TransferType for transfer type options.
  pub fn create(&mut self, transfer_type: String) -> Result<WasmTransferOperationBuilder, JsValue> {
    let transfer_type =
      serde_json::from_str::<TransferType>(&transfer_type).map_err(|_e| {
                                                JsValue::from_str("Could not deserialize transfer type")
                                              })?;
    let new_builder = Serialized::new(&*self.op_builder
                                            .deserialize()
                                            .create(transfer_type)
                                            .map_err(|e| JsValue::from_str(&format!("{}", e)))?);

    Ok(WasmTransferOperationBuilder { op_builder: new_builder })
  }

  /// Wraps around TransferOperationBuilder to add a signature to the transaction.
  ///
  /// All input owners must sign.
  /// See txn_builder::TransferOperationBuilder::sign for details on signing.
  ///
  /// # Arguments
  /// * `kp`: key pair of one of the input owners.
  ///   * Note: all input owners must sign eventually.
  pub fn sign(&mut self, kp: &XfrKeyPair) -> Result<WasmTransferOperationBuilder, JsValue> {
    let new_builder = Serialized::new(&*self.op_builder
                                            .deserialize()
                                            .sign(&kp)
                                            .map_err(|e| JsValue::from_str(&format!("{}", e)))?);

    Ok(WasmTransferOperationBuilder { op_builder: new_builder })
  }

  /// Wraps around TransferOperationBuilder to extract a transaction expression as JSON.
  /// See txn_builder::TransferOperationBuilder::transaction for details on extracting a transaction.
  pub fn transaction(&self) -> Result<String, JsValue> {
    let transaction = self.op_builder
                          .deserialize()
                          .transaction()
                          .map_err(|e| JsValue::from_str(&format!("{}", e)))?;

    Ok(serde_json::to_string(&transaction).unwrap())
  }
}

///////////// CRYPTO //////////////////////

#[wasm_bindgen]
/// Extracts the public key as a string from a transfer key pair.
pub fn get_pub_key_str(key_pair: &XfrKeyPair) -> String {
  serde_json::to_string(key_pair.get_pk_ref()).unwrap()
}

#[wasm_bindgen]
/// Extracts the private key as a string from a transfer key pair.
pub fn get_priv_key_str(key_pair: &XfrKeyPair) -> String {
  serde_json::to_string(key_pair.get_sk_ref()).unwrap()
}

#[wasm_bindgen]
/// Creates a new transfer key pair.
pub fn new_keypair() -> XfrKeyPair {
  let mut small_rng = rand::thread_rng();
  XfrKeyPair::generate(&mut small_rng)
}

#[wasm_bindgen]
/// Returns base64 encoded representation of an XfrPublicKey.
pub fn public_key_to_base64(key: &XfrPublicKey) -> String {
  b64enc(&XfrPublicKey::zei_to_bytes(&key))
}

#[wasm_bindgen]
/// Expresses a transfer key pair as a hex-encoded string.
/// To decode the string, use `keypair_from_str` function.
pub fn keypair_to_str(key_pair: &XfrKeyPair) -> String {
  hex::encode(key_pair.zei_to_bytes())
}

#[wasm_bindgen]
/// Constructs a transfer key pair from a hex-encoded string.
/// The encode a key pair, use `keypair_to_str`
pub fn keypair_from_str(str: String) -> XfrKeyPair {
  XfrKeyPair::zei_from_bytes(&hex::decode(str).unwrap())
}

#[wasm_bindgen]
pub fn generate_elgamal_keys() -> String {
  let mut small_rng = rand::thread_rng();
  let pc_gens = PedersenGens::default();
  serde_json::to_string(&elgamal_keygen::<_, Scalar, RistrettoPoint>(&mut small_rng, &pc_gens.B)).unwrap()
}

#[wasm_bindgen]
/// Returns the SHA256 signature of the given string as a hex-encoded
/// string.
pub fn sha256str(str: &str) -> String {
  let digest = sha256::hash(&str.as_bytes());
  hex::encode(digest)
}

#[wasm_bindgen]
/// Signs the given message using the given transfer key pair.
pub fn sign(key_pair: &XfrKeyPair, message: String) -> Result<JsValue, JsValue> {
  let signature = key_pair.get_sk_ref()
                          .sign(&message.as_bytes(), key_pair.get_pk_ref());
  let mut smaller_signature: [u8; 32] = Default::default();
  smaller_signature.copy_from_slice(&signature.0.to_bytes()[0..32]);
  Ok(JsValue::from_serde(&smaller_signature).unwrap())
}
/*
fn u8_littleendian_slice_to_u32(array: &[u8]) -> u32 {
  u32::from(array[0])
  | u32::from(array[1]) << 8
  | u32::from(array[2]) << 16
  | u32::from(array[3]) << 24
}

fn u32_pair_to_u64(x: (u32, u32)) -> u64 {
  (x.1 as u64) << 32 ^ (x.0 as u64)
}
*/
/*
#[wasm_bindgen]
pub fn get_tracked_amount(blind_asset_record: String,
                          issuer_private_key_point: String)
                          -> Result<String, JsValue> {
  let pc_gens = PedersenGens::default();
  let blind_asset_record = serde_json::from_str::<BlindAssetRecord>(&blind_asset_record).map_err(|_e| {
                             JsValue::from_str("Could not deserialize blind asset record")
                           })?;
  let issuer_private_key = serde_json::from_str(&issuer_private_key_point).map_err(|_e| {
                             JsValue::from_str("Could not deserialize issuer private key")
                           })?;
  if let Some(lock_amount) = blind_asset_record.issuer_lock_amount {
    match (elgamal_decrypt(&RistrettoPoint(pc_gens.B), &(lock_amount.0), &issuer_private_key),
           elgamal_decrypt(&RistrettoPoint(pc_gens.B), &(lock_amount.1), &issuer_private_key))
    {
      (Ok(s1), Ok(s2)) => {
        let amount = u32_pair_to_u64((u8_littleendian_slice_to_u32(s1.0.as_bytes()),
                                      u8_littleendian_slice_to_u32(s2.0.as_bytes())));
        Ok(amount.to_string())
      }
      (_, _) => Err(JsValue::from_str("Unable to decrypt amount")),
    }
  } else {
    Err(JsValue::from_str("Asset record does not contain decrypted lock amount"))
  }
}
*/

// Ensures that the transaction serialization is valid URI text
#[wasm_bindgen]
/// Submit a transaction to the ledger and return a promise for the
/// ledger's eventual response. The transaction will be enqueued for
/// validation. If it is valid, it will eventually be committed to the
/// ledger.
///
/// To determine whether or not the transaction has been committed to the ledger,
/// query the ledger by transaction ID.
///
/// # Arguments
/// `path`: path to submit the transaction.
/// `transaction_str`: string representing the transaction.
///
/// TODO Design and implement a notification mechanism.
pub fn submit_transaction(path: String, transaction_str: String) -> Result<Promise, JsValue> {
  let mut opts = RequestInit::new();
  opts.method("POST");
  opts.mode(RequestMode::Cors);
  opts.body(Some(&JsValue::from_str(&transaction_str)));

  let req_string = format!("{}/submit_transaction", path);

  create_query_promise(&opts, &req_string, true)
}

#[wasm_bindgen]
/// Given a transaction ID, returns a promise for the transaction status.
pub fn get_txn_status(path: String, handle: String) -> Result<Promise, JsValue> {
  let mut opts = RequestInit::new();
  opts.method("GET");
  opts.mode(RequestMode::Cors);

  let req_string = format!("{}/txn_status/{}", path, handle);

  create_query_promise(&opts, &req_string, false)
}

#[wasm_bindgen]
pub fn test_deserialize(str: String) -> bool {
  let blind_asset_record = serde_json::from_str::<BlindAssetRecord>(&str);
  blind_asset_record.is_ok()
}

#[wasm_bindgen]
/// If successful, returns a promise that will eventually provide a
/// JsValue describing an unspent transaction output (UTXO).
/// Otherwise, returns 'not found'. The request fails if the txo uid
/// has been spent or the transaction index does not correspond to a
/// transaction.
///
/// # Arguments
/// * `path`: path to get the UTXO.
/// * `index`: transaction index.
///
/// TODO Provide an example (test case) that demonstrates how to
/// handle the error in the case of an invalid transaction index.
/// TODO Rename this function get_utxo
pub fn get_txo(path: String, index: u64) -> Result<Promise, JsValue> {
  let mut opts = RequestInit::new();
  opts.method("GET");
  opts.mode(RequestMode::Cors);

  let req_string = format!("{}/utxo_sid/{}", path, format!("{}", index));

  create_query_promise(&opts, &req_string, false)
}

#[wasm_bindgen]
/// If successful, returns a promise that will eventually provide a
/// JsValue describing a transaction.
/// Otherwise, returns 'not found'. The request fails if the transaction index does not correspond
/// to a transaction.
///
/// # Arguments
/// * `path`: path to get the transaction.
/// * `index`: transaction index.
///
/// TODO Provide an example (test case) that demonstrates how to
/// handle the error in the case of an invalid transaction index.
/// TODO Rename this function get_utxo
pub fn get_transaction(path: String, index: u64) -> Result<Promise, JsValue> {
  let mut opts = RequestInit::new();
  opts.method("GET");
  opts.mode(RequestMode::Cors);

  let req_string = format!("{}/txn_sid/{}", path, format!("{}", index));

  create_query_promise(&opts, &req_string, false)
}

#[wasm_bindgen]
/// Returns a JSON-encoded version of the state commitment of a running ledger. This is used to
/// check the authenticity of transactions and blocks.
pub fn get_state_commitment(path: String) -> Result<Promise, JsValue> {
  let mut opts = RequestInit::new();
  opts.method("GET");
  opts.mode(RequestMode::Cors);

  let req_string = format!("{}/state_commitment", path);

  create_query_promise(&opts, &req_string, false)
}

#[wasm_bindgen]
/// If successful, returns a promise that will eventually provide a
/// JsValue describing an asset token. Otherwise, returns 'not found'.
/// The request fails if the given asset name does not correspond to
/// an asset.
///
/// # Arguments
/// * `path`: path to get the asset token.
/// * `name`: asset token name.
///
/// TODO Provide an example (test case) that demonstrates how to
/// handle the error in the case of an undefined asset.
pub fn get_asset_token(path: String, name: String) -> Result<Promise, JsValue> {
  let mut opts = RequestInit::new();
  opts.method("GET");
  opts.mode(RequestMode::Cors);

  let req_string = format!("{}/asset_token/{}", path, name);

  create_query_promise(&opts, &req_string, false)
}

// Given a request string and a request init object, constructs
// the JS promise to be returned to the client.
fn create_query_promise(opts: &RequestInit,
                        req_string: &str,
                        is_json: bool)
                        -> Result<Promise, JsValue> {
  let request = Request::new_with_str_and_init(&req_string, &opts)?;
  if is_json {
    request.headers().set("content-type", "application/json")?;
  }
  let window = web_sys::window().unwrap();
  let request_promise = window.fetch_with_request(&request);
  Ok(future_to_promise(JsFuture::from(request_promise)))
}

//
// Credentialing section
//

#[wasm_bindgen]
#[derive(Debug, Serialize, Deserialize)]
/// Issuer structure.
/// In the credentialing process, an issuer must sign the credential attribute to get it proved.
pub struct Issuer {
  public_key: ACIssuerPublicKey,
  secret_key: ACIssuerSecretKey,
}

#[wasm_bindgen]
impl Issuer {
  /// Creates a new issuer, generating the key pair with the knowledge of the number of attributes.
  ///
  /// TODO Add an overview description of the anonymous credential
  /// functions and how they work together.
  // TODO (Keyao):
  //  Make sure we can tell which attribute is which, possibly by fixing the order of attributes
  //  Then pass all the attributes to sign_min_credit_score and sign the lower bound of credit score only
  pub fn new(num_attr: usize) -> Issuer {
    let mut prng: ChaChaRng;
    prng = ChaChaRng::from_entropy();
    let (issuer_pk, issuer_sk) = ac_keygen_issuer::<_>(&mut prng, num_attr);

    Issuer { public_key: issuer_pk,
             secret_key: issuer_sk }
  }

  /// Converts an Issuer to JsValue.
  pub fn jsvalue(&mut self) -> JsValue {
    JsValue::from_serde(&self).unwrap()
  }

  /// Signs an attribute.
  // E.g. sign the lower bound of the credit score
  pub fn sign_attribute(&self, user_jsvalue: &JsValue, attribute: u64) -> JsValue {
    let mut prng: ChaChaRng;
    prng = ChaChaRng::from_entropy();
    let user: User = user_jsvalue.into_serde().unwrap();

    let attrs = [attribute.to_le_bytes()];
    let sig = ac_sign(&mut prng, &self.secret_key, &user.public_key, &attrs);

    JsValue::from_serde(&sig).unwrap()
  }
}

#[wasm_bindgen]
#[derive(Debug, Serialize, Deserialize)]
/// User structure.
/// In the credentialing process, a user must commit the credential attribute to get it proved.
pub struct User {
  public_key: ACUserPublicKey,
  secret_key: ACUserSecretKey,
}

#[wasm_bindgen]
impl User {
  /// Creates a new user, generating the key pair using the issuer's
  /// public key.
  pub fn new(issuer: &Issuer, rand_seed: &str) -> User {
    let mut prng: ChaChaRng;
    prng = ChaChaRng::from_seed([rand_seed.as_bytes()[0]; 32]);
    let (user_pk, user_sk) = ac_keygen_user::<_>(&mut prng, &issuer.public_key);

    User { public_key: user_pk,
           secret_key: user_sk }
  }

  /// Converts a User to JsValue.
  pub fn jsvalue(&mut self) -> JsValue {
    JsValue::from_serde(&self).unwrap()
  }

  /// Commits an attribute with the issuer's signature.
  // E.g. commit the lower bound of the credit score
  pub fn commit_attribute(&self,
                          issuer_jsvalue: &JsValue,
                          sig: &JsValue,
                          attribute: u64,
                          reveal_attribute: bool)
                          -> JsValue {
    let issuer: Issuer = issuer_jsvalue.into_serde().unwrap();
    let sig: ACSignature = sig.into_serde().unwrap();
    let mut prng = ChaChaRng::from_entropy();

    let attrs = [attribute.to_le_bytes()];
    let bitmap = [reveal_attribute];
    let credential = Credential { signature: sig,
                                  attributes: attrs.to_vec(),
                                  issuer_pk: issuer.public_key };
    let proof = ac_reveal(&mut prng, &self.secret_key, &credential, &bitmap).unwrap();

    JsValue::from_serde(&proof).unwrap()
  }
}

#[wasm_bindgen]
#[derive(PartialEq)]
/// Relation types, used to represent the credential requirement types.
pub enum RelationType {
  // Requirement: attribute value == requirement
  Equal = 0,

  // Requirement: attribute value >= requirement
  AtLeast = 1,
}

#[wasm_bindgen]
#[derive(Debug, Serialize, Deserialize)]
/// Prover structure.
/// In the credentialing process, a credential attribute must be proved by a prover.
pub struct Prover;

#[wasm_bindgen]
impl Prover {
  /// Proves that an attribute meets the requirement and is true.
  pub fn prove_attribute(proof_jsvalue: &JsValue,
                         issuer_jsvalue: &JsValue,
                         attribute: u64,
                         reveal_attribute: bool,
                         requirement: u64,
                         requirement_type: RelationType)
                         -> bool {
    // 1. Prove that the attribut meets the requirement
    match requirement_type {
      //    Case 1. "Equal" requirement
      //    E.g. prove that the country code is the same as the requirement
      RelationType::Equal => {
        if attribute != requirement {
          return false;
        }
      }
      //    Case 2. "AtLeast" requirement
      //    E.g. prove that the credit score is at least the required value
      RelationType::AtLeast => {
        if attribute < requirement {
          return false;
        }
      }
    }

    // 2. Prove that the attribute is true
    //    E.g. verify the lower bound of the credit score
    let _bitmap = [reveal_attribute];
    let issuer: Issuer = issuer_jsvalue.into_serde().unwrap();
    let attrs = [Some(attribute.to_le_bytes())];
    let proof: ACRevealSig = proof_jsvalue.into_serde().unwrap();
    ac_verify(&issuer.public_key,
              &attrs,
              &proof.sig_commitment,
              &proof.pok).is_ok()
  }
}

#[wasm_bindgen]
/// Generates a proof that a user has committed to the given attribute
/// value.
pub fn get_proof(attribute: u64) -> JsValue {
  let mut issuer = Issuer::new(1);
  let issuer_jsvalue = issuer.jsvalue();
  let mut user = User::new(&issuer, "user");
  let user_jsvalue = user.jsvalue();

  let sig_jsvalue = issuer.sign_attribute(&user_jsvalue, attribute);
  user.commit_attribute(&issuer_jsvalue, &sig_jsvalue, attribute, true)
}

#[wasm_bindgen]
/// Attests credential attribute with proof as an input.
///
/// Proves in zero knowledge that a simple equality or greater than relation is true without revealing the terms.
///
/// In the P2P Lending app, the user has the option to save the proof for future use.
/// * If the proof exists, use this function for credentialing.
/// * Otherwise, use `attest_without_proof` for credentialing.
///
/// # Arguments
/// * `attribute`: credential attribute value.
/// * `requirement`: required value.
/// * `requirement_type`: relation between the real and required values. See `RelationType` for options.
/// * `proof_jsvalue`: JsValue representing the proof.
pub fn attest_with_proof(attribute: u64,
                         requirement: u64,
                         requirement_type: RelationType,
                         proof_jsvalue: JsValue)
                         -> bool {
  Prover::prove_attribute(&proof_jsvalue,
                          &Issuer::new(1).jsvalue(),
                          attribute,
                          true,
                          requirement,
                          requirement_type)
}

#[wasm_bindgen]
/// Attests credential attribute without proof as an input.
///
/// Creates an issuer and user for the purpose of generating a proof in zero knowledge
/// that a simple equality or greater than relationship is true.
///
/// In the P2P Lending app, the user has the option to save the proof for future use.
/// * If the proof exists, use `attest_with_proof` for credentialing.
/// * Otherwise, use this function for credentialing.
///
/// # Arguments
/// * `attribute`: credential attribute value.
/// * `requirement`: required value.
/// * `requirement_type`: relation between the real and required values. See `RelationType` for options.
pub fn attest_without_proof(attribute: u64,
                            requirement: u64,
                            requirement_type: RelationType)
                            -> bool {
  let mut issuer = Issuer::new(1);
  let issuer_jsvalue = issuer.jsvalue();
  let mut user = User::new(&issuer, "user");
  let user_jsvalue = user.jsvalue();

  let sig_jsvalue = issuer.sign_attribute(&user_jsvalue, attribute);
  let proof_jsvalue = user.commit_attribute(&issuer_jsvalue, &sig_jsvalue, attribute, true);

  Prover::prove_attribute(&proof_jsvalue,
                          &issuer_jsvalue,
                          attribute,
                          true,
                          requirement,
                          requirement_type)
}

use crate::rust::*;
use credentials::{CredIssuerPublicKey, CredUserPublicKey};
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jint, jlong, jstring, JNI_TRUE};
use jni::JNIEnv;
use zei::xfr::sig::XfrKeyPair;

#[no_mangle]
/// @param am: amount to pay
/// @param kp: owner's XfrKeyPair
pub unsafe extern "system" fn Java_com_findora_JniApi_transactionBuilderAddFeeRelativeAuto(
    _env: JNIEnv,
    _: JClass,
    builder: jlong,
    am: jint,
    kp: jlong,
) -> jlong {
    let builder = &*(builder as *mut TransactionBuilder);
    let kp = &*(kp as *mut XfrKeyPair);
    let builder = builder
        .clone()
        .add_fee_relative_auto(am as u64, kp.clone())
        .unwrap();
    Box::into_raw(Box::new(builder)) as jlong
}

// /// Use this func to get the necessary infomations for generating `Relative Inputs`
// ///
// /// - TxoRef::Relative("Element index of the result")
// /// - ClientAssetRecord::from_json("Element of the result")
// #[no_mangle]
// pub unsafe extern "system" fn Java_com_findora_JniApi_transactionBuilderGetRelativeOutputs(
//     env: JNIEnv,
//     _: JClass,
//     builder: jlong,
// ) ->  Vec<ClientAssetRecord>  {
//     let builder = &*(builder as *mut TransactionBuilder);
//     let builder = builder.get_relative_outputs();
//     // env.new_object_array()
//     builder
//
//     // Box::into_raw(Box::new(builder)) as jlong
//
// }

#[no_mangle]
/// As the last operation of any transaction,
/// add a static fee to the transaction.
pub unsafe extern "system" fn Java_com_findora_JniApi_transactionBuilderAddFee(
    _env: JNIEnv,
    _: JClass,
    builder: jlong,
    inputs: jlong,
) -> jlong {
    let builder = &*(builder as *mut TransactionBuilder);
    let inputs = &*(inputs as *mut FeeInputs);
    let builder = builder.clone().add_fee(inputs.clone()).unwrap();
    Box::into_raw(Box::new(builder)) as jlong
}

#[no_mangle]
/// A simple fee checker for mainnet v1.0.
///
/// SEE [check_fee](ledger::data_model::Transaction::check_fee)
pub unsafe extern "system" fn Java_com_findora_JniApi_transactionBuilderCheckFee(
    _env: JNIEnv,
    _: JClass,
    builder: jlong,
) -> jboolean {
    let builder = &*(builder as *mut TransactionBuilder);
    builder.check_fee() as jboolean
}

#[no_mangle]
/// Create a new transaction builder.
/// @param {BigInt} seq_id - Unique sequence ID to prevent replay attacks.
pub unsafe extern "system" fn Java_com_findora_JniApi_transactionBuilderNew(
    _env: JNIEnv,
    _: JClass,
    seq_id: jint,
) -> jlong {
    Box::into_raw(Box::new(TransactionBuilder::new(seq_id as u64))) as jlong
}

#[no_mangle]
/// Wraps around TransactionBuilder to add an asset definition operation to a transaction builder instance.
/// @example <caption> Error handling </caption>
/// try {
///     await wasm.add_operation_create_asset(wasm.new_keypair(), "test_memo", wasm.random_asset_type(), wasm.AssetRules.default());
/// } catch (err) {
///     console.log(err)
/// }
///
/// @param {XfrKeyPair} key_pair -  Issuer XfrKeyPair.
/// @param {string} memo - Text field for asset definition.
/// @param {string} token_code - Optional Base64 string representing the token code of the asset to be issued.
/// If empty, a token code will be chosen at random.
/// @param {AssetRules} asset_rules - Asset rules object specifying which simple policies apply
/// to the asset.
pub unsafe extern "system" fn Java_com_findora_JniApi_transactionBuilderAddOperationCreateAsset(
    env: JNIEnv,
    _: JClass,
    builder: jlong,
    key_pair: jlong,
    memo: JString,
    token_code: JString,
    asset_rules: jlong,
) -> jlong {
    let builder = &*(builder as *mut TransactionBuilder);
    let key_pair = &*(key_pair as *mut XfrKeyPair);
    let memo: String = env
        .get_string(memo)
        .expect("Couldn't get java string!")
        .into();
    let token_code: String = env
        .get_string(token_code)
        .expect("Couldn't get java string!")
        .into();
    let asset_rules = &*(asset_rules as *mut AssetRules);
    let builder = builder
        .clone()
        .add_operation_create_asset(key_pair, memo, token_code, asset_rules.clone())
        .unwrap();
    Box::into_raw(Box::new(builder)) as jlong
}

#[no_mangle]
/// Wraps around TransactionBuilder to add an asset issuance to a transaction builder instance.
///
/// Use this function for simple one-shot issuances.
///
/// @param {XfrKeyPair} key_pair  - Issuer XfrKeyPair.
/// and types of traced assets.
/// @param {string} code - base64 string representing the token code of the asset to be issued.
/// @param {BigInt} seq_num - Issuance sequence number. Every subsequent issuance of a given asset type must have a higher sequence number than before.
/// @param {BigInt} amount - Amount to be issued.
/// @param {boolean} conf_amount - `true` means the asset amount is confidential, and `false` means it's nonconfidential.
/// @param {PublicParams} zei_params - Public parameters necessary to generate asset records.
pub unsafe extern "system" fn Java_com_findora_JniApi_transactionBuilderAddBasicIssueAsset(
    env: JNIEnv,
    _: JClass,
    builder: jlong,
    key_pair: jlong,
    code: JString,
    seq_num: jint,
    amount: jint,
    conf_amount: jboolean,
    zei_params: jlong,
) -> jlong {
    let builder = &*(builder as *mut TransactionBuilder);
    let key_pair = &*(key_pair as *mut XfrKeyPair);
    let code: String = env
        .get_string(code)
        .expect("Couldn't get java string!")
        .into();
    let zei_params = &*(zei_params as *mut PublicParams);
    let builder = builder
        .clone()
        .add_basic_issue_asset(
            key_pair,
            code,
            seq_num as u64,
            amount as u64,
            conf_amount == JNI_TRUE,
            zei_params,
        )
        .unwrap();
    Box::into_raw(Box::new(builder)) as jlong
}

#[no_mangle]
/// Adds an operation to the transaction builder that appends a credential commitment to the address
/// identity registry.
/// @param {XfrKeyPair} key_pair - Ledger key that is tied to the credential.
/// @param {CredUserPublicKey} user_public_key - Public key of the credential user.
/// @param {CredIssuerPublicKey} issuer_public_key - Public key of the credential issuer.
/// @param {CredentialCommitment} commitment - Credential commitment to add to the address identity registry.
/// @param {CredPoK} pok- Proof that the credential commitment is valid.
/// @see {@link module:Findora-Wasm.wasm_credential_commit|wasm_credential_commit} for information about how to generate a credential
/// commitment.
pub unsafe extern "system" fn Java_com_findora_JniApi_transactionBuilderAddOperationAirAssign(
    _env: JNIEnv,
    _: JClass,
    builder: jlong,
    key_pair: jlong,
    user_public_key: jlong,
    issuer_public_key: jlong,
    commitment: jlong,
    pok: jlong,
) -> jlong {
    let builder = &*(builder as *mut TransactionBuilder);
    let key_pair = &*(key_pair as *mut XfrKeyPair);
    let user_public_key = &*(user_public_key as *mut CredUserPublicKey);
    let issuer_public_key = &*(issuer_public_key as *mut CredIssuerPublicKey);
    let commitment = &*(commitment as *mut CredentialCommitment);
    let pok = &*(pok as *mut CredentialPoK);
    let builder = builder
        .clone()
        .add_operation_air_assign(
            key_pair,
            user_public_key,
            issuer_public_key,
            commitment,
            pok,
        )
        .unwrap();
    Box::into_raw(Box::new(builder)) as jlong
}

#[no_mangle]
/// Adds an operation to the transaction builder that removes a hash from ledger's custom data
/// store.
/// @param {XfrKeyPair} auth_key_pair - Key pair that is authorized to delete the hash at the
/// provided key.
/// @param {Key} key - The key of the custom data store whose value will be cleared if the
/// transaction validates.
/// @param {BigInt} seq_num - Nonce to prevent replays.
pub unsafe extern "system" fn Java_com_findora_JniApi_transactionBuilderAddOperationKvUpdateNoHash(
    _env: JNIEnv,
    _: JClass,
    builder: jlong,
    auth_key_pair: jlong,
    key: jlong,
    seq_num: u64,
) -> jlong {
    let builder = &*(builder as *mut TransactionBuilder);
    let auth_key_pair = &*(auth_key_pair as *mut XfrKeyPair);
    let key = &*(key as *mut Key);
    let builder = builder
        .clone()
        .add_operation_kv_update_no_hash(auth_key_pair, key, seq_num as u64)
        .unwrap();
    Box::into_raw(Box::new(builder)) as jlong
}

#[no_mangle]
/// Adds an operation to the transaction builder that adds a hash to the ledger's custom data
/// store.
/// @param {XfrKeyPair} auth_key_pair - Key pair that is authorized to add the hash at the
/// provided key.
/// @param {Key} key - The key of the custom data store the value will be added to if the
/// transaction validates.
/// @param {KVHash} hash - The hash to add to the custom data store.
/// @param {BigInt} seq_num - Nonce to prevent replays.
pub unsafe extern "system" fn Java_com_findora_JniApi_transactionBuilderAddOperationKvUpdateWithHash(
    _env: JNIEnv,
    _: JClass,
    builder: jlong,
    auth_key_pair: jlong,
    key: jlong,
    seq_num: jint,
    kv_hash: jlong,
) -> jlong {
    let builder = &*(builder as *mut TransactionBuilder);
    let auth_key_pair = &*(auth_key_pair as *mut XfrKeyPair);
    let key = &*(key as *mut Key);
    let kv_hash = &*(kv_hash as *mut KVHash);
    let builder = builder
        .clone()
        .add_operation_kv_update_with_hash(auth_key_pair, key, seq_num as u64, kv_hash)
        .unwrap();
    Box::into_raw(Box::new(builder)) as jlong
}

#[no_mangle]
/// Adds an operation to the transaction builder that adds a hash to the ledger's custom data
/// store.
/// @param {XfrKeyPair} auth_key_pair - Asset creator key pair.
/// @param {String} code - base64 string representing token code of the asset whose memo will be updated.
/// transaction validates.
/// @param {String} new_memo - The new asset memo.
/// @see {@link module:Findora-Wasm~AssetRules#set_updatable|AssetRules.set_updatable} for more information about how
/// to define an updatable asset.
pub unsafe extern "system" fn Java_com_findora_JniApi_transactionBuilderAddOperationUpdateMemo(
    env: JNIEnv,
    _: JClass,
    builder: jlong,
    auth_key_pair: jlong,
    code: JString,
    new_memo: JString,
) -> jlong {
    let builder = &*(builder as *mut TransactionBuilder);
    let auth_key_pair = &*(auth_key_pair as *mut XfrKeyPair);
    let code: String = env
        .get_string(code)
        .expect("Couldn't get java string!")
        .into();
    let new_memo: String = env
        .get_string(new_memo)
        .expect("Couldn't get java string!")
        .into();
    let builder = builder
        .clone()
        .add_operation_update_memo(auth_key_pair, code, new_memo)
        .unwrap();
    Box::into_raw(Box::new(builder)) as jlong
}

#[no_mangle]
/// Adds a serialized transfer asset operation to a transaction builder instance.
/// @param {string} op - a JSON-serialized transfer operation.
/// @see {@link module:Findora-Wasm~TransferOperationBuilder} for details on constructing a transfer operation.
/// @throws Will throw an error if `op` fails to deserialize.
pub unsafe extern "system" fn Java_com_findora_JniApi_transactionBuilderAddTransferOperation(
    env: JNIEnv,
    _: JClass,
    builder: jlong,
    op: JString,
) -> jlong {
    let builder = &*(builder as *mut TransactionBuilder);
    let op: String = env
        .get_string(op)
        .expect("Couldn't get java string!")
        .into();
    let builder = builder.clone().add_transfer_operation(op).unwrap();
    Box::into_raw(Box::new(builder)) as jlong
}

#[no_mangle]
pub unsafe extern "system" fn Java_com_findora_JniApi_transactionBuilderSign(
    _env: JNIEnv,
    _: JClass,
    builder: jlong,
    kp: jlong,
) -> jlong {
    let builder = &*(builder as *mut TransactionBuilder);
    let kp = &*(kp as *mut XfrKeyPair);
    let builder = builder.clone().sign(kp).unwrap();
    Box::into_raw(Box::new(builder)) as jlong
}

#[no_mangle]
/// Extracts the serialized form of a transaction.
pub unsafe extern "system" fn Java_com_findora_JniApi_transactionBuilderTransaction(
    env: JNIEnv,
    _: JClass,
    builder: jlong,
) -> jstring {
    let builder = &*(builder as *mut TransactionBuilder);
    let output = env
        .new_string(builder.transaction())
        .expect("Couldn't create java string!");
    output.into_inner()
}

#[no_mangle]
/// Calculates transaction handle.
pub unsafe extern "system" fn Java_com_findora_JniApi_transactionBuilderTransactionHandle(
    env: JNIEnv,
    _: JClass,
    builder: jlong,
) -> jstring {
    let builder = &*(builder as *mut TransactionBuilder);
    let output = env
        .new_string(builder.transaction_handle())
        .expect("Couldn't create java string!");
    output.into_inner()
}

#[no_mangle]
/// Fetches a client record from a transaction.
/// @param {number} idx - Record to fetch. Records are added to the transaction builder sequentially.
pub unsafe extern "system" fn Java_com_findora_JniApi_transactionBuilderGetOwnerRecord(
    _env: JNIEnv,
    _: JClass,
    builder: jlong,
    idx: jint,
) -> jlong {
    let builder = &*(builder as *mut TransactionBuilder);
    Box::into_raw(Box::new(builder.get_owner_record(idx as usize))) as jlong
}

#[no_mangle]
/// Fetches an owner memo from a transaction
/// @param {number} idx - Owner memo to fetch. Owner memos are added to the transaction builder sequentially.
pub unsafe extern "system" fn Java_com_findora_JniApi_transactionBuilderGetOwnerMemo(
    _env: JNIEnv,
    _: JClass,
    builder: jlong,
    idx: jint,
) -> jlong {
    let builder = &*(builder as *mut TransactionBuilder);
    Box::into_raw(Box::new(builder.get_owner_memo(idx as usize))) as jlong
}
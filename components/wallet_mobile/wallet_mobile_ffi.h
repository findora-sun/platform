#ifndef wallet_mobile_ffi_h
#define wallet_mobile_ffi_h

/* Warning, this file is autogenerated by cbindgen. Don't modify this manually. */

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * Object representing an asset definition. Used to fetch tracing policies and any other
 * information that may be required to construct a valid transfer or issuance.
 */
typedef struct AssetType AssetType;

typedef struct AuthenticatedKVLookup AuthenticatedKVLookup;

/**
 * This object represents an asset record owned by a ledger key pair.
 * @see {@link module:Findora-Wasm.open_client_asset_record|open_client_asset_record} for information about how to decrypt an encrypted asset
 * record.
 */
typedef struct ClientAssetRecord ClientAssetRecord;

typedef struct FeeInputs FeeInputs;

typedef struct OpenAssetRecord OpenAssetRecord;

/**
 * Asset owner memo. Contains information needed to decrypt an asset record.
 * @see {@link module:Findora-Wasm.ClientAssetRecord|ClientAssetRecord} for more details about asset records.
 */
typedef struct OwnerMemo OwnerMemo;

/**
 * A collection of tracing policies. Use this object when constructing asset transfers to generate
 * the correct tracing proofs for traceable assets.
 */
typedef struct TracingPolicies TracingPolicies;

/**
 * Structure that enables clients to construct complex transfers.
 */
typedef struct TransferOperationBuilder TransferOperationBuilder;

/**
 * Indicates whether the TXO ref is an absolute or relative value.
 */
typedef struct TxoRef TxoRef;

typedef struct XfrKeyPair XfrKeyPair;

typedef struct XfrPublicKey XfrPublicKey;

typedef struct ByteBuffer {
  int64_t len;
  uint8_t *data;
} ByteBuffer;

/**
 * Returns the git commit hash and commit date of the commit this library was built against.
 */
char *findora_ffi_build_id(void);

char *findora_ffi_random_asset_type(void);

/**
 * Generates asset type as a Base64 string from a JSON-serialized JavaScript value.
 */
char *findora_ffi_asset_type_from_value(const char *code);

/**
 * Given a serialized state commitment and transaction, returns true if the transaction correctly
 * hashes up to the state commitment and false otherwise.
 * @param {string} state_commitment - String representing the state commitment.
 * @param {string} authenticated_txn - String representing the transaction.
 * @see {@link module:Network~Network#getTxn|Network.getTxn} for instructions on fetching a transaction from the ledger.
 * @see {@link module:Network~Network#getStateCommitment|Network.getStateCommitment}
 * for instructions on fetching a ledger state commitment.
 * @throws Will throw an error if the state commitment or the transaction fails to deserialize.
 */
bool findora_ffi_verify_authenticated_txn(const char *state_commitment,
                                          const char *authenticated_txn);

struct AuthenticatedKVLookup *findora_ffi_authenticated_kv_lookup_new(void);

/**
 * Given a serialized state commitment and an authenticated custom data result, returns true if the custom data result correctly
 * hashes up to the state commitment and false otherwise.
 * @param {string} state_commitment - String representing the state commitment.
 * @param {JsValue} authenticated_txn - JSON-encoded value representing the authenticated custom
 * data result.
 * @throws Will throw an error if the state commitment or the authenticated result fail to deserialize.
 */
bool findora_ffi_verify_authenticated_custom_data_result(const char *state_commitment,
                                                         const struct AuthenticatedKVLookup *authenticated_res);

uint64_t findora_ffi_calculate_fee(uint64_t ir_numerator,
                                   uint64_t ir_denominator,
                                   uint64_t outstanding_balance);

struct XfrPublicKey *findora_ffi_get_null_pk(void);

char *findora_ffi_create_default_policy_info(void);

char *findora_ffi_create_debt_policy_info(uint64_t ir_numerator,
                                          uint64_t ir_denominator,
                                          const char *fiat_code,
                                          uint64_t loan_amount);

char *findora_ffi_create_debt_memo(uint64_t ir_numerator,
                                   uint64_t ir_denominator,
                                   const char *fiat_code,
                                   uint64_t loan_amount);

/**
 * Generate mnemonic with custom length and language.
 * - @param `wordslen`: acceptable value are one of [ 12, 15, 18, 21, 24 ]
 * - @param `lang`: acceptable value are one of [ "en", "zh", "zh_traditional", "fr", "it", "ko", "sp", "jp" ]
 */
char *findora_ffi_generate_mnemonic_custom(uint8_t words_len,
                                           const char *lang);

char *findora_ffi_decryption_pbkdf2_aes256gcm(char *enc_key_pair, const char *password);

struct ByteBuffer findora_ffi_encryption_pbkdf2_aes256gcm(const char *key_pair,
                                                          const char *password);

/**
 * Constructs a transfer key pair from a hex-encoded string.
 * The encode a key pair, use `keypair_to_str` function.
 */
struct XfrKeyPair *findora_ffi_keypair_from_str(const char *key_pair_str);

/**
 * Returns bech32 encoded representation of an XfrPublicKey.
 */
char *findora_ffi_public_key_to_bech32(const struct XfrPublicKey *key);

/**
 * Extracts the public key as a string from a transfer key pair.
 */
char *findora_ffi_get_pub_key_str(const struct XfrKeyPair *key);

/**
 * Extracts the private key as a string from a transfer key pair.
 */
char *findora_ffi_get_priv_key_str(const struct XfrKeyPair *key);

/**
 * Restore the XfrKeyPair from a mnemonic with a default bip44-path,
 * that is "m/44'/917'/0'/0/0" ("m/44'/coin'/account'/change/address").
 */
struct XfrKeyPair *findora_ffi_restore_keypair_from_mnemonic_default(const char *phrase);

/**
 * Expresses a transfer key pair as a hex-encoded string.
 * To decode the string, use `keypair_from_str` function.
 */
char *findora_ffi_keypair_to_str(const struct XfrKeyPair *key_pair);

struct XfrKeyPair *findora_ffi_create_keypair_from_secret(const char *sk_str);

struct XfrPublicKey *findora_ffi_get_pk_from_keypair(const struct XfrKeyPair *key_pair);

/**
 * Creates a new transfer key pair.
 */
struct XfrKeyPair *findora_ffi_new_keypair(void);

char *findora_ffi_bech32_to_base64(const char *pk);

char *findora_ffi_base64_to_bech32(const char *pk);

/**
 * Builds an asset type from a JSON-encoded JavaScript value.
 */
struct AssetType *findora_ffi_asset_type_from_json(const char *asset_type_json);

/**
 * Fetch the tracing policies associated with this asset type.
 */
struct TracingPolicies *findora_ffi_asset_type_get_tracing_policies(const struct AssetType *asset_type);

/**
 * Converts a base64 encoded public key string to a public key.
 */
struct XfrPublicKey *findora_ffi_public_key_from_base64(const char *pk);

/**
 * Creates a relative txo reference as a JSON string. Relative txo references are offset
 * backwards from the operation they appear in -- 0 is the most recent, (n-1) is the first output
 * of the transaction.
 *
 * Use relative txo indexing when referring to outputs of intermediate operations (e.g. a
 * transaction containing both an issuance and a transfer).
 *
 * # Arguments
 * @param {BigInt} idx -  Relative TXO (transaction output) SID.
 */
struct TxoRef *findora_ffi_txo_ref_relative(uint64_t idx);

/**
 * Creates an absolute transaction reference as a JSON string.
 *
 * Use absolute txo indexing when referring to an output that has been assigned a utxo index (i.e.
 * when the utxo has been committed to the ledger in an earlier transaction).
 *
 * # Arguments
 * @param {BigInt} idx -  Txo (transaction output) SID.
 */
struct TxoRef *findora_ffi_txo_ref_absolute(uint64_t idx);

/**
 * Returns a object containing decrypted owner record information,
 * where `amount` is the decrypted asset amount, and `asset_type` is the decrypted asset type code.
 *
 * @param {ClientAssetRecord} record - Owner record.
 * @param {OwnerMemo} owner_memo - Owner memo of the associated record.
 * @param {XfrKeyPair} keypair - Keypair of asset owner.
 * @see {@link module:Findora-Wasm~ClientAssetRecord#from_json_record|ClientAssetRecord.from_json_record} for information about how to construct an asset record object
 * from a JSON result returned from the ledger server.
 */
struct OpenAssetRecord *findora_ffi_open_client_asset_record(const struct ClientAssetRecord *record,
                                                             const struct OwnerMemo *owner_memo,
                                                             const struct XfrKeyPair *keypair);

/**
 * pub enum AssetRecordType {
 *     NonConfidentialAmount_ConfidentialAssetType = 0,
 *     ConfidentialAmount_NonConfidentialAssetType = 1,
 *     ConfidentialAmount_ConfidentialAssetType = 2,
 *     NonConfidentialAmount_NonConfidentialAssetType = 3,
 * }
 */
int32_t findora_ffi_open_client_asset_record_get_record_type(const struct OpenAssetRecord *record);

char *findora_ffi_open_client_asset_record_get_asset_type(const struct OpenAssetRecord *record);

uint64_t findora_ffi_open_client_asset_record_get_amount(const struct OpenAssetRecord *record);

struct XfrPublicKey *findora_ffi_open_client_asset_record_get_pub_key(const struct OpenAssetRecord *record);

/**
 * Builds a client record from a JSON-encoded JavaScript value.
 *
 * @param {JsValue} val - JSON-encoded autehtnicated asset record fetched from ledger server with the `utxo_sid/{sid}` route,
 * where `sid` can be fetched from the query server with the `get_owned_utxos/{address}` route.
 * Note: The first field of an asset record is `utxo`. See the example below.
 *
 * @example
 * "utxo":{
 *   "amount":{
 *     "NonConfidential":5
 *   },
 *  "asset_type":{
 *     "NonConfidential":[113,168,158,149,55,64,18,189,88,156,133,204,156,46,106,46,232,62,69,233,157,112,240,132,164,120,4,110,14,247,109,127]
 *   },
 *   "public_key":"Glf8dKF6jAPYHzR_PYYYfzaWqpYcMvnrIcazxsilmlA="
 * }
 *
 * @see {@link module:Findora-Network~Network#getUtxo|Network.getUtxo} for information about how to
 * fetch an asset record from the ledger server.
 */
struct ClientAssetRecord *findora_ffi_client_asset_record_from_json(const char *val);

/**
 * Builds an owner memo from a JSON-serialized JavaScript value.
 * @param {JsValue} val - JSON owner memo fetched from query server with the `get_owner_memo/{sid}` route,
 * where `sid` can be fetched from the query server with the `get_owned_utxos/{address}` route. See the example below.
 *
 * @example
 * {
 *   "blind_share":[91,251,44,28,7,221,67,155,175,213,25,183,70,90,119,232,212,238,226,142,159,200,54,19,60,115,38,221,248,202,74,248],
 *   "lock":{"ciphertext":[119,54,117,136,125,133,112,193],"encoded_rand":"8KDql2JphPB5WLd7-aYE1bxTQAcweFSmrqymLvPDntM="}
 * }
 */
struct OwnerMemo *findora_ffi_owner_memo_from_json(const char *val);

/**
 * Fee smaller than this value will be denied.
 */
uint64_t findora_ffi_fra_get_minimal_fee(void);

/**
 * The destination for fee to be transfered to.
 */
struct XfrPublicKey *findora_ffi_fra_get_dest_pubkey(void);

struct FeeInputs *findora_ffi_fee_inputs_new(void);

void findora_ffi_fee_inputs_append(struct FeeInputs *ptr,
                                   uint64_t am,
                                   const struct TxoRef *tr,
                                   const struct ClientAssetRecord *ar,
                                   const struct OwnerMemo *om,
                                   const struct XfrKeyPair *kp);

void findora_ffi_authenticated_kv_lookup_free(struct AuthenticatedKVLookup *ptr);

void findora_ffi_xfr_public_key_free(struct XfrPublicKey *ptr);

void findora_ffi_fee_inputs_free(struct FeeInputs *ptr);

/**
 * Create a new transfer operation builder.
 */
struct TransferOperationBuilder *findora_ffi_transfer_operation_builder_new(void);

/**
 * Debug function that does not need to go into the docs.
 */
char *findora_ffi_transfer_operation_builder_debug(const struct TransferOperationBuilder *builder);

/**
 * Wraps around TransferOperationBuilder to add an input to a transfer operation builder.
 */
struct TransferOperationBuilder *findora_ffi_transfer_operation_builder_add_input_with_tracing(const struct TransferOperationBuilder *builder,
                                                                                               const struct TxoRef *txo_ref,
                                                                                               const struct ClientAssetRecord *asset_record,
                                                                                               const struct OwnerMemo *owner_memo,
                                                                                               const struct TracingPolicies *tracing_policies,
                                                                                               const struct XfrKeyPair *key,
                                                                                               uint64_t amount);

/**
 * Wraps around TransferOperationBuilder to add an input to a transfer operation builder.
 */
struct TransferOperationBuilder *findora_ffi_transfer_operation_builder_add_input_no_tracing(const struct TransferOperationBuilder *builder,
                                                                                             const struct TxoRef *txo_ref,
                                                                                             const struct ClientAssetRecord *asset_record,
                                                                                             const struct OwnerMemo *owner_memo,
                                                                                             const struct XfrKeyPair *key,
                                                                                             uint64_t amount);

/**
 * Wraps around TransferOperationBuilder to add an output to a transfer operation builder.
 */
struct TransferOperationBuilder *findora_ffi_transfer_operation_builder_add_output_with_tracing(const struct TransferOperationBuilder *builder,
                                                                                                uint64_t amount,
                                                                                                const struct XfrPublicKey *recipient,
                                                                                                const struct TracingPolicies *tracing_policies,
                                                                                                const char *code,
                                                                                                bool conf_amount,
                                                                                                bool conf_type);

/**
 * Wraps around TransferOperationBuilder to add an output to a transfer operation builder.
 */
struct TransferOperationBuilder *findora_ffi_transfer_operation_builder_add_output_no_tracing(const struct TransferOperationBuilder *builder,
                                                                                              uint64_t amount,
                                                                                              const struct XfrPublicKey *recipient,
                                                                                              const char *code,
                                                                                              bool conf_amount,
                                                                                              bool conf_type);

/**
 * Wraps around TransferOperationBuilder to ensure the transfer inputs and outputs are balanced.
 * This function will add change outputs for all unspent portions of input records.
 * @throws Will throw an error if the transaction cannot be balanced.
 */
struct TransferOperationBuilder *findora_ffi_transfer_operation_builder_balance(const struct TransferOperationBuilder *builder);

/**
 * Wraps around TransferOperationBuilder to finalize the transaction.
 */
struct TransferOperationBuilder *findora_ffi_transfer_operation_builder_create(const struct TransferOperationBuilder *builder);

/**
 * Wraps around TransferOperationBuilder to add a signature to the operation.
 *
 * All input owners must sign.
 */
struct TransferOperationBuilder *findora_ffi_transfer_operation_builder_sign(const struct TransferOperationBuilder *builder,
                                                                             const struct XfrKeyPair *kp);

/**
 * Co-sign an input index
 */
struct TransferOperationBuilder *findora_ffi_transfer_operation_builder_add_cosignature(const struct TransferOperationBuilder *builder,
                                                                                        const struct XfrKeyPair *kp,
                                                                                        uintptr_t input_idx);

char *findora_ffi_transfer_operation_builder_builder(const struct TransferOperationBuilder *builder);

/**
 * Wraps around TransferOperationBuilder to extract an operation expression as JSON.
 */
char *findora_ffi_transfer_operation_builder_transaction(const struct TransferOperationBuilder *builder);

#endif /* wallet_mobile_ffi_h */

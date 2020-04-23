#![deny(warnings)]
use bulletproofs::r1cs::R1CSProof;
use bulletproofs::PedersenGens;
use curve25519_dalek::ristretto::CompressedRistretto;
use curve25519_dalek::scalar::Scalar;
use ledger::data_model::errors::PlatformError;
use ledger::data_model::AssetTypeCode;
use ledger::error_location;
use linear_map::LinearMap;
use rand_chacha::ChaChaRng;
use rand_core::SeedableRng;
use serde::{Deserialize, Serialize};
use zei::crypto::solvency::{prove_solvency, verify_solvency};
use zei::xfr::structs::asset_type_to_scalar;

pub type AssetAmountAndCode = (Scalar, Scalar);
pub type AssetCodeAndRate = (Scalar, Scalar);
pub type AssetCommitment = (CompressedRistretto, CompressedRistretto);
pub type LiabilityCommitment = (CompressedRistretto, CompressedRistretto);

/// Asset and liability information, and associated solvency proof if exists
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AssetAndLiabilityAccount {
  /// Amount and code of the public assets
  pub public_assets: Vec<AssetAmountAndCode>,

  /// Amount and code of the hidden assets
  pub hidden_assets: Vec<AssetAmountAndCode>,

  /// Commitments to hidden assets, null iff any of the following:
  /// * Solvency hasn't been proved
  /// * Assets or liabilities have been updated
  pub hidden_assets_commitments: Option<Vec<AssetCommitment>>,

  /// Amount and code of the public liabilities
  pub public_liabilities: Vec<AssetAmountAndCode>,

  /// Amount and code of the hidden liabilities
  pub hidden_liabilities: Vec<AssetAmountAndCode>,

  /// Commitments to hidden liabilities, null iff any of the following:
  /// * Solvency hasn't been proved
  /// * Assets or liabilities have been updated
  pub hidden_liabilities_commitments: Option<Vec<LiabilityCommitment>>,

  /// Serialized solvency proof, null iff any of the following:
  /// * Solvency hasn't been proved
  /// * Assets or liabilities have been updated
  pub proof: Option<Vec<u8>>,
}

impl AssetAndLiabilityAccount {
  /// Sets the commitments to hidden assets and liabilities, and the solvency proof to null.
  /// Used when the asset or liabilities are updated.
  fn remove_commitments_and_proof(&mut self) {
    self.hidden_assets_commitments = None;
    self.hidden_liabilities_commitments = None;
    self.proof = None;
  }

  /// Adds the commitments to hidden assets and liabilities, and the solvency proof.
  /// Used when the the solvency is proved.
  pub fn add_commitments_and_proof(&mut self,
                                   hidden_assets_commitments: Vec<AssetCommitment>,
                                   hidden_liabilities_commitments: Vec<LiabilityCommitment>,
                                   proof: R1CSProof) {
    self.hidden_assets_commitments = Some(hidden_assets_commitments);
    self.hidden_liabilities_commitments = Some(hidden_liabilities_commitments);
    self.proof = Some(proof.to_bytes());
  }

  /// Adds a public asset and remove the solvency proof.
  pub fn add_public_asset(&mut self, amount: u64, code: Scalar) {
    self.public_assets.push((Scalar::from(amount), code));
    self.remove_commitments_and_proof();
  }

  /// Adds a hidden asset and remove the solvency proof.
  pub fn add_hidden_asset(&mut self, amount: u64, code: Scalar) {
    self.hidden_assets.push((Scalar::from(amount), code));
    self.remove_commitments_and_proof();
  }

  /// Adds a public liability and remove the solvency proof.
  pub fn add_public_liability(&mut self, amount: u64, code: Scalar) {
    self.public_liabilities.push((Scalar::from(amount), code));
    self.remove_commitments_and_proof();
  }

  /// Adds a hidden liability and remove the solvency proof.
  pub fn add_hidden_liability(&mut self, amount: u64, code: Scalar) {
    self.hidden_liabilities.push((Scalar::from(amount), code));
    self.remove_commitments_and_proof();
  }
}

/// Used to audit the solvency.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SolvencyAudit {
  /// Table mapping each asset code to its conversion rate.
  pub conversion_rates: Vec<AssetCodeAndRate>,
}

impl SolvencyAudit {
  /// Sets conversion rate for the asset.
  pub fn set_rate(&mut self, code: AssetTypeCode, rate: u64) {
    self.conversion_rates
        .push((asset_type_to_scalar(&code.val), Scalar::from(rate)));
  }

  /// Geneartes a new asset and sets its conversion rate.
  /// Returns the generated asset code.
  pub fn set_asset_and_rate(&mut self, rate: u64) -> Scalar {
    let code = asset_type_to_scalar(&AssetTypeCode::gen_random().val);
    self.conversion_rates.push((code, Scalar::from(rate)));
    code
  }

  /// Proves the solvency and stores the commitments and proof.
  /// Must be used before `verify_solvency`.
  pub fn prove_solvency_and_store(&self,
                                  account: &mut AssetAndLiabilityAccount)
                                  -> Result<(), PlatformError> {
    // Prove the solvency
    let mut prng = ChaChaRng::from_seed([0u8; 32]);
    let hidden_assets_size = account.hidden_assets.len();
    let hidden_liabilities_size = account.hidden_liabilities.len();
    let assets_hiddens =
      vec![(Scalar::random(&mut prng), Scalar::random(&mut prng)); hidden_assets_size];
    let liabilities_hiddens =
      vec![(Scalar::random(&mut prng), Scalar::random(&mut prng)); hidden_liabilities_size];
    let mut rates = LinearMap::new();
    for (code, rate) in self.conversion_rates.clone() {
      rates.insert(code, rate);
    }
    let proof =
      prove_solvency(&account.hidden_assets,
                     &assets_hiddens,
                     &account.public_assets,
                     &account.hidden_liabilities,
                     &liabilities_hiddens,
                     &account.public_liabilities,
                     &rates).or_else(|e| Err(PlatformError::ZeiError(error_location!(), e)))?;

    // Commit the hidden assets and liabilities
    let pc_gens = PedersenGens::default();
    let hidden_assets_commitments: Vec<AssetCommitment> =
      account.hidden_assets
             .iter()
             .zip(assets_hiddens.iter())
             .map(|((a, t), (ba, bt))| {
               (pc_gens.commit(*a, *ba).compress(), pc_gens.commit(*t, *bt).compress())
             })
             .collect();
    let hidden_liabilities_commitments: Vec<LiabilityCommitment> =
      account.hidden_liabilities
             .iter()
             .zip(liabilities_hiddens.iter())
             .map(|((a, t), (ba, bt))| {
               (pc_gens.commit(*a, *ba).compress(), pc_gens.commit(*t, *bt).compress())
             })
             .collect();

    // Update data
    account.add_commitments_and_proof(hidden_assets_commitments,
                                      hidden_liabilities_commitments,
                                      proof);
    Ok(())
  }

  /// Verifies the solvency proof.
  /// Must not be used before `prove_solvency_and_store`.
  pub fn verify_solvency(&self, account: &AssetAndLiabilityAccount) -> Result<(), PlatformError> {
    let hidden_assets_commitments = if let Some(commitments) = &account.hidden_assets_commitments {
      commitments
    } else {
      println!("Missing commitments to the hidden assets. Prove the solvency first.");
      return Err(PlatformError::InputsError(error_location!()));
    };
    let hidden_liabilities_commitments =
      if let Some(commitments) = &account.hidden_liabilities_commitments {
        commitments
      } else {
        println!("Missing commitments to the hidden liabilities. Prove the solvency first.");
        return Err(PlatformError::InputsError(error_location!()));
      };
    let proof = if let Some(p) = &account.proof {
      R1CSProof::from_bytes(p).or(Err(PlatformError::DeserializationError))?
    } else {
      println!("Prove the solvency first.");
      return Err(PlatformError::InputsError(error_location!()));
    };
    let mut rates = LinearMap::new();
    for (code, rate) in self.conversion_rates.clone() {
      rates.insert(code, rate);
    }
    verify_solvency(hidden_assets_commitments,
                    &account.public_assets,
                    hidden_liabilities_commitments,
                    &account.public_liabilities,
                    &rates,
                    &proof).or_else(|e| Err(PlatformError::ZeiError(error_location!(), e)))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use zei::errors::ZeiError;

  // Add three public assets
  fn add_public_assets(account: &mut AssetAndLiabilityAccount, codes: (Scalar, Scalar, Scalar)) {
    account.add_public_asset(100, codes.0);
    account.add_public_asset(200, codes.1);
    account.add_public_asset(300, codes.2);
  }

  // Add three hidden assets
  fn add_hidden_assets(account: &mut AssetAndLiabilityAccount, codes: (Scalar, Scalar, Scalar)) {
    account.add_hidden_asset(10, codes.0);
    account.add_hidden_asset(20, codes.1);
    account.add_hidden_asset(30, codes.2);
  }

  // Add three public liabilities
  fn add_public_liabilities(account: &mut AssetAndLiabilityAccount,
                            codes: (Scalar, Scalar, Scalar)) {
    account.add_public_asset(100, codes.0);
    account.add_public_asset(200, codes.1);
    account.add_public_asset(200, codes.2);
  }

  // Add three hidden liabilities, with total value smaller than hidden assets'
  fn add_hidden_liabilities_smaller(account: &mut AssetAndLiabilityAccount,
                                    codes: (Scalar, Scalar, Scalar)) {
    account.add_hidden_liability(10, codes.0);
    account.add_hidden_liability(20, codes.1);
    account.add_hidden_liability(20, codes.2);
  }

  // Add three hidden liabilities, with total value larger than hidden assets'
  fn add_hidden_liabilities_larger(account: &mut AssetAndLiabilityAccount,
                                   codes: (Scalar, Scalar, Scalar)) {
    account.add_hidden_liability(10, codes.0);
    account.add_hidden_liability(20, codes.1);
    account.add_hidden_liability(40, codes.2);
  }

  // Add asset conversion rates for all related assets
  fn add_conversion_rate_complete(audit: &mut SolvencyAudit) -> (Scalar, Scalar, Scalar) {
    (audit.set_asset_and_rate(1), audit.set_asset_and_rate(2), audit.set_asset_and_rate(3))
  }

  // Add asset conversion rates with one missing asset
  fn add_conversion_rate_incomplete(audit: &mut SolvencyAudit) -> (Scalar, Scalar) {
    (audit.set_asset_and_rate(1), audit.set_asset_and_rate(2))
  }

  #[test]
  fn test_prove_solvency_fail() {
    // Start a solvency audit process
    let mut audit = SolvencyAudit::default();

    // Set asset conversion rates, but miss one asset
    let (codes_0, codes_1) = add_conversion_rate_incomplete(&mut audit);
    let code_2 = asset_type_to_scalar(&AssetTypeCode::gen_random().val);

    // Create an asset and liability account
    let mut account = &mut AssetAndLiabilityAccount::default();

    // Adds hidden assets and liabilities
    add_hidden_assets(&mut account, (codes_0, codes_1, code_2));
    add_hidden_liabilities_smaller(&mut account, (codes_0, codes_1, code_2));

    // Prove the solvency
    // Should fail with ZeiError::SolvencyProveError
    match audit.prove_solvency_and_store(&mut account) {
      Err(PlatformError::ZeiError(_, ZeiError::SolvencyProveError)) => {}
      unexpected_result => {
        panic!(format!("Expected ZeiError::SolvencyVerificationError, found {:?}.",
                       unexpected_result));
      }
    }
  }

  #[test]
  fn test_verify_solvency_fail() {
    // Start a solvency audit process and set the asset conversion rates
    let mut audit = SolvencyAudit::default();
    let codes = add_conversion_rate_complete(&mut audit);

    // Create a asset and liability account
    let mut account = &mut AssetAndLiabilityAccount::default();

    // Adds hidden assets
    add_hidden_assets(&mut account, codes);

    // Adds hidden liabilities, with total value larger than hidden assets'
    add_hidden_liabilities_larger(&mut account, codes);

    // Verify the solvency without a proof
    // Should fail with InputsError
    match audit.verify_solvency(&account) {
      Err(PlatformError::InputsError(_)) => {}
      unexpected_result => {
        panic!(format!("Expected InputsError, found {:?}.", unexpected_result));
      }
    }
  }

  #[test]
  fn test_prove_and_verify_solvency_fail() {
    // Start a solvency audit process and set the asset conversion rates
    let mut audit = SolvencyAudit::default();
    let codes = add_conversion_rate_complete(&mut audit);

    // Create a asset and liability account
    let mut account = &mut AssetAndLiabilityAccount::default();

    // Adds hidden assets
    add_hidden_assets(&mut account, codes);

    // Adds hidden liabilities, with total value larger than hidden assets'
    add_hidden_liabilities_larger(&mut account, codes);

    // Prove the solvency
    audit.prove_solvency_and_store(&mut account).unwrap();
    assert!(account.hidden_assets_commitments.is_some());
    assert!(account.hidden_liabilities_commitments.is_some());
    assert!(account.proof.is_some());

    // Verify the solvency proof
    // Should fail with ZeiError::SolvencyVerificationError
    match audit.verify_solvency(&account) {
      Err(PlatformError::ZeiError(_, ZeiError::SolvencyVerificationError)) => {}
      unexpected_result => {
        panic!(format!("Expected ZeiError::SolvencyVerificationError, found {:?}.",
                       unexpected_result));
      }
    }
  }

  #[test]
  fn test_prove_and_verify_solvency_pass() {
    // Start a solvency audit process and set the asset conversion rates
    let mut audit = SolvencyAudit::default();
    let codes = add_conversion_rate_complete(&mut audit);

    // Create an account and add assets and liabilities
    let mut account = &mut AssetAndLiabilityAccount::default();
    add_public_assets(&mut account, codes);
    add_hidden_assets(&mut account, codes);
    add_public_liabilities(&mut account, codes);
    add_hidden_liabilities_smaller(&mut account, codes);

    // Prove the solvency and verify the commitments and proof are stored
    audit.prove_solvency_and_store(&mut account).unwrap();
    assert!(account.hidden_assets_commitments.is_some());
    assert!(account.hidden_liabilities_commitments.is_some());
    assert!(account.proof.is_some());

    // Verify the solvency proof
    audit.verify_solvency(&account).unwrap();
  }

  #[test]
  fn test_update_asset_and_verify_solvency_mixed() {
    // Start a solvency audit process and set the asset conversion rates
    let mut audit = SolvencyAudit::default();
    let codes = add_conversion_rate_complete(&mut audit);

    // Create an account and add assets and liabilities
    let mut account = &mut AssetAndLiabilityAccount::default();
    add_public_assets(&mut account, codes);
    add_hidden_assets(&mut account, codes);
    add_public_liabilities(&mut account, codes);
    add_hidden_liabilities_smaller(&mut account, codes);

    // Prove and verify the solvency
    audit.prove_solvency_and_store(&mut account).unwrap();
    audit.verify_solvency(&account).unwrap();

    // Update the public assets and verify the commitments and proof are removed
    account.add_public_asset(40, codes.0);
    assert!(account.hidden_assets_commitments.is_none());
    assert!(account.hidden_liabilities_commitments.is_none());
    assert!(account.proof.is_none());

    // Verify the solvency without proving it again
    // Should fail with InputsError
    match audit.verify_solvency(&account) {
      Err(PlatformError::InputsError(_)) => {}
      unexpected_result => {
        panic!(format!("Expected InputsError, found {:?}.", unexpected_result));
      }
    }

    // Prove the solvency again and verify the proof
    audit.prove_solvency_and_store(&mut account).unwrap();
    audit.verify_solvency(&account).unwrap();
  }

  #[test]
  fn test_update_liability_and_verify_solvency_fail() {
    // Start a solvency audit process and set the asset conversion rates
    let mut audit = SolvencyAudit::default();
    let codes = add_conversion_rate_complete(&mut audit);

    // Create an account and add assets and liabilities
    let mut account = &mut AssetAndLiabilityAccount::default();
    add_public_assets(&mut account, codes);
    add_hidden_assets(&mut account, codes);
    add_public_liabilities(&mut account, codes);
    add_hidden_liabilities_smaller(&mut account, codes);

    // Prove and verify the solvency
    audit.prove_solvency_and_store(&mut account).unwrap();
    audit.verify_solvency(&account).unwrap();

    // Update the hidden liability
    // to make the liabilities' total value greater than assets'
    account.add_hidden_liability(4000, codes.0);

    // Prove the solvency again
    audit.prove_solvency_and_store(&mut account).unwrap();

    // Verify the solvency proof
    // Should fail with SolvencyVerificationError
    match audit.verify_solvency(&account) {
      Err(PlatformError::ZeiError(_, ZeiError::SolvencyVerificationError)) => {}
      unexpected_result => {
        panic!(format!("Expected ZeiError::SolvencyVerificationError, found {:?}.",
                       unexpected_result));
      }
    }
  }
}
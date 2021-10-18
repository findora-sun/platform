use crate::storage::*;
use crate::{App, Config};
use ethereum_types::{H160, H256, U256};
use fp_core::context::Context;
use fp_evm::Account;
use fp_storage::{Borrow, BorrowMut};
use fp_traits::{
    account::AccountAsset,
    evm::{AddressMapping, OnChargeEVMTransaction},
};
use fp_utils::proposer_converter;
use ruc::Result;

impl<C: Config> App<C> {
    /// Check whether an account is empty.
    pub fn is_account_empty(ctx: &Context, address: &H160) -> bool {
        let account = Self::account_basic(ctx, address);
        let code_len =
            AccountCodes::decode_len(ctx.state.read().borrow(), address).unwrap_or(0);

        account.nonce == U256::zero() && account.balance == U256::zero() && code_len == 0
    }

    /// Remove an account.
    pub fn remove_account(ctx: &Context, address: &H160) {
        AccountCodes::remove(ctx.state.write().borrow_mut(), address);
        AccountStorages::remove_prefix(ctx.state.write().borrow_mut(), address);
    }

    /// Create an account.
    pub fn create_account(ctx: &Context, address: H160, code: Vec<u8>) -> Result<()> {
        if code.is_empty() {
            return Ok(());
        }

        AccountCodes::insert(ctx.state.write().borrow_mut(), &address, &code)
    }

    /// Get the account code
    pub fn account_codes(
        ctx: &Context,
        address: &H160,
        height: Option<u64>,
    ) -> Option<Vec<u8>> {
        match height {
            Some(ver) => AccountCodes::get_ver(ctx.state.read().borrow(), address, ver),
            None => AccountCodes::get(ctx.state.read().borrow(), address),
        }
    }

    /// Get the account storage
    pub fn account_storages(
        ctx: &Context,
        address: &H160,
        index: &H256,
        height: Option<u64>,
    ) -> Option<H256> {
        match height {
            Some(ver) => {
                AccountStorages::get_ver(ctx.state.read().borrow(), address, index, ver)
            }
            None => AccountStorages::get(ctx.state.read().borrow(), address, index),
        }
    }

    /// Get the account basic in EVM format.
    pub fn account_basic(ctx: &Context, address: &H160) -> Account {
        let account_id = C::AddressMapping::convert_to_account_id(*address);
        let nonce = C::AccountAsset::nonce(ctx, &account_id);
        let balance = C::AccountAsset::balance(ctx, &account_id);

        Account { balance, nonce }
    }

    /// Get the block proposer.
    pub fn find_proposer(ctx: &Context) -> H160 {
        // TODO
        proposer_converter(ctx.header.proposer_address.clone()).unwrap_or_default()
    }
}

/// Implements the transaction payment for a module implementing the `Currency`
/// trait (eg. the pallet_balances) using an unbalance handler (implementing
/// `OnUnbalanced`).
impl<C: Config> OnChargeEVMTransaction for App<C> {
    fn withdraw_fee(ctx: &Context, who: &H160, fee: U256) -> Result<()> {
        // TODO fee pay to block author
        let account_id = C::AddressMapping::convert_to_account_id(*who);
        C::AccountAsset::withdraw(ctx, &account_id, fee)
    }

    fn correct_and_deposit_fee(
        ctx: &Context,
        who: &H160,
        corrected_fee: U256,
        already_withdrawn: U256,
    ) -> Result<()> {
        let account_id = C::AddressMapping::convert_to_account_id(*who);
        C::AccountAsset::refund(ctx, &account_id, already_withdrawn)?;
        C::AccountAsset::burn(ctx, &account_id, corrected_fee)
    }
}
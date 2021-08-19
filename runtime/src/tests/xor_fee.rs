// This file is part of the SORA network and Polkaswap app.

// Copyright (c) 2020, 2021, Polka Biome Ltd. All rights reserved.
// SPDX-License-Identifier: BSD-4-Clause

// Redistribution and use in source and binary forms, with or without modification,
// are permitted provided that the following conditions are met:

// Redistributions of source code must retain the above copyright notice, this list
// of conditions and the following disclaimer.
// Redistributions in binary form must reproduce the above copyright notice, this
// list of conditions and the following disclaimer in the documentation and/or other
// materials provided with the distribution.
//
// All advertising materials mentioning features or use of this software must display
// the following acknowledgement: This product includes software developed by Polka Biome
// Ltd., SORA, and Polkaswap.
//
// Neither the name of the Polka Biome Ltd. nor the names of its contributors may be used
// to endorse or promote products derived from this software without specific prior written permission.

// THIS SOFTWARE IS PROVIDED BY Polka Biome Ltd. AS IS AND ANY EXPRESS OR IMPLIED WARRANTIES,
// INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
// A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL Polka Biome Ltd. BE LIABLE FOR ANY
// DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
// BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;
// OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
// STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
// USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use crate::mock::{ensure_pool_initialized, fill_spot_price};
use crate::{
    AccountId, AssetId, Assets, Balance, Balances, Call, Currencies, GetXorFeeAccountId, Origin,
    PoolXYK, ReferralSystem, ReferrerWeight, Runtime, SoraParliamentShare, Staking, Tokens, Weight,
    XorBurnedWeight, XorFee, XorIntoValBurnedWeight,
};
use common::mock::{alice, bob, charlie};
use common::prelude::constants::SMALL_FEE;
use common::prelude::{AssetName, AssetSymbol, FixedWrapper, SwapAmount, WeightToFixedFee};
use common::{balance, fixed_wrapper, FilterMode, VAL, XOR};
use frame_support::assert_ok;
use frame_support::dispatch::{DispatchInfo, PostDispatchInfo};
use frame_support::pallet_prelude::{InvalidTransaction, Pays};
use frame_support::traits::OnFinalize;
use frame_support::unsigned::TransactionValidityError;
use frame_support::weights::WeightToFeePolynomial;
use framenode_chain_spec::ext;
use log::LevelFilter;
use pallet_balances::NegativeImbalance;
use pallet_transaction_payment::{ChargeTransactionPayment, OnChargeTransaction};
use referral_system::ReferrerBalances;
use sp_runtime::traits::SignedExtension;
use sp_runtime::AccountId32;
use traits::MultiCurrency;
use xor_fee::{LiquidityInfo, XorToVal};

type BlockWeights = <Runtime as frame_system::Config>::BlockWeights;
type TransactionByteFee = <Runtime as pallet_transaction_payment::Config>::TransactionByteFee;

const MOCK_WEIGHT: Weight = 600_000_000;

const INITIAL_BALANCE: Balance = balance!(1000);
const INITIAL_RESERVES: Balance = balance!(10000);
const TRANSFER_AMOUNT: Balance = balance!(69);

fn sora_parliament_account() -> AccountId {
    AccountId32::from([7; 32])
}

/// create a transaction info struct from weight. Handy to avoid building the whole struct.
fn info_from_weight(w: Weight) -> DispatchInfo {
    // pays_fee: Pays::Yes -- class: DispatchClass::Normal
    DispatchInfo {
        weight: w,
        ..Default::default()
    }
}

fn default_post_info() -> PostDispatchInfo {
    PostDispatchInfo {
        actual_weight: None,
        pays_fee: Default::default(),
    }
}

fn post_info_from_weight(w: Weight) -> PostDispatchInfo {
    PostDispatchInfo {
        actual_weight: Some(w),
        pays_fee: Default::default(),
    }
}

fn post_info_pays_no() -> PostDispatchInfo {
    PostDispatchInfo {
        actual_weight: None,
        pays_fee: Pays::No,
    }
}

fn give_xor_initial_balance(target: AccountId) {
    increase_balance(target, XOR.into(), INITIAL_BALANCE);
}

fn increase_balance(target: AccountId, asset: AssetId, balance: Balance) {
    assert_ok!(Currencies::update_balance(
        Origin::root(),
        target,
        asset,
        balance as i128
    ));
}

#[test]
fn referrer_gets_bonus_from_tx_fee() {
    ext().execute_with(|| {
        give_xor_initial_balance(alice());
        give_xor_initial_balance(charlie());

        let call: &<Runtime as frame_system::Config>::Call =
            &Call::Assets(assets::Call::transfer(VAL.into(), bob(), TRANSFER_AMOUNT));

        let len = 10;
        let dispatch_info = info_from_weight(MOCK_WEIGHT);
        let pre = ChargeTransactionPayment::<Runtime>::from(0u128.into())
            .pre_dispatch(&alice(), call, &dispatch_info, len)
            .unwrap();
        let balance_after_reserving_fee = FixedWrapper::from(INITIAL_BALANCE) - SMALL_FEE;
        let balance_after_reserving_fee = balance_after_reserving_fee.into_balance();
        assert_eq!(Balances::free_balance(alice()), balance_after_reserving_fee);
        assert!(ChargeTransactionPayment::<Runtime>::post_dispatch(
            pre,
            &dispatch_info,
            &default_post_info(),
            len,
            &Ok(())
        )
        .is_ok());
        assert_eq!(Balances::free_balance(alice()), balance_after_reserving_fee);
        let weights_sum: FixedWrapper = FixedWrapper::from(balance!(ReferrerWeight::get()))
            + FixedWrapper::from(balance!(XorBurnedWeight::get()))
            + FixedWrapper::from(balance!(XorIntoValBurnedWeight::get()));
        let referrer_weight = FixedWrapper::from(balance!(ReferrerWeight::get()));
        let initial_balance = FixedWrapper::from(INITIAL_BALANCE);
        let expected_referrer_balance = SMALL_FEE * referrer_weight / weights_sum + initial_balance;
        assert!(
            Balances::free_balance(charlie())
                >= (expected_referrer_balance.clone() - fixed_wrapper!(1)).into_balance()
                && Balances::free_balance(charlie())
                    <= (expected_referrer_balance + fixed_wrapper!(1)).into_balance()
        );
    });
}

#[test]
fn notify_val_burned_works() {
    env_logger::Builder::new()
        .filter_level(LevelFilter::Debug)
        .init();

    ext().execute_with(|| {
        increase_balance(alice(), XOR.into(), INITIAL_RESERVES);

        Staking::on_finalize(0);

        increase_balance(bob(), XOR.into(), 2 * INITIAL_RESERVES);
        increase_balance(bob(), VAL.into(), 2 * INITIAL_RESERVES);

        ensure_pool_initialized(XOR.into(), VAL.into());
        PoolXYK::deposit_liquidity(
            Origin::signed(bob()),
            0,
            XOR.into(),
            VAL.into(),
            INITIAL_RESERVES,
            INITIAL_RESERVES,
            INITIAL_RESERVES,
            INITIAL_RESERVES,
        )
        .unwrap();

        fill_spot_price();

        assert_eq!(
            pallet_staking::Module::<Runtime>::era_val_burned(),
            0_u128.into()
        );

        let mut total_xor_val = 0;
        for _ in 0..3 {
            let call: &<Runtime as frame_system::Config>::Call =
                &Call::Assets(assets::Call::transfer(VAL.into(), bob(), TRANSFER_AMOUNT));

            let len = 10;
            let dispatch_info = info_from_weight(MOCK_WEIGHT);
            let pre = ChargeTransactionPayment::<Runtime>::from(0_u128.into())
                .pre_dispatch(&alice(), call, &dispatch_info, len)
                .unwrap();
            assert!(ChargeTransactionPayment::<Runtime>::post_dispatch(
                pre,
                &dispatch_info,
                &default_post_info(),
                len,
                &Ok(())
            )
            .is_ok());
            let xor_into_val_burned_weight = XorIntoValBurnedWeight::get() as u128;
            let weights_sum = ReferrerWeight::get() as u128
                + XorBurnedWeight::get() as u128
                + xor_into_val_burned_weight;
            let x =
                FixedWrapper::from(SMALL_FEE * xor_into_val_burned_weight as u128 / weights_sum);
            let y = INITIAL_RESERVES;
            let expected_val_burned = x.clone() * y / (x.clone() + y);
            total_xor_val += expected_val_burned.into_balance();
        }

        // The correct answer is 3E-13 away
        assert_eq!(XorToVal::<Runtime>::get(), total_xor_val + 36750000);
        assert_eq!(
            pallet_staking::Module::<Runtime>::era_val_burned(),
            0_u128.into()
        );

        <xor_fee::Module<Runtime> as pallet_session::historical::SessionManager<_, _>>::end_session(
            0,
        );

        // The correct answer is 2E-13 away
        assert_eq!(
            pallet_staking::Module::<Runtime>::era_val_burned(),
            total_xor_val - 3150072839481
        );
    });
}

#[test]
fn custom_fees_work() {
    ext().execute_with(|| {
        give_xor_initial_balance(alice());
        give_xor_initial_balance(bob());

        let len = 10;
        let dispatch_info = info_from_weight(MOCK_WEIGHT);
        let base_fee =
            WeightToFixedFee::calc(&BlockWeights::get().get(dispatch_info.class).base_extrinsic);
        let len_fee = len as u128 * TransactionByteFee::get();
        let weight_fee = WeightToFixedFee::calc(&MOCK_WEIGHT);

        // A ten-fold extrinsic; fee is 0.007 XOR
        let call: &<Runtime as frame_system::Config>::Call = &Call::Assets(assets::Call::register(
            AssetSymbol(b"ALIC".to_vec()),
            AssetName(b"ALICE".to_vec()),
            balance!(0),
            true,
        ));

        let pre = ChargeTransactionPayment::<Runtime>::from(0u128.into())
            .pre_dispatch(&alice(), call, &dispatch_info, len)
            .unwrap();
        let balance_after_fee_withdrawal =
            FixedWrapper::from(INITIAL_BALANCE) - fixed_wrapper!(0.007);
        let balance_after_fee_withdrawal = balance_after_fee_withdrawal.into_balance();
        assert_eq!(
            Balances::free_balance(alice()),
            balance_after_fee_withdrawal
        );
        assert!(ChargeTransactionPayment::<Runtime>::post_dispatch(
            pre,
            &dispatch_info,
            &default_post_info(),
            len,
            &Ok(())
        )
        .is_ok());
        assert_eq!(
            Balances::free_balance(alice()),
            balance_after_fee_withdrawal
        );

        // A normal extrinsic; fee is 0.0007 XOR
        let call: &<Runtime as frame_system::Config>::Call =
            &Call::Assets(assets::Call::mint(XOR, bob(), balance!(1)));

        let pre = ChargeTransactionPayment::<Runtime>::from(0u128.into())
            .pre_dispatch(&alice(), call, &dispatch_info, len)
            .unwrap();
        let balance_after_fee_withdrawal =
            FixedWrapper::from(balance_after_fee_withdrawal) - fixed_wrapper!(0.0007);
        let balance_after_fee_withdrawal = balance_after_fee_withdrawal.into_balance();
        assert_eq!(
            Balances::free_balance(alice()),
            balance_after_fee_withdrawal
        );
        assert!(ChargeTransactionPayment::<Runtime>::post_dispatch(
            pre,
            &dispatch_info,
            &default_post_info(),
            len,
            &Ok(())
        )
        .is_ok());
        assert_eq!(
            Balances::free_balance(alice()),
            balance_after_fee_withdrawal
        );

        // An extrinsic without manual fee adjustment
        let call: &<Runtime as frame_system::Config>::Call =
            &Call::Balances(pallet_balances::Call::transfer(bob(), TRANSFER_AMOUNT));

        let pre = ChargeTransactionPayment::<Runtime>::from(0u128.into())
            .pre_dispatch(&alice(), call, &dispatch_info, len)
            .unwrap();
        let balance_after_fee_withdrawal =
            FixedWrapper::from(balance_after_fee_withdrawal) - base_fee - len_fee - weight_fee;
        let balance_after_fee_withdrawal = balance_after_fee_withdrawal.into_balance();
        assert_eq!(
            Balances::free_balance(alice()),
            balance_after_fee_withdrawal
        );
        assert!(ChargeTransactionPayment::<Runtime>::post_dispatch(
            pre,
            &dispatch_info,
            &default_post_info(),
            len,
            &Ok(())
        )
        .is_ok());
        assert_eq!(
            Balances::free_balance(alice()),
            balance_after_fee_withdrawal
        );
    });
}

#[test]
fn refund_if_pays_no_works() {
    ext().execute_with(|| {
        give_xor_initial_balance(alice());

        let tech_account_id = GetXorFeeAccountId::get();
        assert_eq!(Balances::free_balance(&tech_account_id), 0_u128.into());

        let len = 10;
        let dispatch_info = info_from_weight(MOCK_WEIGHT);

        let call: &<Runtime as frame_system::Config>::Call = &Call::Assets(assets::Call::register(
            AssetSymbol(b"ALIC".to_vec()),
            AssetName(b"ALICE".to_vec()),
            balance!(0),
            true,
        ));

        let pre = ChargeTransactionPayment::<Runtime>::from(0u128.into())
            .pre_dispatch(&alice(), call, &dispatch_info, len)
            .unwrap();
        let balance_after_fee_withdrawal =
            FixedWrapper::from(INITIAL_BALANCE) - fixed_wrapper!(0.007);
        let balance_after_fee_withdrawal = balance_after_fee_withdrawal.into_balance();
        assert_eq!(
            Balances::free_balance(alice()),
            balance_after_fee_withdrawal
        );
        assert!(ChargeTransactionPayment::<Runtime>::post_dispatch(
            pre,
            &dispatch_info,
            &post_info_pays_no(),
            len,
            &Ok(())
        )
        .is_ok());
        assert_eq!(Balances::free_balance(alice()), INITIAL_BALANCE,);
        assert_eq!(Balances::free_balance(tech_account_id), 0_u128.into());
    });
}

#[test]
fn actual_weight_is_ignored_works() {
    ext().execute_with(|| {
        give_xor_initial_balance(alice());

        let len = 10;
        let dispatch_info = info_from_weight(MOCK_WEIGHT);

        let call: &<Runtime as frame_system::Config>::Call =
            &Call::Assets(assets::Call::transfer(XOR.into(), bob(), TRANSFER_AMOUNT));

        let pre = ChargeTransactionPayment::<Runtime>::from(0u128.into())
            .pre_dispatch(&alice(), call, &dispatch_info, len)
            .unwrap();
        let balance_after_fee_withdrawal = FixedWrapper::from(INITIAL_BALANCE) - SMALL_FEE;
        let balance_after_fee_withdrawal = balance_after_fee_withdrawal.into_balance();
        assert_eq!(
            Balances::free_balance(alice()),
            balance_after_fee_withdrawal
        );
        assert!(ChargeTransactionPayment::<Runtime>::post_dispatch(
            pre,
            &dispatch_info,
            &post_info_from_weight(MOCK_WEIGHT / 2),
            len,
            &Ok(())
        )
        .is_ok());
        assert_eq!(
            Balances::free_balance(alice()),
            balance_after_fee_withdrawal,
        );
    });
}

#[ignore]
#[test]
fn reminting_for_sora_parliament_works() {
    ext().execute_with(|| {
        assert_eq!(
            Balances::free_balance(sora_parliament_account()),
            0_u128.into()
        );
        let call: &<Runtime as frame_system::Config>::Call = &Call::Assets(assets::Call::register(
            AssetSymbol(b"ALIC".to_vec()),
            AssetName(b"ALICE".to_vec()),
            balance!(0),
            true,
        ));

        let len = 10;
        let dispatch_info = info_from_weight(MOCK_WEIGHT);
        let pre = ChargeTransactionPayment::<Runtime>::from(0_u128.into())
            .pre_dispatch(&alice(), call, &dispatch_info, len)
            .unwrap();
        assert!(ChargeTransactionPayment::<Runtime>::post_dispatch(
            pre,
            &dispatch_info,
            &default_post_info(),
            len,
            &Ok(())
        )
        .is_ok());
        let fee = balance!(0.007);
        let xor_into_val_burned_weight = XorIntoValBurnedWeight::get() as u128;
        let weights_sum = ReferrerWeight::get() as u128
            + XorBurnedWeight::get() as u128
            + xor_into_val_burned_weight;
        let x = FixedWrapper::from(fee / (weights_sum / xor_into_val_burned_weight));
        let y = INITIAL_RESERVES;
        let val_burned = (x.clone() * y / (x + y)).into_balance();

        let sora_parliament_share = SoraParliamentShare::get();
        let expected_balance = FixedWrapper::from(sora_parliament_share * val_burned);

        <xor_fee::Module<Runtime> as pallet_session::historical::SessionManager<_, _>>::end_session(
            0,
        );

        // Mock uses MockLiquiditySource that doesn't exchange.
        assert!(
            Tokens::free_balance(VAL.into(), &sora_parliament_account())
                >= (expected_balance.clone() - FixedWrapper::from(1)).into_balance()
                && Balances::free_balance(sora_parliament_account())
                    <= (expected_balance + FixedWrapper::from(1)).into_balance()
        );
    });
}

/// No special fee handling should be performed
#[test]
fn fee_payment_regular_swap() {
    ext().execute_with(|| {
        give_xor_initial_balance(alice());

        let dispatch_info = info_from_weight(100_000_000);

        let call = Call::LiquidityProxy(liquidity_proxy::Call::swap(
            0,
            VAL,
            XOR,
            SwapAmount::WithDesiredInput {
                desired_amount_in: balance!(100),
                min_amount_out: balance!(100),
            },
            vec![],
            FilterMode::Disabled,
        ));

        let regular_fee =
            xor_fee::Pallet::<Runtime>::withdraw_fee(&alice(), &call, &dispatch_info, 1337, 0);

        assert!(matches!(regular_fee, Ok(LiquidityInfo::Paid(_))));
    });
}

/// Fee should be postponed until after the transaction
#[test]
fn fee_payment_postponed() {
    ext().execute_with(|| {
        increase_balance(alice(), VAL.into(), balance!(1000));

        increase_balance(bob(), XOR.into(), balance!(1000));
        increase_balance(bob(), VAL.into(), balance!(1000));

        ensure_pool_initialized(XOR.into(), VAL.into());
        PoolXYK::deposit_liquidity(
            Origin::signed(bob()),
            0,
            XOR.into(),
            VAL.into(),
            balance!(500),
            balance!(500),
            balance!(450),
            balance!(450),
        )
        .unwrap();

        fill_spot_price();

        let dispatch_info = info_from_weight(100_000_000);

        let call = Call::LiquidityProxy(liquidity_proxy::Call::swap(
            0,
            VAL,
            XOR,
            SwapAmount::WithDesiredInput {
                desired_amount_in: balance!(100),
                min_amount_out: balance!(50),
            },
            vec![],
            FilterMode::Disabled,
        ));

        let quoted_fee =
            xor_fee::Pallet::<Runtime>::withdraw_fee(&alice(), &call, &dispatch_info, 1337, 0)
                .unwrap();

        assert!(matches!(quoted_fee, LiquidityInfo::Postponed(SMALL_FEE)));
    });
}

/// Payment should not be postponed if we are not producing XOR
#[test]
fn fee_payment_should_not_postpone() {
    ext().execute_with(|| {
        let dispatch_info = info_from_weight(100_000_000);

        let call = Call::LiquidityProxy(liquidity_proxy::Call::swap(
            0,
            XOR,
            VAL,
            SwapAmount::WithDesiredInput {
                desired_amount_in: balance!(100),
                min_amount_out: balance!(100),
            },
            vec![],
            FilterMode::Disabled,
        ));

        let quoted_fee =
            xor_fee::Pallet::<Runtime>::withdraw_fee(&alice(), &call, &dispatch_info, 1337, 0);

        assert!(matches!(quoted_fee, Err(_)));
    });
}

#[test]
fn withdraw_fee_set_referrer() {
    ext().execute_with(|| {
        increase_balance(bob(), XOR.into(), balance!(1000));

        ReferralSystem::reserve(Origin::signed(bob()), SMALL_FEE).unwrap();

        let dispatch_info = info_from_weight(100_000_000);
        let call = Call::ReferralSystem(referral_system::Call::set_referrer(bob()));
        let result = XorFee::withdraw_fee(&alice(), &call, &dispatch_info, 1337, 0);
        assert_eq!(
            result,
            Ok(LiquidityInfo::Paid((
                bob(),
                Some(NegativeImbalance::new(SMALL_FEE))
            )))
        );
    });
}

#[test]
fn withdraw_fee_set_referrer_already() {
    ext().execute_with(|| {
        ReferralSystem::set_referrer_to(&alice(), bob()).unwrap();

        increase_balance(bob(), XOR.into(), balance!(1000));

        ReferralSystem::reserve(Origin::signed(bob()), SMALL_FEE).unwrap();

        let dispatch_info = info_from_weight(100_000_000);
        let call = Call::ReferralSystem(referral_system::Call::set_referrer(bob()));
        let result = XorFee::withdraw_fee(&alice(), &call, &dispatch_info, 1337, 0);
        assert_eq!(
            result,
            Err(TransactionValidityError::Invalid(
                InvalidTransaction::Payment
            ))
        );
        assert_eq!(ReferrerBalances::<Runtime>::get(&bob()), Some(SMALL_FEE));
    });
}

#[test]
fn withdraw_fee_set_referrer_already2() {
    ext().execute_with(|| {
        ReferralSystem::set_referrer_to(&alice(), bob()).unwrap();

        increase_balance(alice(), XOR.into(), balance!(1));
        increase_balance(bob(), XOR.into(), balance!(1000));

        ReferralSystem::reserve(Origin::signed(bob()), SMALL_FEE).unwrap();

        let dispatch_info = info_from_weight(100_000_000);
        let call = Call::ReferralSystem(referral_system::Call::set_referrer(bob()));
        let result = XorFee::withdraw_fee(&alice(), &call, &dispatch_info, 1337, 0);
        assert_eq!(
            result,
            Ok(LiquidityInfo::Paid((
                alice(),
                Some(NegativeImbalance::new(SMALL_FEE))
            )))
        );
        assert_eq!(
            Assets::free_balance(&XOR.into(), &alice()),
            Ok(balance!(1) - SMALL_FEE)
        );
        assert_eq!(ReferrerBalances::<Runtime>::get(&bob()), Some(SMALL_FEE));
    });
}

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

use frame_support::assert_ok;

use common::{balance, RewardReason, DOT, PSWAP, VAL, XOR, XSTUSD};
use frame_support::log::debug;
use pool_xyk::Properties;
use vested_rewards::Rewards;

use crate::mock::{
    self, run_to_block, AssetId, DEXId, ExtBuilder, Runtime, RuntimeOrigin, ALICE, BOB, CHARLIE,
    DAVE, DEX_A_ID, DEX_B_ID, EVE, REFRESH_FREQUENCY, VESTING_FREQUENCY,
};
use crate::{PoolFarmer, PoolFarmers};

fn init_pool(dex_id: DEXId, base_asset: AssetId, other_asset: AssetId) {
    assert_ok!(trading_pair::Pallet::<Runtime>::register(
        RuntimeOrigin::signed(BOB()),
        dex_id,
        base_asset,
        other_asset
    ));

    assert_ok!(pool_xyk::Pallet::<Runtime>::initialize_pool(
        RuntimeOrigin::signed(BOB()),
        dex_id,
        base_asset,
        other_asset,
    ));
}

// Checks that accounts that have more than 1 XOR are automatically added to farming each REFRESH_FREQUENCY blocks. Also, checks that accounts that no longer has 1 XOR are removed from farming.
#[test]
fn test() {
    let _ = env_logger::Builder::new()
        .filter_level(log::LevelFilter::Debug)
        .try_init();

    let dex_id = DEX_A_ID;
    ExtBuilder::default().build().execute_with(|| {
        init_pool(DEX_A_ID, XOR, DOT);
        init_pool(DEX_A_ID, XOR, PSWAP);
        init_pool(DEX_A_ID, XOR, XSTUSD);
        init_pool(DEX_B_ID, XSTUSD, VAL);
        init_pool(DEX_B_ID, XSTUSD, PSWAP);

        let xor_dot_pool = Properties::<Runtime>::get(XOR, DOT).unwrap().0;
        debug!("xor_dot_pool: {}", xor_dot_pool);
        let xor_pswap_pool = Properties::<Runtime>::get(XOR, PSWAP).unwrap().0;
        debug!("xor_pswap_pool: {}", xor_pswap_pool);
        let xor_xstusd_pool = Properties::<Runtime>::get(XOR, XSTUSD).unwrap().0;
        debug!("xor_xstusd_pool: {}", xor_xstusd_pool);
        let xstusd_val_pool = Properties::<Runtime>::get(XSTUSD, VAL).unwrap().0;
        debug!("xstusd_val_pool: {}", xstusd_val_pool);
        let xstusd_pswap_pool = Properties::<Runtime>::get(XSTUSD, PSWAP).unwrap().0;
        debug!("xstusd_pswap_pool: {}", xstusd_pswap_pool);

        debug!("alice: {}", ALICE());
        debug!("bob: {}", BOB());
        debug!("charlie: {}", CHARLIE());
        debug!("dave: {}", DAVE());
        debug!("eve: {}", EVE());

        // Add liquidity before the first refresh
        {
            assert_ok!(pool_xyk::Pallet::<Runtime>::deposit_liquidity(
                RuntimeOrigin::signed(EVE()),
                DEX_A_ID,
                XOR,
                XSTUSD,
                balance!(10),
                balance!(30),
                balance!(10),
                balance!(30),
            ));

            assert_ok!(pool_xyk::Pallet::<Runtime>::deposit_liquidity(
                RuntimeOrigin::signed(EVE()),
                DEX_B_ID,
                XSTUSD,
                VAL,
                balance!(3.3),
                balance!(0.5),
                balance!(3.3),
                balance!(0.5),
            ));

            assert_ok!(pool_xyk::Pallet::<Runtime>::deposit_liquidity(
                RuntimeOrigin::signed(ALICE()),
                DEX_A_ID,
                XOR,
                DOT,
                balance!(1.1),
                balance!(4.4),
                balance!(1.1),
                balance!(4.4),
            ));

            assert_ok!(pool_xyk::Pallet::<Runtime>::deposit_liquidity(
                RuntimeOrigin::signed(BOB()),
                DEX_A_ID,
                XOR,
                DOT,
                balance!(1.1),
                balance!(4.4),
                balance!(1.1),
                balance!(4.4),
            ));

            assert_ok!(pool_xyk::Pallet::<Runtime>::deposit_liquidity(
                RuntimeOrigin::signed(ALICE()),
                DEX_A_ID,
                XOR,
                PSWAP,
                balance!(1.1),
                balance!(4.4),
                balance!(1.1),
                balance!(4.4),
            ));

            assert_ok!(pool_xyk::Pallet::<Runtime>::deposit_liquidity(
                RuntimeOrigin::signed(ALICE()),
                DEX_B_ID,
                XSTUSD,
                VAL,
                balance!(3.3),
                balance!(0.5),
                balance!(3.3),
                balance!(0.5),
            ));

            assert_ok!(pool_xyk::Pallet::<Runtime>::deposit_liquidity(
                RuntimeOrigin::signed(DAVE()),
                DEX_B_ID,
                XSTUSD,
                PSWAP,
                balance!(3.3),
                balance!(20),
                balance!(3.3),
                balance!(20),
            ));
        }

        mock::run_to_block(REFRESH_FREQUENCY);

        // Check that after the first refresh both Alice and Bob are farmers
        {
            let farmers = PoolFarmers::<Runtime>::get(&xor_dot_pool);
            assert_eq!(
                farmers,
                vec![
                    PoolFarmer {
                        account: ALICE(),
                        block: REFRESH_FREQUENCY,
                        weight: balance!(1.099999999999999498),
                    },
                    PoolFarmer {
                        account: BOB(),
                        block: REFRESH_FREQUENCY,
                        weight: balance!(1.099999999999999998),
                    }
                ]
            );

            let farmers = PoolFarmers::<Runtime>::get(&xor_pswap_pool);
            assert_eq!(
                farmers,
                vec![PoolFarmer {
                    account: ALICE(),
                    block: REFRESH_FREQUENCY,
                    weight: balance!(2.199999999999999000),
                },]
            );

            let farmers = PoolFarmers::<Runtime>::get(&xstusd_pswap_pool);
            assert_eq!(
                farmers,
                vec![PoolFarmer {
                    account: DAVE(),
                    block: REFRESH_FREQUENCY,
                    weight: balance!(2.275862068965516962),
                }]
            );

            let farmers = PoolFarmers::<Runtime>::get(&xstusd_val_pool);
            assert_eq!(
                farmers,
                vec![
                    PoolFarmer {
                        account: ALICE(),
                        block: REFRESH_FREQUENCY,
                        weight: balance!(2.275862068965517238),
                    },
                    PoolFarmer {
                        account: EVE(),
                        block: REFRESH_FREQUENCY,
                        weight: balance!(2.275862068965515470),
                    },
                ]
            );

            let farmers = PoolFarmers::<Runtime>::get(&xor_xstusd_pool);
            assert_eq!(
                farmers,
                vec![PoolFarmer {
                    account: EVE(),
                    block: REFRESH_FREQUENCY,
                    weight: balance!(9.999999999999999430),
                },]
            );
        }

        // Remove Alice and add Charlie before the second refresh
        assert_ok!(pool_xyk::Pallet::<Runtime>::withdraw_liquidity(
            RuntimeOrigin::signed(ALICE()),
            dex_id,
            XOR,
            PSWAP,
            balance!(1),
            balance!(0.1),
            balance!(0.1),
        ));
        assert_ok!(pool_xyk::Pallet::<Runtime>::deposit_liquidity(
            RuntimeOrigin::signed(CHARLIE()),
            dex_id,
            XOR,
            PSWAP,
            balance!(10),
            balance!(40),
            balance!(5),
            balance!(5),
        ));

        mock::run_to_block(REFRESH_FREQUENCY * 2);

        // Check that after the second refresh Alice, Bob and Charlie are farmers
        {
            let farmers = PoolFarmers::<Runtime>::get(&xor_dot_pool);
            assert_eq!(
                farmers,
                vec![
                    PoolFarmer {
                        account: ALICE(),
                        block: REFRESH_FREQUENCY,
                        weight: balance!(1.099999999999999498),
                    },
                    PoolFarmer {
                        account: BOB(),
                        block: REFRESH_FREQUENCY,
                        weight: balance!(1.099999999999999998),
                    }
                ]
            );

            let farmers = PoolFarmers::<Runtime>::get(&xor_pswap_pool);
            assert_eq!(
                farmers,
                vec![PoolFarmer {
                    account: CHARLIE(),
                    block: REFRESH_FREQUENCY * 2,
                    weight: balance!(19.999999999999999962),
                },]
            );
        }

        // Add Alice
        assert_ok!(pool_xyk::Pallet::<Runtime>::deposit_liquidity(
            RuntimeOrigin::signed(ALICE()),
            dex_id,
            XOR,
            PSWAP,
            balance!(10.1),
            balance!(40.4),
            balance!(1.1),
            balance!(4.4),
        ));

        mock::run_to_block(VESTING_FREQUENCY);

        let alice_reward = *Rewards::<Runtime>::get(&ALICE())
            .rewards
            .get(&RewardReason::LiquidityProvisionFarming)
            .unwrap();
        assert_eq!(alice_reward, balance!(147095.556665051128722662));

        let bob_reward = *Rewards::<Runtime>::get(&BOB())
            .rewards
            .get(&RewardReason::LiquidityProvisionFarming)
            .unwrap();
        assert_eq!(bob_reward, balance!(20230.033841899841271451));

        let charlie_reward = *Rewards::<Runtime>::get(&CHARLIE())
            .rewards
            .get(&RewardReason::LiquidityProvisionFarming)
            .unwrap();
        assert_eq!(charlie_reward, balance!(188323.224128231249527342));

        let dave_reward = *Rewards::<Runtime>::get(&DAVE())
            .rewards
            .get(&RewardReason::LiquidityProvisionFarming)
            .unwrap();
        assert_eq!(dave_reward, balance!(41855.242431516907913566));

        let eve_reward = *Rewards::<Runtime>::get(&EVE())
            .rewards
            .get(&RewardReason::LiquidityProvisionFarming)
            .unwrap();
        assert_eq!(eve_reward, balance!(225764.640994242700064977));

        assert_ok!(pool_xyk::Pallet::<Runtime>::deposit_liquidity(
            RuntimeOrigin::signed(ALICE()),
            dex_id,
            XOR,
            DOT,
            balance!(0.5),
            balance!(2),
            balance!(0.3),
            balance!(0.5),
        ));

        assert_ok!(pool_xyk::Pallet::<Runtime>::withdraw_liquidity(
            RuntimeOrigin::signed(BOB()),
            dex_id,
            XOR,
            DOT,
            balance!(1.5),
            balance!(0.5),
            balance!(2),
        ));

        run_to_block(VESTING_FREQUENCY + REFRESH_FREQUENCY);

        let farmers = PoolFarmers::<Runtime>::get(&xor_dot_pool);
        assert_eq!(
            farmers,
            vec![PoolFarmer {
                account: ALICE(),
                block: REFRESH_FREQUENCY,
                weight: balance!(1.599999999999999498),
            }]
        );

        debug!("second vesting");

        run_to_block(VESTING_FREQUENCY + VESTING_FREQUENCY);

        let info = Rewards::<Runtime>::get(&ALICE());
        assert_eq!(
            *info
                .rewards
                .get(&RewardReason::LiquidityProvisionFarming)
                .unwrap(),
            balance!(362281.956723538535819602)
        );

        let info = Rewards::<Runtime>::get(&BOB());
        // BOB's rewards didn't change
        assert_eq!(
            *info
                .rewards
                .get(&RewardReason::LiquidityProvisionFarming)
                .unwrap(),
            balance!(20230.033841899841271451)
        );

        let info = Rewards::<Runtime>::get(&CHARLIE());
        assert_eq!(
            *info
                .rewards
                .get(&RewardReason::LiquidityProvisionFarming)
                .unwrap(),
            balance!(395638.161949291391006768)
        );

        let info = Rewards::<Runtime>::get(&DAVE());
        assert_eq!(
            *info
                .rewards
                .get(&RewardReason::LiquidityProvisionFarming)
                .unwrap(),
            balance!(73254.876962256299307236)
        );

        let info = Rewards::<Runtime>::get(&EVE());
        assert_eq!(
            *info
                .rewards
                .get(&RewardReason::LiquidityProvisionFarming)
                .unwrap(),
            balance!(395132.366644897587594938)
        );
    });
}

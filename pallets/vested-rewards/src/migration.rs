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

use crate::{Config, Pallet, RewardInfo, Weight};
use common::RewardReason;
use frame_support::traits::{Get, GetPalletVersion, PalletVersion};

pub fn migrate<T: Config>() -> Weight {
    let mut weight: Weight = 0;

    #[cfg(feature = "std")]
    println!("{:?}", Pallet::<T>::storage_version());

    match Pallet::<T>::storage_version() {
        Some(version) if version == PalletVersion::new(1, 0, 0) => {
            for (account, (vested_amount, tbc_rewards_amount)) in
                multicollateral_bonding_curve_pool::Rewards::<T>::iter()
            {
                let reward_info = RewardInfo {
                    limit: vested_amount,
                    total_available: tbc_rewards_amount,
                    rewards: [(RewardReason::BuyOnBondingCurve, tbc_rewards_amount)]
                        .iter()
                        .cloned()
                        .collect(),
                };
                // assuming target storage is empty before migration
                crate::Rewards::<T>::insert(account, reward_info);
                weight = weight.saturating_add(T::DbWeight::get().reads_writes(1, 1));
            }
        }
        _ => (),
    }

    weight
}

pub fn get_storage_version<T: Config>() -> Option<PalletVersion> {
    Pallet::<T>::storage_version()
}

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

use crate::{AssetId32, Balance, PredefinedAssetId, TechAssetId};
use codec::{Decode, Encode, MaxEncodedLen};
use frame_support::dispatch::DispatchError;
use frame_support::weights::{
    WeightToFeeCoefficient, WeightToFeeCoefficients, WeightToFeePolynomial,
};
use orml_traits::parameter_type_with_key;
#[cfg(feature = "std")]
use serde::{Deserialize, Serialize};
use smallvec::smallvec;
use sp_arithmetic::Perbill;
use sp_runtime::AccountId32;
use sp_std::convert::TryFrom;

#[derive(
    Encode,
    Decode,
    Eq,
    PartialEq,
    Copy,
    Clone,
    PartialOrd,
    Ord,
    Debug,
    scale_info::TypeInfo,
    MaxEncodedLen,
)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize, Hash))]
#[repr(u8)]
pub enum ComicAssetId {
    GoldenTicket,
    AppleTree,
    Apple,
    Teapot,
    Flower,
    RedPepper,
    BlackPepper,
    AcmeSpyKit,
    BatteryForMusicPlayer,
    MusicPlayer,
    Headphones,
    GreenPromise,
    BluePromise,
    Mango,
    MichaelJacksonCD,
}

impl crate::traits::IsRepresentation for ComicAssetId {
    fn is_representation(&self) -> bool {
        false
    }
}

impl From<PredefinedAssetId> for AssetId32<ComicAssetId> {
    fn from(asset: PredefinedAssetId) -> Self {
        let comic = ComicAssetId::from(asset);
        AssetId32::<ComicAssetId>::from(comic)
    }
}

impl From<PredefinedAssetId> for ComicAssetId {
    fn from(asset_id: PredefinedAssetId) -> Self {
        use ComicAssetId::*;
        match asset_id {
            PredefinedAssetId::XOR => GoldenTicket,
            PredefinedAssetId::DOT => AppleTree,
            PredefinedAssetId::KSM => Apple,
            PredefinedAssetId::USDT => Teapot,
            PredefinedAssetId::VAL => Flower,
            PredefinedAssetId::PSWAP => RedPepper,
            PredefinedAssetId::DAI => BlackPepper,
            PredefinedAssetId::ETH => AcmeSpyKit,
            PredefinedAssetId::XSTUSD => Mango,
            PredefinedAssetId::XST => BatteryForMusicPlayer,
            PredefinedAssetId::TBCD => MichaelJacksonCD,
        }
    }
}

impl Default for ComicAssetId {
    fn default() -> Self {
        Self::GoldenTicket
    }
}

// This is never used, and just makes some tests compatible.
impl From<AssetId32<PredefinedAssetId>> for AssetId32<ComicAssetId> {
    fn from(_asset: AssetId32<PredefinedAssetId>) -> Self {
        unreachable!()
    }
}

// This is never used, and just makes some tests compatible.
impl From<TechAssetId<PredefinedAssetId>> for PredefinedAssetId {
    fn from(_tech: TechAssetId<PredefinedAssetId>) -> Self {
        unimplemented!()
    }
}

// This is never used, and just makes some tests compatible.
impl TryFrom<PredefinedAssetId> for TechAssetId<TechAssetId<PredefinedAssetId>>
where
    TechAssetId<PredefinedAssetId>: Decode,
{
    type Error = DispatchError;
    fn try_from(_asset: PredefinedAssetId) -> Result<Self, Self::Error> {
        unimplemented!()
    }
}

impl From<PredefinedAssetId> for TechAssetId<ComicAssetId> {
    fn from(asset_id: PredefinedAssetId) -> Self {
        TechAssetId::Wrapped(ComicAssetId::from(asset_id))
    }
}

pub struct WeightToFixedFee;

impl WeightToFeePolynomial for WeightToFixedFee {
    type Balance = Balance;

    fn polynomial() -> WeightToFeeCoefficients<Self::Balance> {
        smallvec!(WeightToFeeCoefficient {
            coeff_integer: 7_000_000,
            coeff_frac: Perbill::zero(),
            negative: false,
            degree: 1,
        })
    }
}

parameter_type_with_key! {
    pub ExistentialDeposits: |_currency_id: AssetId32<PredefinedAssetId>| -> Balance {
        0
    };
}
pub struct GetTradingPairRestrictedFlag;

impl<T> orml_traits::get_by_key::GetByKey<T, bool> for GetTradingPairRestrictedFlag {
    fn get(_key: &T) -> bool {
        false
    }
}

pub fn alice() -> AccountId32 {
    AccountId32::from([1; 32])
}

pub fn bob() -> AccountId32 {
    AccountId32::from([2; 32])
}

pub fn charlie() -> AccountId32 {
    AccountId32::from([3; 32])
}

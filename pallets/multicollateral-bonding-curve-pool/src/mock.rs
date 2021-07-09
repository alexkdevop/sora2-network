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

use crate::{self as multicollateral_bonding_curve_pool, Config, Rewards, TotalRewards};
use common::mock::ExistentialDeposits;
use common::prelude::{
    Balance, FixedWrapper, PriceToolsPallet, QuoteAmount, SwapAmount, SwapOutcome,
};
use common::{
    self, balance, fixed, fixed_wrapper, hash, Amount, AssetId32, AssetName, AssetSymbol, DEXInfo,
    Fixed, LiquiditySourceFilter, LiquiditySourceType, TechPurpose, VestedRewardsPallet, PSWAP,
    USDT, VAL, XOR, XSTUSD,
};
use currencies::BasicCurrencyAdapter;
use frame_support::traits::GenesisBuild;
use frame_support::weights::Weight;
use frame_support::{construct_runtime, parameter_types};
use hex_literal::hex;
use orml_traits::MultiCurrency;
use permissions::{Scope, INIT_DEX, MANAGE_DEX};
use sp_core::crypto::AccountId32;
use sp_core::H256;
use sp_runtime::testing::Header;
use sp_runtime::traits::{BlakeTwo256, IdentityLookup, Zero};
use sp_runtime::{DispatchError, DispatchResult, Perbill};
use std::collections::HashMap;

pub type AccountId = AccountId32;
pub type BlockNumber = u64;
pub type TechAccountId = common::TechAccountId<AccountId, TechAssetId, DEXId>;
type TechAssetId = common::TechAssetId<common::PredefinedAssetId>;
pub type ReservesAccount =
    mock_liquidity_source::ReservesAcc<Runtime, mock_liquidity_source::Instance1>;
pub type AssetId = AssetId32<common::PredefinedAssetId>;
type DEXId = common::DEXId;
type UncheckedExtrinsic = frame_system::mocking::MockUncheckedExtrinsic<Runtime>;
type Block = frame_system::mocking::MockBlock<Runtime>;

pub fn alice() -> AccountId {
    AccountId32::from([1u8; 32])
}

pub fn bob() -> AccountId {
    AccountId32::from([2u8; 32])
}

pub fn assets_owner() -> AccountId {
    AccountId32::from([3u8; 32])
}

pub fn incentives_account() -> AccountId {
    AccountId32::from([4u8; 32])
}

pub fn free_reserves_account() -> AccountId {
    AccountId32::from([5u8; 32])
}

pub fn get_pool_reserves_account_id() -> AccountId {
    let reserves_tech_account_id = crate::ReservesAcc::<Runtime>::get();
    let reserves_account_id =
        Technical::tech_account_id_to_account_id(&reserves_tech_account_id).unwrap();
    reserves_account_id
}

pub const DEX_A_ID: DEXId = DEXId::Polkaswap;
pub const DAI: AssetId = common::AssetId32::from_bytes(hex!(
    "0200060000000000000000000000000000000000000000000000000000000111"
));

parameter_types! {
    pub const BlockHashCount: u64 = 250;
    pub const MaximumBlockWeight: Weight = 1024;
    pub const MaximumBlockLength: u32 = 2 * 1024;
    pub const AvailableBlockRatio: Perbill = Perbill::from_percent(75);
    pub const GetDefaultFee: u16 = 30;
    pub const GetDefaultProtocolFee: u16 = 0;
    pub const GetBaseAssetId: AssetId = XOR;
    pub const ExistentialDeposit: u128 = 0;
    pub const TransferFee: u128 = 0;
    pub const CreationFee: u128 = 0;
    pub const TransactionByteFee: u128 = 1;
    pub const GetNumSamples: usize = 40;
    pub GetIncentiveAssetId: AssetId = common::AssetId32::from_bytes(hex!("0200050000000000000000000000000000000000000000000000000000000000").into());
    pub GetPswapDistributionAccountId: AccountId = AccountId32::from([151; 32]);
    pub const GetDefaultSubscriptionFrequency: BlockNumber = 10;
    pub const GetBurnUpdateFrequency: BlockNumber = 14400;
    pub GetParliamentAccountId: AccountId = AccountId32::from([152; 32]);
    pub GetMarketMakerRewardsAccountId: AccountId = AccountId32::from([153; 32]);
    pub GetBondingCurveRewardsAccountId: AccountId = AccountId32::from([154; 32]);
    pub GetTeamReservesAccountId: AccountId = AccountId32::from([11; 32]);
    pub GetXykFee: Fixed = fixed!(0.003);
}

construct_runtime! {
    pub enum Runtime where
        Block = Block,
        NodeBlock = Block,
        UncheckedExtrinsic = UncheckedExtrinsic,
    {
        System: frame_system::{Module, Call, Config, Storage, Event<T>},
        DexManager: dex_manager::{Module, Call, Storage},
        TradingPair: trading_pair::{Module, Call, Storage, Event<T>},
        MockLiquiditySource: mock_liquidity_source::<Instance1>::{Module, Call, Config<T>, Storage},
        // VestedRewards: vested_rewards::{Module, Call, Storage, Event<T>},
        Mcbcp: multicollateral_bonding_curve_pool::{Module, Call, Storage, Event<T>},
        Tokens: tokens::{Module, Call, Config<T>, Storage, Event<T>},
        Currencies: currencies::{Module, Call, Storage, Event<T>},
        Assets: assets::{Module, Call, Config<T>, Storage, Event<T>},
        Permissions: permissions::{Module, Call, Config<T>, Storage, Event<T>},
        Technical: technical::{Module, Call, Storage, Event<T>},
        Balances: pallet_balances::{Module, Call, Storage, Event<T>},
        PoolXYK: pool_xyk::{Module, Call, Storage, Event<T>},
        PswapDistribution: pswap_distribution::{Module, Call, Storage, Event<T>},
    }
}

impl frame_system::Config for Runtime {
    type BaseCallFilter = ();
    type BlockWeights = ();
    type BlockLength = ();
    type Origin = Origin;
    type Call = Call;
    type Index = u64;
    type BlockNumber = u64;
    type Hash = H256;
    type Hashing = BlakeTwo256;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
    type Header = Header;
    type Event = Event;
    type BlockHashCount = BlockHashCount;
    type DbWeight = ();
    type Version = ();
    type AccountData = pallet_balances::AccountData<Balance>;
    type OnNewAccount = ();
    type OnKilledAccount = ();
    type SystemWeightInfo = ();
    type PalletInfo = PalletInfo;
    type SS58Prefix = ();
}

impl dex_manager::Config for Runtime {}

impl trading_pair::Config for Runtime {
    type Event = Event;
    type EnsureDEXManager = dex_manager::Module<Runtime>;
    type WeightInfo = ();
}

impl mock_liquidity_source::Config<mock_liquidity_source::Instance1> for Runtime {
    type GetFee = ();
    type EnsureDEXManager = ();
    type EnsureTradingPairExists = ();
}

impl Config for Runtime {
    type Event = Event;
    type LiquidityProxy = MockDEXApi;
    type EnsureTradingPairExists = trading_pair::Module<Runtime>;
    type EnsureDEXManager = dex_manager::Module<Runtime>;
    type PriceToolsPallet = MockDEXApi;
    type VestedRewardsPallet = MockVestedRewards;
    type WeightInfo = ();
}

pub struct MockVestedRewards;

impl VestedRewardsPallet<AccountId> for MockVestedRewards {
    fn update_market_maker_records(_: &AccountId, _: Balance, _: u32) -> DispatchResult {
        // do nothing
        Ok(())
    }
    fn add_tbc_reward(account: &AccountId, amount: Balance) -> DispatchResult {
        Rewards::<Runtime>::mutate(account, |(_, old_amount)| {
            *old_amount = old_amount.saturating_add(amount)
        });
        TotalRewards::<Runtime>::mutate(|old_amount| {
            *old_amount = old_amount.saturating_add(amount)
        });
        Ok(())
    }

    fn add_farming_reward(_: &AccountId, _: Balance) -> DispatchResult {
        // do nothing
        Ok(())
    }

    fn add_market_maker_reward(_: &AccountId, _: Balance) -> DispatchResult {
        // do nothing
        Ok(())
    }
}

impl tokens::Config for Runtime {
    type Event = Event;
    type Balance = Balance;
    type Amount = Amount;
    type CurrencyId = <Runtime as assets::Config>::AssetId;
    type WeightInfo = ();
    type ExistentialDeposits = ExistentialDeposits;
    type OnDust = ();
}

impl currencies::Config for Runtime {
    type Event = Event;
    type MultiCurrency = Tokens;
    type NativeCurrency = BasicCurrencyAdapter<Runtime, Balances, Amount, BlockNumber>;
    type GetNativeCurrencyId = <Runtime as assets::Config>::GetBaseAssetId;
    type WeightInfo = ();
}

impl common::Config for Runtime {
    type DEXId = DEXId;
    type LstId = common::LiquiditySourceType;
}

impl assets::Config for Runtime {
    type Event = Event;
    type ExtraAccountId = [u8; 32];
    type ExtraAssetRecordArg =
        common::AssetIdExtraAssetRecordArg<common::DEXId, common::LiquiditySourceType, [u8; 32]>;
    type AssetId = AssetId;
    type GetBaseAssetId = GetBaseAssetId;
    type Currency = currencies::Module<Runtime>;
    type GetTeamReservesAccountId = GetTeamReservesAccountId;
    type WeightInfo = ();
}

impl permissions::Config for Runtime {
    type Event = Event;
}

impl technical::Config for Runtime {
    type Event = Event;
    type TechAssetId = TechAssetId;
    type TechAccountId = TechAccountId;
    type Trigger = ();
    type Condition = ();
    type SwapAction = pool_xyk::PolySwapAction<AssetId, AccountId, TechAccountId>;
    type WeightInfo = ();
}

impl pallet_balances::Config for Runtime {
    type Balance = Balance;
    type Event = Event;
    type DustRemoval = ();
    type ExistentialDeposit = ExistentialDeposit;
    type AccountStore = System;
    type WeightInfo = ();
    type MaxLocks = ();
}

impl pswap_distribution::Config for Runtime {
    type Event = Event;
    type GetIncentiveAssetId = GetIncentiveAssetId;
    type LiquidityProxy = ();
    type CompatBalance = Balance;
    type GetDefaultSubscriptionFrequency = GetDefaultSubscriptionFrequency;
    type GetBurnUpdateFrequency = GetBurnUpdateFrequency;
    type GetTechnicalAccountId = GetPswapDistributionAccountId;
    type EnsureDEXManager = ();
    type OnPswapBurnedAggregator = ();
    type WeightInfo = ();
    type GetParliamentAccountId = GetParliamentAccountId;
    type PoolXykPallet = PoolXYK;
}

impl pool_xyk::Config for Runtime {
    const MIN_XOR: Balance = balance!(0.0007);
    type Event = Event;
    type PairSwapAction = pool_xyk::PairSwapAction<AssetId, AccountId, TechAccountId>;
    type DepositLiquidityAction =
        pool_xyk::DepositLiquidityAction<AssetId, AccountId, TechAccountId>;
    type WithdrawLiquidityAction =
        pool_xyk::WithdrawLiquidityAction<AssetId, AccountId, TechAccountId>;
    type PolySwapAction = pool_xyk::PolySwapAction<AssetId, AccountId, TechAccountId>;
    type EnsureDEXManager = dex_manager::Module<Runtime>;
    type GetFee = GetXykFee;
    type OnPoolCreated = PswapDistribution;
    type WeightInfo = ();
}

pub struct MockDEXApi;

impl MockDEXApi {
    fn get_mock_source_account() -> Result<(TechAccountId, AccountId), DispatchError> {
        let tech_account_id =
            TechAccountId::Pure(DEXId::Polkaswap.into(), TechPurpose::FeeCollector);
        let account_id = Technical::tech_account_id_to_account_id(&tech_account_id)?;
        Ok((tech_account_id, account_id))
    }

    pub fn init_without_reserves() -> Result<(), DispatchError> {
        let (tech_account_id, _) = Self::get_mock_source_account()?;
        Technical::register_tech_account_id(tech_account_id.clone())?;
        MockLiquiditySource::set_reserves_account_id(tech_account_id)?;
        Ok(())
    }

    pub fn add_reserves(funds: Vec<(AssetId, Balance)>) -> Result<(), DispatchError> {
        let (_, account_id) = Self::get_mock_source_account()?;
        for (asset_id, balance) in funds {
            Currencies::deposit(asset_id, &account_id, balance)?;
        }
        Ok(())
    }

    pub fn init() -> Result<(), DispatchError> {
        Self::init_without_reserves()?;
        Self::add_reserves(vec![
            (XOR, balance!(100000)),
            (VAL, balance!(100000)),
            (USDT, balance!(1000000)),
        ])?;
        Ok(())
    }

    fn _can_exchange(
        _target_id: &DEXId,
        input_asset_id: &AssetId,
        output_asset_id: &AssetId,
    ) -> bool {
        get_mock_prices().contains_key(&(*input_asset_id, *output_asset_id))
    }

    fn inner_quote(
        _target_id: &DEXId,
        input_asset_id: &AssetId,
        output_asset_id: &AssetId,
        swap_amount: QuoteAmount<Balance>,
    ) -> Result<SwapOutcome<Balance>, DispatchError> {
        match swap_amount {
            QuoteAmount::WithDesiredInput {
                desired_amount_in, ..
            } => {
                let amount_out = FixedWrapper::from(desired_amount_in)
                    * get_mock_prices()[&(*input_asset_id, *output_asset_id)];
                let fee = amount_out.clone() * balance!(0.003);
                let fee = fee.into_balance();
                let amount_out: Balance = amount_out.into_balance();
                let amount_out = amount_out - fee;
                Ok(SwapOutcome::new(amount_out, fee))
            }
            QuoteAmount::WithDesiredOutput {
                desired_amount_out, ..
            } => {
                let amount_in = FixedWrapper::from(desired_amount_out)
                    / get_mock_prices()[&(*input_asset_id, *output_asset_id)];
                let with_fee = amount_in.clone() / balance!(0.997);
                let fee = with_fee.clone() - amount_in;
                let fee = fee.into_balance();
                let with_fee = with_fee.into_balance();
                Ok(SwapOutcome::new(with_fee, fee))
            }
        }
    }

    fn inner_exchange(
        sender: &AccountId,
        receiver: &AccountId,
        target_id: &DEXId,
        input_asset_id: &AssetId,
        output_asset_id: &AssetId,
        swap_amount: SwapAmount<Balance>,
    ) -> Result<SwapOutcome<Balance>, DispatchError> {
        match swap_amount {
            SwapAmount::WithDesiredInput {
                desired_amount_in, ..
            } => {
                let outcome = Self::inner_quote(
                    target_id,
                    input_asset_id,
                    output_asset_id,
                    swap_amount.into(),
                )?;
                let reserves_account_id =
                    &Technical::tech_account_id_to_account_id(&ReservesAccount::get())?;
                assert_ne!(desired_amount_in, 0);
                let old = Assets::total_balance(input_asset_id, sender)?;
                Assets::transfer_from(
                    input_asset_id,
                    sender,
                    reserves_account_id,
                    desired_amount_in,
                )?;
                let new = Assets::total_balance(input_asset_id, sender)?;
                assert_ne!(old, new);
                Assets::transfer_from(
                    output_asset_id,
                    reserves_account_id,
                    receiver,
                    outcome.amount,
                )?;
                Ok(SwapOutcome::new(outcome.amount, outcome.fee))
            }
            SwapAmount::WithDesiredOutput {
                desired_amount_out, ..
            } => {
                let outcome = Self::inner_quote(
                    target_id,
                    input_asset_id,
                    output_asset_id,
                    swap_amount.into(),
                )?;
                let reserves_account_id =
                    &Technical::tech_account_id_to_account_id(&ReservesAccount::get())?;
                assert_ne!(outcome.amount, 0);
                let old = Assets::total_balance(input_asset_id, sender)?;
                Assets::transfer_from(input_asset_id, sender, reserves_account_id, outcome.amount)?;
                let new = Assets::total_balance(input_asset_id, sender)?;
                assert_ne!(old, new);
                Assets::transfer_from(
                    output_asset_id,
                    reserves_account_id,
                    receiver,
                    desired_amount_out,
                )?;
                Ok(SwapOutcome::new(outcome.amount, outcome.fee))
            }
        }
    }
}

pub fn get_mock_prices() -> HashMap<(AssetId, AssetId), Balance> {
    let direct = vec![
        ((XOR, VAL), balance!(2.0)),
        // USDT
        ((XOR, USDT), balance!(100.0)),
        ((VAL, USDT), balance!(50.0)),
        // DAI
        ((XOR, DAI), balance!(102.0)),
        ((VAL, DAI), balance!(51.0)),
        ((USDT, DAI), balance!(1.02)),
        ((XSTUSD, DAI), balance!(1)),
        // PSWAP
        ((XOR, PSWAP), balance!(10)),
        ((VAL, PSWAP), balance!(5)),
        ((USDT, PSWAP), balance!(0.1)),
        ((DAI, PSWAP), balance!(0.098)),
        ((XSTUSD, PSWAP), balance!(1)),
        // XSTUSD
        ((XOR, XSTUSD), balance!(102.0)),
    ];
    let reverse = direct.clone().into_iter().map(|((a, b), price)| {
        (
            (b, a),
            (fixed_wrapper!(1) / FixedWrapper::from(price))
                .try_into_balance()
                .unwrap(),
        )
    });
    direct.into_iter().chain(reverse).collect()
}

impl liquidity_proxy::LiquidityProxyTrait<DEXId, AccountId, AssetId> for MockDEXApi {
    fn exchange(
        sender: &AccountId,
        receiver: &AccountId,
        input_asset_id: &AssetId,
        output_asset_id: &AssetId,
        amount: SwapAmount<Balance>,
        filter: LiquiditySourceFilter<DEXId, LiquiditySourceType>,
    ) -> Result<SwapOutcome<Balance>, DispatchError> {
        Self::inner_exchange(
            sender,
            receiver,
            &filter.dex_id,
            input_asset_id,
            output_asset_id,
            amount,
        )
    }

    fn quote(
        input_asset_id: &AssetId,
        output_asset_id: &AssetId,
        amount: QuoteAmount<Balance>,
        filter: LiquiditySourceFilter<DEXId, LiquiditySourceType>,
    ) -> Result<SwapOutcome<Balance>, DispatchError> {
        Self::inner_quote(&filter.dex_id, input_asset_id, output_asset_id, amount)
    }
}

impl PriceToolsPallet<AssetId> for MockDEXApi {
    fn get_average_price(
        input_asset_id: &AssetId,
        output_asset_id: &AssetId,
    ) -> Result<Balance, DispatchError> {
        Ok(Self::inner_quote(
            &DEXId::Polkaswap.into(),
            input_asset_id,
            output_asset_id,
            QuoteAmount::with_desired_input(balance!(1)),
        )?
        .amount)
    }

    fn register_asset(_: &AssetId) -> DispatchResult {
        // do nothing
        Ok(())
    }
}

pub struct ExtBuilder {
    endowed_accounts: Vec<(AccountId, AssetId, Balance, AssetSymbol, AssetName, u8)>,
    dex_list: Vec<(DEXId, DEXInfo<AssetId>)>,
    initial_permission_owners: Vec<(u32, Scope, Vec<AccountId>)>,
    initial_permissions: Vec<(AccountId, Scope, Vec<u32>)>,
    reference_asset_id: AssetId,
}

impl Default for ExtBuilder {
    fn default() -> Self {
        Self {
            endowed_accounts: vec![
                (
                    alice(),
                    USDT,
                    0,
                    AssetSymbol(b"USDT".to_vec()),
                    AssetName(b"Tether USD".to_vec()),
                    18,
                ),
                (
                    alice(),
                    XOR,
                    balance!(350000),
                    AssetSymbol(b"XOR".to_vec()),
                    AssetName(b"SORA".to_vec()),
                    18,
                ),
                (
                    alice(),
                    VAL,
                    balance!(500000),
                    AssetSymbol(b"VAL".to_vec()),
                    AssetName(b"SORA Validator Token".to_vec()),
                    18,
                ),
                (
                    alice(),
                    PSWAP,
                    balance!(0),
                    AssetSymbol(b"PSWAP".to_vec()),
                    AssetName(b"Polkaswap Token".to_vec()),
                    18,
                ),
                (
                    alice(),
                    XSTUSD,
                    balance!(100),
                    AssetSymbol(b"XSTUSD".to_vec()),
                    AssetName(b"XST USD".to_vec()),
                    18,
                ),
                (
                    alice(),
                    DAI,
                    balance!(100),
                    AssetSymbol(b"DAI".to_vec()),
                    AssetName(b"DAI".to_vec()),
                    18,
                ),
            ],
            dex_list: vec![(
                DEX_A_ID,
                DEXInfo {
                    base_asset_id: GetBaseAssetId::get(),
                    is_public: true,
                },
            )],
            initial_permission_owners: vec![
                (INIT_DEX, Scope::Unlimited, vec![alice()]),
                (MANAGE_DEX, Scope::Limited(hash(&DEX_A_ID)), vec![alice()]),
            ],
            initial_permissions: vec![
                (alice(), Scope::Unlimited, vec![INIT_DEX]),
                (alice(), Scope::Limited(hash(&DEX_A_ID)), vec![MANAGE_DEX]),
                (
                    assets_owner(),
                    Scope::Unlimited,
                    vec![permissions::MINT, permissions::BURN],
                ),
                (
                    free_reserves_account(),
                    Scope::Unlimited,
                    vec![permissions::MINT, permissions::BURN],
                ),
            ],
            reference_asset_id: USDT,
        }
    }
}

impl ExtBuilder {
    pub fn new(
        endowed_accounts: Vec<(AccountId, AssetId, Balance, AssetSymbol, AssetName, u8)>,
    ) -> Self {
        Self {
            endowed_accounts,
            ..Default::default()
        }
    }

    pub fn build(self) -> sp_io::TestExternalities {
        let mut t = frame_system::GenesisConfig::default()
            .build_storage::<Runtime>()
            .unwrap();

        pallet_balances::GenesisConfig::<Runtime> {
            balances: self
                .endowed_accounts
                .iter()
                .cloned()
                .filter_map(|(account_id, asset_id, balance, ..)| {
                    if asset_id == GetBaseAssetId::get() {
                        Some((account_id, balance))
                    } else {
                        None
                    }
                })
                .chain(vec![
                    (bob(), 0),
                    (assets_owner(), 0),
                    (incentives_account(), 0),
                    (free_reserves_account(), 0),
                ])
                .collect(),
        }
        .assimilate_storage(&mut t)
        .unwrap();

        crate::GenesisConfig::<Runtime> {
            distribution_accounts: Default::default(),
            reserves_account_id: Default::default(),
            reference_asset_id: self.reference_asset_id,
            incentives_account_id: incentives_account(),
            initial_collateral_assets: Default::default(),
            free_reserves_account_id: free_reserves_account(),
        }
        .assimilate_storage(&mut t)
        .unwrap();

        dex_manager::GenesisConfig::<Runtime> {
            dex_list: self.dex_list,
        }
        .assimilate_storage(&mut t)
        .unwrap();

        permissions::GenesisConfig::<Runtime> {
            initial_permission_owners: self.initial_permission_owners,
            initial_permissions: self.initial_permissions,
        }
        .assimilate_storage(&mut t)
        .unwrap();

        assets::GenesisConfig::<Runtime> {
            endowed_assets: self
                .endowed_accounts
                .iter()
                .cloned()
                .map(|(account_id, asset_id, _, symbol, name, precision)| {
                    (
                        asset_id,
                        account_id,
                        symbol,
                        name,
                        precision,
                        Balance::zero(),
                        true,
                    )
                })
                .collect(),
        }
        .assimilate_storage(&mut t)
        .unwrap();

        tokens::GenesisConfig::<Runtime> {
            endowed_accounts: self
                .endowed_accounts
                .into_iter()
                .map(|(account_id, asset_id, balance, ..)| (account_id, asset_id, balance))
                .collect(),
        }
        .assimilate_storage(&mut t)
        .unwrap();

        t.into()
    }
}

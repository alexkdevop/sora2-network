use crate::{Module, Trait};
use codec::{Decode, Encode};
use common::BasisPoints;
use currencies::BasicCurrencyAdapter;
use frame_support::dispatch;
use frame_support::{impl_outer_origin, parameter_types, weights::Weight};
use frame_system as system;
use sp_core::crypto::AccountId32;
use sp_core::H256;
use sp_runtime::{
    testing::Header,
    traits::{BlakeTwo256, IdentityLookup},
    Perbill,
};

pub use common::{mock::ComicAssetId::*, mock::*, TechAssetId::*, TechPurpose::*, TradingPair};

pub type AssetId = common::mock::ComicAssetId;
pub type Technical = Module<Testtime>;

pub fn get_alice() -> AccountId {
    AccountId32::from([1; 32])
}
pub fn get_bob() -> AccountId {
    AccountId32::from([2; 32])
}

pub struct ExtBuilder {
    endowed_accounts: Vec<(AccountId, AssetId, Balance)>,
}

impl Default for ExtBuilder {
    fn default() -> Self {
        Self {
            endowed_accounts: vec![
                (get_alice(), RedPepper, 99_000_u128),
                (get_alice(), BlackPepper, 2000_000_u128),
                (get_bob(), RedPepper, 2000_000_u128),
            ],
        }
    }
}

impl_outer_origin! {
    pub enum Origin for Testtime {}
}

// Configure a mock runtime to test the pallet.

#[derive(Clone, Eq, PartialEq)]
pub struct Testtime;
parameter_types! {
    pub const BlockHashCount: u64 = 250;
    pub const MaximumBlockWeight: Weight = 1024;
    pub const MaximumBlockLength: u32 = 2 * 1024;
    pub const AvailableBlockRatio: Perbill = Perbill::from_percent(75);
}

impl system::Trait for Testtime {
    type BaseCallFilter = ();
    type Origin = Origin;
    type Call = ();
    type Index = u64;
    type BlockNumber = BlockNumber;
    type Hash = H256;
    type Hashing = BlakeTwo256;
    type AccountId = AccountId;
    type Lookup = IdentityLookup<Self::AccountId>;
    type Header = Header;
    type Event = ();
    type BlockHashCount = BlockHashCount;
    type MaximumBlockWeight = MaximumBlockWeight;
    type DbWeight = ();
    type BlockExecutionWeight = ();
    type ExtrinsicBaseWeight = ();
    type MaximumExtrinsicWeight = MaximumBlockWeight;
    type MaximumBlockLength = MaximumBlockLength;
    type AvailableBlockRatio = AvailableBlockRatio;
    type Version = ();
    type ModuleToIndex = ();
    type AccountData = pallet_balances::AccountData<Balance>;
    type OnNewAccount = ();
    type OnKilledAccount = ();
    type SystemWeightInfo = ();
}

parameter_types! {
    pub const GetDefaultFee: BasisPoints = 30;
    pub const GetDefaultProtocolFee: BasisPoints = 0;
}

impl permissions::Trait for Testtime {
    type Event = ();
}

impl dex_manager::Trait for Testtime {
    type Event = ();
    type GetDefaultFee = GetDefaultFee;
    type GetDefaultProtocolFee = GetDefaultProtocolFee;
}

type DEXId = u32;

pub type BlockNumber = u64;
pub type AccountId = AccountId32;
pub type Amount = i128;
pub type Balance = u128;

impl common::Trait for Testtime {
    type DEXId = DEXId;
}

parameter_types! {
    pub const GetBaseAssetId: AssetId = GoldenTicket;
}

parameter_types! {
    pub const ExistentialDeposit: u128 = 0;
}

pub type System = frame_system::Module<Testtime>;

impl pallet_balances::Trait for Testtime {
    type Balance = Balance;
    type Event = ();
    type DustRemoval = ();
    type ExistentialDeposit = ExistentialDeposit;
    type AccountStore = System;
    type WeightInfo = ();
}

impl tokens::Trait for Testtime {
    type Event = ();
    type Balance = Balance;
    type Amount = Amount;
    type CurrencyId = <Testtime as assets::Trait>::AssetId;
    type OnReceived = ();
}

impl currencies::Trait for Testtime {
    type Event = ();
    type MultiCurrency = tokens::Module<Testtime>;
    type NativeCurrency = BasicCurrencyAdapter<
        pallet_balances::Module<Testtime>,
        Balance,
        Balance,
        Amount,
        BlockNumber,
    >;
    type GetNativeCurrencyId = <Testtime as assets::Trait>::GetBaseAssetId;
}

impl assets::Trait for Testtime {
    type Event = ();
    type AssetId = AssetId;
    type GetBaseAssetId = GetBaseAssetId;
}

#[derive(Clone, Eq, PartialEq, Encode, Decode, Debug)]
pub struct GenericPairSwapActionExample {
    pub give_minted: bool,
    pub give_asset: AssetId,
    pub give_amount: TechBalance,
    pub take_burn: bool,
    pub take_asset: AssetId,
    pub take_amount: TechBalance,
    pub take_account: TechAccountId,
}

impl common::SwapAction<AccountId, TechAccountId, Testtime> for GenericPairSwapActionExample {
    fn reserve(&self, source: &AccountId) -> dispatch::DispatchResult {
        let src = (*source).clone();
        //FIXME now in this place exist two operations, and it is not lock.
        crate::Module::<Testtime>::set_transfer_in(
            self.give_asset.into(),
            src.clone(),
            self.take_account.clone(),
            self.give_amount,
        )?;
        crate::Module::<Testtime>::set_transfer_out(
            self.take_asset.into(),
            self.take_account.clone(),
            src.clone(),
            self.take_amount,
        )?;
        Ok(())
    }
    fn claim(&self, _source: &AccountId) -> bool {
        //FIXME implement lock for swap and apply swap from lock, these operation must come
        //together and appears in one block as one container for operations.
        true
    }
    fn weight(&self) -> Weight {
        unimplemented!()
    }
    fn cancel(&self, _source: &AccountId) {
        unimplemented!()
    }
}

impl common::SwapRulesValidation<AccountId, TechAccountId, Testtime>
    for GenericPairSwapActionExample
{
    fn validate(&self, _source: &AccountId) -> bool {
        true
    }
    fn instant_auto_claim_used(&self) -> bool {
        true
    }
    fn triggered_auto_claim_used(&self) -> bool {
        false
    }
    fn is_able_to_claim(&self) -> bool {
        true
    }
}

#[derive(Clone, Eq, PartialEq, Encode, Decode, Debug)]
pub struct MultiSwapActionExample {
    give_amount_a: TechAmount,
    give_amount_b: TechAmount,
    take_amount_c: TechAmount,
    take_amount_d: TechAmount,
    take_amount_e: TechAmount,
}

impl common::SwapAction<AccountId, TechAccountId, Testtime> for MultiSwapActionExample {
    fn reserve(&self, _source: &AccountId) -> dispatch::DispatchResult {
        Ok(())
    }
    fn claim(&self, _source: &AccountId) -> bool {
        true
    }
    fn weight(&self) -> Weight {
        unimplemented!()
    }
    fn cancel(&self, _source: &AccountId) {
        unimplemented!()
    }
}

impl common::SwapRulesValidation<AccountId, TechAccountId, Testtime> for MultiSwapActionExample {
    fn validate(&self, _source: &AccountId) -> bool {
        true
    }
    fn instant_auto_claim_used(&self) -> bool {
        true
    }
    fn triggered_auto_claim_used(&self) -> bool {
        true
    }
    fn is_able_to_claim(&self) -> bool {
        true
    }
}

#[derive(Clone, Eq, PartialEq, Encode, Decode, Debug)]
pub struct CrowdSwapActionExample {
    crowd_id: u32,
    give_amount: TechAmount,
    take_amount: TechAmount,
}

impl common::SwapAction<AccountId, TechAccountId, Testtime> for CrowdSwapActionExample {
    fn reserve(&self, _source: &AccountId) -> dispatch::DispatchResult {
        unimplemented!()
    }
    fn claim(&self, _source: &AccountId) -> bool {
        true
    }
    fn weight(&self) -> Weight {
        unimplemented!()
    }
    fn cancel(&self, _source: &AccountId) {
        unimplemented!()
    }
}

impl common::SwapRulesValidation<AccountId, TechAccountId, Testtime> for CrowdSwapActionExample {
    fn validate(&self, _source: &AccountId) -> bool {
        true
    }
    fn instant_auto_claim_used(&self) -> bool {
        false
    }
    fn triggered_auto_claim_used(&self) -> bool {
        true
    }
    fn is_able_to_claim(&self) -> bool {
        true
    }
}

#[derive(Clone, Eq, PartialEq, Encode, Decode, Debug)]
pub enum PolySwapActionExample {
    GenericPair(GenericPairSwapActionExample),
    Multi(MultiSwapActionExample),
    Crowd(CrowdSwapActionExample),
}

use PolySwapActionExample::*;

impl common::SwapAction<AccountId, TechAccountId, Testtime> for PolySwapActionExample {
    fn reserve(&self, source: &AccountId) -> dispatch::DispatchResult {
        match self {
            GenericPair(a) => a.reserve(source),
            Multi(a) => a.reserve(source),
            Crowd(a) => a.reserve(source),
        }
    }
    fn claim(&self, source: &AccountId) -> bool {
        match self {
            GenericPair(a) => a.claim(source),
            Multi(a) => a.claim(source),
            Crowd(a) => a.claim(source),
        }
    }
    fn weight(&self) -> Weight {
        match self {
            GenericPair(a) => a.weight(),
            Multi(a) => a.weight(),
            Crowd(a) => a.weight(),
        }
    }
    fn cancel(&self, source: &AccountId) {
        match self {
            GenericPair(a) => a.cancel(source),
            Multi(a) => a.cancel(source),
            Crowd(a) => a.cancel(source),
        }
    }
}

impl common::SwapRulesValidation<AccountId, TechAccountId, Testtime> for PolySwapActionExample {
    fn validate(&self, source: &AccountId) -> bool {
        match self {
            GenericPair(a) => a.validate(source),
            Multi(a) => a.validate(source),
            Crowd(a) => a.validate(source),
        }
    }
    fn instant_auto_claim_used(&self) -> bool {
        match self {
            GenericPair(a) => a.instant_auto_claim_used(),
            Multi(a) => a.instant_auto_claim_used(),
            Crowd(a) => a.instant_auto_claim_used(),
        }
    }
    fn triggered_auto_claim_used(&self) -> bool {
        match self {
            GenericPair(a) => a.triggered_auto_claim_used(),
            Multi(a) => a.triggered_auto_claim_used(),
            Crowd(a) => a.triggered_auto_claim_used(),
        }
    }
    fn is_able_to_claim(&self) -> bool {
        match self {
            GenericPair(a) => a.is_able_to_claim(),
            Multi(a) => a.is_able_to_claim(),
            Crowd(a) => a.is_able_to_claim(),
        }
    }
}

type TechAssetId = common::TechAssetId<AssetId, DEXId>;
type TechAccountIdPrimitive = common::TechAccountId<AccountId, AssetId, DEXId>;
type TechAccountId = crate::TechAccountIdReprCompat<Testtime, TechAccountIdPrimitive>;
type TechAmount = Amount;
type TechBalance = Balance;

impl Trait for Testtime {
    type Event = ();
    type TechAssetId = TechAssetId;
    type TechAccountIdPrimitive = TechAccountIdPrimitive;
    type TechAmount = TechAmount;
    type TechBalance = TechBalance;
    type Trigger = ();
    type Condition = ();
    type SwapAction = PolySwapActionExample;
}

impl ExtBuilder {
    pub fn build(self) -> sp_io::TestExternalities {
        let mut t = system::GenesisConfig::default()
            .build_storage::<Testtime>()
            .unwrap();

        tokens::GenesisConfig::<Testtime> {
            endowed_accounts: self.endowed_accounts,
        }
        .assimilate_storage(&mut t)
        .unwrap();

        t.into()
    }
}

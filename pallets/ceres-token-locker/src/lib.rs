#![cfg_attr(not(feature = "std"), no_std)]

pub mod weights;

mod benchmarking;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

use codec::{Decode, Encode};
use frame_support::weights::Weight;

pub trait WeightInfo {
    fn lock_tokens() -> Weight;
    fn withdraw_tokens() -> Weight;
    fn change_fee() -> Weight;
}

#[derive(Encode, Decode, Default, PartialEq, Eq, scale_info::TypeInfo)]
#[cfg_attr(feature = "std", derive(Debug))]
pub struct TokenLockInfo<Balance, BlockNumber, AssetId> {
    /// Amount of locked tokens
    pub tokens: Balance,
    /// The time (block height) at which the tokens will be unlocked
    pub unlocking_block: BlockNumber,
    /// Locked asset id
    pub asset_id: AssetId,
}

pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
    use crate::{TokenLockInfo, WeightInfo};
    use common::balance;
    use common::prelude::{Balance, FixedWrapper};
    use frame_support::pallet_prelude::*;
    use frame_support::PalletId;
    use frame_system::ensure_signed;
    use frame_system::pallet_prelude::*;
    use hex_literal::hex;
    use sp_runtime::traits::AccountIdConversion;
    use sp_std::vec::Vec;

    const PALLET_ID: PalletId = PalletId(*b"crstlock");

    #[pallet::config]
    pub trait Config: frame_system::Config + assets::Config + technical::Config {
        /// Because this pallet emits events, it depends on the runtime's definition of an event.
        type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;

        /// Ceres asset id
        type CeresAssetId: Get<Self::AssetId>;

        /// Weight information for extrinsics in this pallet.
        type WeightInfo: WeightInfo;
    }

    type Assets<T> = assets::Pallet<T>;
    pub type AccountIdOf<T> = <T as frame_system::Config>::AccountId;
    type AssetIdOf<T> = <T as assets::Config>::AssetId;

    #[pallet::pallet]
    #[pallet::generate_store(pub (super) trait Store)]
    #[pallet::without_storage_info]
    pub struct Pallet<T>(PhantomData<T>);

    #[pallet::type_value]
    pub fn DefaultForFeesAccount<T: Config>() -> AccountIdOf<T> {
        let bytes = hex!("96ea3c9c0be7bbc7b0656a1983db5eed75210256891a9609012362e36815b132");
        AccountIdOf::<T>::decode(&mut &bytes[..]).unwrap()
    }

    /// Account for collecting fees
    #[pallet::storage]
    #[pallet::getter(fn fees_account)]
    pub type FeesAccount<T: Config> =
        StorageValue<_, AccountIdOf<T>, ValueQuery, DefaultForFeesAccount<T>>;

    #[pallet::type_value]
    pub fn DefaultForAuthorityAccount<T: Config>() -> AccountIdOf<T> {
        let bytes = hex!("0a0455d92e1fda8dee17b2c58761c8efca490ef2a1a03322dbfea7379481d517");
        AccountIdOf::<T>::decode(&mut &bytes[..]).unwrap()
    }

    /// Account which has permissions for changing fee
    #[pallet::storage]
    #[pallet::getter(fn authority_account)]
    pub type AuthorityAccount<T: Config> =
        StorageValue<_, AccountIdOf<T>, ValueQuery, DefaultForAuthorityAccount<T>>;

    #[pallet::type_value]
    pub fn DefaultForFeeAmount<T: Config>() -> Balance {
        balance!(0.005)
    }

    /// Amount of CERES for locker fees option two
    #[pallet::storage]
    #[pallet::getter(fn fee_amount)]
    pub type FeeAmount<T: Config> = StorageValue<_, Balance, ValueQuery, DefaultForFeeAmount<T>>;

    #[pallet::storage]
    #[pallet::getter(fn locker_data)]
    pub type TokenLockerData<T: Config> = StorageMap<
        _,
        Identity,
        AccountIdOf<T>,
        Vec<TokenLockInfo<Balance, T::BlockNumber, AssetIdOf<T>>>,
        ValueQuery,
    >;

    #[pallet::event]
    #[pallet::generate_deposit(pub (super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// Funds Locked [who, amount, asset]
        Locked(AccountIdOf<T>, Balance, AssetIdOf<T>),
        /// Funds Withdrawn [who, amount, asset]
        Withdrawn(AccountIdOf<T>, Balance, AssetIdOf<T>),
        /// Fee Changed [who, amount]
        FeeChanged(AccountIdOf<T>, Balance),
    }

    #[pallet::error]
    pub enum Error<T> {
        /// Number of tokens equals zero
        InvalidNumberOfTokens,
        /// Unauthorized access
        Unauthorized,
        /// Block number in past,
        InvalidUnlockingBlock,
        /// Not enough funds
        NotEnoughFunds,
        /// Tokens not unlocked yet
        NotUnlockedYet,
        /// Lock info does not exist
        LockInfoDoesNotExist,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Lock tokens
        #[pallet::weight(<T as Config>::WeightInfo::lock_tokens())]
        pub fn lock_tokens(
            origin: OriginFor<T>,
            asset_id: AssetIdOf<T>,
            unlocking_block: T::BlockNumber,
            number_of_tokens: Balance,
        ) -> DispatchResultWithPostInfo {
            let user = ensure_signed(origin)?;
            ensure!(
                number_of_tokens > balance!(0),
                Error::<T>::InvalidNumberOfTokens
            );

            // Get current block
            let current_block = frame_system::Pallet::<T>::block_number();
            ensure!(
                unlocking_block > current_block,
                Error::<T>::InvalidUnlockingBlock
            );

            let token_lock_info = TokenLockInfo {
                tokens: number_of_tokens,
                unlocking_block,
                asset_id,
            };

            let fee = (FixedWrapper::from(number_of_tokens)
                * FixedWrapper::from(FeeAmount::<T>::get()))
            .try_into_balance()
            .unwrap_or(0);
            let total = number_of_tokens + fee;

            ensure!(
                total <= Assets::<T>::free_balance(&asset_id, &user).unwrap_or(0),
                Error::<T>::NotEnoughFunds
            );

            // Transfer tokens
            Assets::<T>::transfer_from(&asset_id, &user, &Self::account_id(), number_of_tokens)?;

            // Pay fees
            Assets::<T>::transfer_from(&asset_id, &user, &FeesAccount::<T>::get(), fee)?;

            <TokenLockerData<T>>::append(&user, token_lock_info);

            // Emit an event
            Self::deposit_event(Event::Locked(user, number_of_tokens, asset_id));

            // Return a successful DispatchResult
            Ok(().into())
        }

        /// Withdraw tokens
        #[pallet::weight(<T as Config>::WeightInfo::withdraw_tokens())]
        pub fn withdraw_tokens(
            origin: OriginFor<T>,
            asset_id: AssetIdOf<T>,
            unlocking_block: T::BlockNumber,
            number_of_tokens: Balance,
        ) -> DispatchResultWithPostInfo {
            let user = ensure_signed(origin)?;
            ensure!(
                number_of_tokens > balance!(0),
                Error::<T>::InvalidNumberOfTokens
            );

            // Get current block
            let current_block = frame_system::Pallet::<T>::block_number();
            ensure!(unlocking_block < current_block, Error::<T>::NotUnlockedYet);

            let mut token_lock_info_vec = <TokenLockerData<T>>::get(&user);
            let mut idx = 0;
            let mut exist = false;
            for (index, lock) in token_lock_info_vec.iter().enumerate() {
                if lock.unlocking_block == unlocking_block
                    && lock.asset_id == asset_id
                    && lock.tokens == number_of_tokens
                {
                    idx = index;
                    exist = true;
                    break;
                }
            }

            if !exist {
                return Err(Error::<T>::LockInfoDoesNotExist.into());
            }

            // Withdraw tokens
            Assets::<T>::transfer_from(&asset_id, &Self::account_id(), &user, number_of_tokens)?;

            token_lock_info_vec.remove(idx);
            <TokenLockerData<T>>::insert(&user, token_lock_info_vec);

            // Emit an event
            Self::deposit_event(Event::Withdrawn(user, number_of_tokens, asset_id));

            // Return a successful DispatchResult
            Ok(().into())
        }

        /// Change fee
        #[pallet::weight(<T as Config>::WeightInfo::change_fee())]
        pub fn change_fee(origin: OriginFor<T>, new_fee: Balance) -> DispatchResultWithPostInfo {
            let user = ensure_signed(origin)?;

            if user != AuthorityAccount::<T>::get() {
                return Err(Error::<T>::Unauthorized.into());
            }

            FeeAmount::<T>::put(new_fee);

            // Emit an event
            Self::deposit_event(Event::FeeChanged(user, new_fee));

            Ok(().into())
        }
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

    impl<T: Config> Pallet<T> {
        /// The account ID of pallet
        fn account_id() -> T::AccountId {
            PALLET_ID.into_account_truncating()
        }
    }
}

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

#![cfg_attr(not(feature = "std"), no_std)]
#![allow(dead_code)] // todo (m.tagirov) remove

use assets::AssetIdOf;
use common::prelude::{
    EnsureTradingPairExists, FixedWrapper, QuoteAmount, SwapAmount, SwapOutcome, TradingPair,
};
#[cfg(feature = "wip")] // order-book
use common::LiquiditySourceType;
use common::{
    balance, AssetInfoProvider, AssetName, AssetSymbol, Balance, BalancePrecision, ContentSource,
    Description, DexInfoProvider, LiquiditySource, PriceVariant, RewardReason,
    ToOrderTechUnitFromDEXAndTradingPair, TradingPairSourceManager,
};
use core::fmt::Debug;
use frame_support::ensure;
use frame_support::sp_runtime::DispatchError;
use frame_support::traits::{Get, Time};
use frame_support::weights::{Weight, WeightMeter};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_runtime::traits::{AtLeast32BitUnsigned, MaybeDisplay, Zero};
use sp_runtime::{BoundedVec, Perbill};
use sp_std::collections::btree_map::BTreeMap;
use sp_std::vec::Vec;

pub mod weights;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

pub mod cache_data_layer;
mod limit_order;
mod market_order;
mod order_book;
mod scheduler;
pub mod storage_data_layer;
pub mod traits;
pub mod types;

pub use crate::order_book::OrderBook;
use cache_data_layer::CacheDataLayer;
pub use limit_order::LimitOrder;
pub use market_order::MarketOrder;
pub use traits::{CurrencyLocker, CurrencyUnlocker, DataLayer, ExpirationScheduler};
pub use types::{
    DealInfo, MarketChange, MarketRole, MarketSide, OrderAmount, OrderBookId, OrderBookStatus,
    OrderPrice, OrderVolume, Payment, PriceOrders, UserOrders,
};
pub use weights::WeightInfo;

pub use pallet::*;

pub type MomentOf<T> = <<T as Config>::Time as Time>::Moment;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use common::DEXInfo;
    use frame_support::{
        pallet_prelude::{OptionQuery, *},
        traits::Hooks,
        Blake2_128Concat, Twox128,
    };
    use frame_system::pallet_prelude::*;
    use sp_runtime::Either;

    const STORAGE_VERSION: StorageVersion = StorageVersion::new(0);

    #[pallet::pallet]
    #[pallet::generate_store(pub(super) trait Store)]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(PhantomData<T>);

    #[pallet::config]
    pub trait Config: frame_system::Config + assets::Config + technical::Config {
        const MAX_ORDER_LIFETIME: MomentOf<Self>;
        const MIN_ORDER_LIFETIME: MomentOf<Self>;
        const MILLISECS_PER_BLOCK: MomentOf<Self>;
        const MAX_PRICE_SHIFT: Perbill;

        /// Because this pallet emits events, it depends on the runtime's definition of an event.
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        type OrderId: Parameter
            + Member
            + MaybeSerializeDeserialize
            + Debug
            + MaybeDisplay
            + AtLeast32BitUnsigned
            + Copy
            + Ord
            + PartialEq
            + Eq
            + MaxEncodedLen
            + scale_info::TypeInfo;
        type MaxOpenedLimitOrdersPerUser: Get<u32>;
        type MaxLimitOrdersForPrice: Get<u32>;
        type MaxSidePriceCount: Get<u32>;
        type MaxExpiringOrdersPerBlock: Get<u32>;
        type MaxExpirationWeightPerBlock: Get<Weight>;
        type EnsureTradingPairExists: EnsureTradingPairExists<
            Self::DEXId,
            Self::AssetId,
            DispatchError,
        >;
        type TradingPairSourceManager: TradingPairSourceManager<Self::DEXId, Self::AssetId>;
        type AssetInfoProvider: AssetInfoProvider<
            Self::AssetId,
            Self::AccountId,
            AssetSymbol,
            AssetName,
            BalancePrecision,
            ContentSource,
            Description,
        >;
        type DexInfoProvider: DexInfoProvider<Self::DEXId, DEXInfo<Self::AssetId>>;
        type Time: Time;
        type ParameterUpdateOrigin: EnsureOrigin<
            Self::RuntimeOrigin,
            Success = Either<Self::AccountId, ()>,
        >;
        type StatusUpdateOrigin: EnsureOrigin<Self::RuntimeOrigin, Success = ()>;
        type RemovalOrigin: EnsureOrigin<Self::RuntimeOrigin, Success = ()>;
        type WeightInfo: WeightInfo;
    }

    #[pallet::storage]
    #[pallet::getter(fn order_books)]
    pub type OrderBooks<T: Config> =
        StorageMap<_, Blake2_128Concat, OrderBookId<AssetIdOf<T>>, OrderBook<T>, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn limit_orders)]
    pub type LimitOrders<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        OrderBookId<AssetIdOf<T>>,
        Blake2_128Concat,
        T::OrderId,
        LimitOrder<T>,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn bids)]
    pub type Bids<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        OrderBookId<AssetIdOf<T>>,
        Blake2_128Concat,
        OrderPrice,
        PriceOrders<T::OrderId, T::MaxLimitOrdersForPrice>,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn asks)]
    pub type Asks<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        OrderBookId<AssetIdOf<T>>,
        Blake2_128Concat,
        OrderPrice,
        PriceOrders<T::OrderId, T::MaxLimitOrdersForPrice>,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn aggregated_bids)]
    pub type AggregatedBids<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        OrderBookId<AssetIdOf<T>>,
        MarketSide<T::MaxSidePriceCount>,
        ValueQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn aggregated_asks)]
    pub type AggregatedAsks<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        OrderBookId<AssetIdOf<T>>,
        MarketSide<T::MaxSidePriceCount>,
        ValueQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn user_limit_orders)]
    pub type UserLimitOrders<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Blake2_128Concat,
        OrderBookId<AssetIdOf<T>>,
        UserOrders<T::OrderId, T::MaxOpenedLimitOrdersPerUser>,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn expired_orders_at)]
    pub type ExpirationsAgenda<T: Config> = StorageMap<
        _,
        Twox128,
        T::BlockNumber,
        BoundedVec<(OrderBookId<AssetIdOf<T>>, T::OrderId), T::MaxExpiringOrdersPerBlock>,
        ValueQuery,
    >;

    /// Earliest block with incomplete expirations;
    /// Weight limit might not allow to finish all expirations for a block, so
    /// they might be operated later.
    #[pallet::storage]
    #[pallet::getter(fn incomplete_expirations_since)]
    pub type IncompleteExpirationsSince<T: Config> = StorageValue<_, T::BlockNumber>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// New order book is created by user
        OrderBookCreated {
            order_book_id: OrderBookId<AssetIdOf<T>>,
            dex_id: T::DEXId,
            creator: T::AccountId,
        },

        /// Order book is deleted
        OrderBookDeleted {
            order_book_id: OrderBookId<AssetIdOf<T>>,
            dex_id: T::DEXId,
            count_of_canceled_orders: u32,
        },

        /// Order book status is changed
        OrderBookStatusChanged {
            order_book_id: OrderBookId<AssetIdOf<T>>,
            dex_id: T::DEXId,
            new_status: OrderBookStatus,
        },

        /// Order book attributes are updated
        OrderBookUpdated {
            order_book_id: OrderBookId<AssetIdOf<T>>,
            dex_id: T::DEXId,
        },

        /// User placed new limit order
        LimitOrderPlaced {
            order_book_id: OrderBookId<AssetIdOf<T>>,
            dex_id: T::DEXId,
            order_id: T::OrderId,
            owner_id: T::AccountId,
        },

        /// User tried to place the limit order out of the spread. The limit order is converted into a market order.
        LimitOrderConvertedToMarketOrder {
            order_book_id: OrderBookId<AssetIdOf<T>>,
            dex_id: T::DEXId,
            owner_id: T::AccountId,
        },

        /// User tried to place the limit order out of the spread.
        /// One part of the liquidity of the limit order is converted into a market order, and the other part is placed as a limit order.
        LimitOrderIsSplitIntoMarketOrderAndLimitOrder {
            order_book_id: OrderBookId<AssetIdOf<T>>,
            dex_id: T::DEXId,
            owner_id: T::AccountId,
            market_order_input: OrderAmount,
            limit_order_id: T::OrderId,
            limit_order_input: OrderAmount,
        },

        /// User canceled their limit order
        LimitOrderCanceled {
            order_book_id: OrderBookId<AssetIdOf<T>>,
            dex_id: T::DEXId,
            order_id: T::OrderId,
            owner_id: T::AccountId,
        },

        /// The order has reached the end of its lifespan
        LimitOrderExpired {
            order_book_id: OrderBookId<AssetIdOf<T>>,
            dex_id: T::DEXId,
            order_id: T::OrderId,
            owner_id: T::AccountId,
        },

        /// Failed to cancel expired order
        ExpirationFailure {
            order_book_id: OrderBookId<AssetIdOf<T>>,
            order_id: T::OrderId,
            error: DispatchError,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// Order book does not exist for this trading pair
        UnknownOrderBook,
        /// Invalid order book id
        InvalidOrderBookId,
        /// Order book already exists for this trading pair
        OrderBookAlreadyExists,
        /// Limit order does not exist for this trading pair and order id
        UnknownLimitOrder,
        /// Limit order already exists for this trading pair and order id
        LimitOrderAlreadyExists,
        /// It is impossible to insert the limit order because the bounds have been reached
        LimitOrderStorageOverflow,
        /// It is impossible to update the limit order
        UpdateLimitOrderError,
        /// It is impossible to delete the limit order
        DeleteLimitOrderError,
        /// Expiration schedule for expiration block is full
        BlockScheduleFull,
        /// Could not find expiration in given block schedule
        ExpirationNotFound,
        /// There are no bids/asks for the price
        NoDataForPrice,
        /// There are no aggregated bids/asks for the order book
        NoAggregatedData,
        /// There is not enough liquidity in the order book to cover the deal
        NotEnoughLiquidityInOrderBook,
        /// Cannot create order book with equal base and target assets
        ForbiddenToCreateOrderBookWithSameAssets,
        /// The asset is not allowed to be base. Only dex base asset can be a quote asset for order book
        NotAllowedBaseAsset,
        /// Orderbooks cannot be created with given dex id.
        NotAllowedDEXId,
        /// User cannot create an order book with NFT if they don't have NFT
        UserHasNoNft,
        /// Lifespan exceeds defined limits
        InvalidLifespan,
        /// The order amount (limit or market) does not meet the requirements
        InvalidOrderAmount,
        /// The limit order price does not meet the requirements
        InvalidLimitOrderPrice,
        /// User cannot set the price of limit order too far from actual market price
        LimitOrderPriceIsTooFarFromSpread,
        /// At the moment, Trading is forbidden in the current order book
        TradingIsForbidden,
        /// At the moment, Users cannot place new limit orders in the current order book
        PlacementOfLimitOrdersIsForbidden,
        /// At the moment, Users cannot cancel their limit orders in the current order book
        CancellationOfLimitOrdersIsForbidden,
        /// User has the max available count of open limit orders in the current order book
        UserHasMaxCountOfOpenedOrders,
        /// It is impossible to place the limit order because bounds of the max count of orders at the current price have been reached
        PriceReachedMaxCountOfLimitOrders,
        /// It is impossible to place the limit order because bounds of the max count of prices for the side have been reached
        OrderBookReachedMaxCountOfPricesForSide,
        /// An error occurred while calculating the amount
        AmountCalculationFailed,
        /// An error occurred while calculating the price
        PriceCalculationFailed,
        /// Unauthorized action
        Unauthorized,
        /// Invalid asset
        InvalidAsset,
        /// Invalid tick size
        InvalidTickSize,
        /// Invalid step lot size
        InvalidStepLotSize,
        /// Invalid min lot size
        InvalidMinLotSize,
        /// Invalid max lot size
        InvalidMaxLotSize,
        /// Tick size & step lot size are too big and their multiplication overflows Balance
        TickSizeAndStepLotSizeAreTooBig,
        /// Tick size & step lot size are too small and their multiplication goes out of precision
        TickSizeAndStepLotSizeAreTooSmall,
        /// Max lot size cannot be more that total supply of base asset
        MaxLotSizeIsMoreThanTotalSupply,
        /// Indicated limit for slippage has not been met during transaction execution.
        SlippageLimitExceeded,
        /// NFT order books are temporarily forbidden
        NftOrderBooksAreTemporarilyForbidden,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        /// Perform scheduled expirations
        fn on_initialize(current_block: T::BlockNumber) -> Weight {
            let mut weight_counter = WeightMeter::from_limit(T::MaxExpirationWeightPerBlock::get());
            Self::service(current_block, &mut weight_counter);
            weight_counter.consumed
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::create_orderbook())]
        pub fn create_orderbook(
            origin: OriginFor<T>,
            dex_id: T::DEXId,
            order_book_id: OrderBookId<AssetIdOf<T>>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            ensure!(
                order_book_id.base != order_book_id.quote,
                Error::<T>::ForbiddenToCreateOrderBookWithSameAssets
            );
            ensure!(
                dex_id == common::DEXId::Polkaswap.into(),
                Error::<T>::NotAllowedDEXId
            );
            let dex_info = T::DexInfoProvider::get_dex_info(&dex_id)?;
            // the base asset of DEX must be a quote asset of order book
            ensure!(
                order_book_id.quote == dex_info.base_asset_id,
                Error::<T>::NotAllowedBaseAsset
            );
            T::AssetInfoProvider::ensure_asset_exists(&order_book_id.base)?;
            T::EnsureTradingPairExists::ensure_trading_pair_exists(
                &dex_id,
                &order_book_id.quote.into(),
                &order_book_id.base.into(),
            )?;
            ensure!(
                !<OrderBooks<T>>::contains_key(order_book_id),
                Error::<T>::OrderBookAlreadyExists
            );

            let order_book = if T::AssetInfoProvider::is_non_divisible(&order_book_id.base) {
                // temp solution for stage env
                // will be removed in #542
                // todo (m.tagirov)
                return Err(Error::<T>::NftOrderBooksAreTemporarilyForbidden.into());

                #[allow(unreachable_code)]
                {
                    // nft
                    // ensure the user has nft
                    ensure!(
                        T::AssetInfoProvider::total_balance(&order_book_id.base, &who)?
                            > Balance::zero(),
                        Error::<T>::UserHasNoNft
                    );
                    OrderBook::<T>::default_nft(order_book_id, dex_id)
                }
            } else {
                // regular asset
                OrderBook::<T>::default(order_book_id, dex_id)
            };

            #[cfg(feature = "wip")] // order-book
            {
                T::TradingPairSourceManager::enable_source_for_trading_pair(
                    &dex_id,
                    &order_book_id.quote,
                    &order_book_id.base,
                    LiquiditySourceType::OrderBook,
                )?;
            }

            <OrderBooks<T>>::insert(order_book_id, order_book);
            Self::register_tech_account(dex_id, order_book_id)?;

            Self::deposit_event(Event::<T>::OrderBookCreated {
                order_book_id,
                dex_id,
                creator: who,
            });
            Ok(().into())
        }

        #[pallet::call_index(1)]
        #[pallet::weight(<T as Config>::WeightInfo::delete_orderbook())]
        pub fn delete_orderbook(
            origin: OriginFor<T>,
            order_book_id: OrderBookId<AssetIdOf<T>>,
        ) -> DispatchResult {
            T::RemovalOrigin::ensure_origin(origin)?;
            let order_book =
                <OrderBooks<T>>::get(order_book_id).ok_or(Error::<T>::UnknownOrderBook)?;
            let dex_id = order_book.dex_id;

            let mut data = CacheDataLayer::<T>::new();
            let count_of_canceled_orders =
                order_book.cancel_all_limit_orders::<Self, Self, Self>(&mut data)? as u32;

            data.commit();

            #[cfg(feature = "wip")] // order-book
            {
                T::TradingPairSourceManager::disable_source_for_trading_pair(
                    &dex_id,
                    &order_book_id.quote,
                    &order_book_id.base,
                    LiquiditySourceType::OrderBook,
                )?;
            }

            Self::deregister_tech_account(order_book.dex_id, order_book_id)?;
            <OrderBooks<T>>::remove(order_book_id);

            Self::deposit_event(Event::<T>::OrderBookDeleted {
                order_book_id,
                dex_id,
                count_of_canceled_orders,
            });
            Ok(().into())
        }

        #[pallet::call_index(2)]
        #[pallet::weight(<T as Config>::WeightInfo::update_orderbook())]
        pub fn update_orderbook(
            origin: OriginFor<T>,
            order_book_id: OrderBookId<AssetIdOf<T>>,
            tick_size: OrderPrice,
            step_lot_size: OrderVolume,
            min_lot_size: OrderVolume,
            max_lot_size: OrderVolume,
        ) -> DispatchResult {
            let origin_check_result = T::ParameterUpdateOrigin::ensure_origin(origin)?;
            match origin_check_result {
                Either::Left(who) => {
                    ensure!(
                        T::AssetInfoProvider::is_asset_owner(&order_book_id.base, &who),
                        DispatchError::BadOrigin
                    );
                }
                Either::Right(()) => (),
            }
            let mut order_book =
                <OrderBooks<T>>::get(order_book_id).ok_or(Error::<T>::UnknownOrderBook)?;
            let dex_id = order_book.dex_id;

            // Check that values are non-zero
            ensure!(tick_size > OrderPrice::zero(), Error::<T>::InvalidTickSize);
            ensure!(
                step_lot_size > OrderVolume::zero(),
                Error::<T>::InvalidStepLotSize
            );
            ensure!(
                min_lot_size > OrderVolume::zero(),
                Error::<T>::InvalidMinLotSize
            );
            ensure!(
                max_lot_size > OrderVolume::zero(),
                Error::<T>::InvalidMaxLotSize
            );

            // min <= max
            // It is possible to set min == max if it necessary, e.g. some NFTs
            ensure!(min_lot_size <= max_lot_size, Error::<T>::InvalidMaxLotSize);

            if T::AssetInfoProvider::is_non_divisible(&order_book_id.base) {
                // NFT has special bounds as non-divisible asset
                ensure!(step_lot_size >= balance!(1), Error::<T>::InvalidStepLotSize);
                ensure!(
                    step_lot_size % balance!(1) == 0,
                    Error::<T>::InvalidStepLotSize
                );
            }

            // min & max couldn't be less then `step_lot_size`
            ensure!(min_lot_size >= step_lot_size, Error::<T>::InvalidMinLotSize);
            ensure!(max_lot_size >= step_lot_size, Error::<T>::InvalidMaxLotSize);

            // min & max must be a multiple of `step_lot_size`
            ensure!(
                min_lot_size % step_lot_size == 0,
                Error::<T>::InvalidMinLotSize
            );
            ensure!(
                max_lot_size % step_lot_size == 0,
                Error::<T>::InvalidMaxLotSize
            );

            // Even if `tick_size` & `step_lot_size` meet precision conditions the min possible deal amount could not match.
            // The min possible deal amount = `tick_size` * `step_lot_size`.
            // We need to be sure that the value doesn't overflow Balance if `tick_size` & `step_lot_size` are too big
            // and doesn't go out of precision if `tick_size` & `step_lot_size` are too small.

            // Returns error if value overflows.
            let min_possible_deal_amount = (FixedWrapper::from(tick_size)
                * FixedWrapper::from(step_lot_size))
            .try_into_balance()
            .map_err(|_| Error::<T>::TickSizeAndStepLotSizeAreTooBig)?;

            // 1 is a min non-zero possible value. balance!(0.000000000000000001) == 1
            // If `tick_size` * `step_lot_size` result goes out of 18 digits precision, the min possible deal amount == 0,
            // because FixedWrapper::try_into_balance() returns 0 for such cases.
            ensure!(
                min_possible_deal_amount >= 1,
                Error::<T>::TickSizeAndStepLotSizeAreTooSmall
            );

            // `max_lot_size` couldn't be more then total supply of `base` asset
            let total_supply = T::AssetInfoProvider::total_issuance(&order_book_id.base)?;
            ensure!(
                max_lot_size <= total_supply,
                Error::<T>::MaxLotSizeIsMoreThanTotalSupply
            );

            order_book.tick_size = tick_size;
            order_book.step_lot_size = step_lot_size;
            order_book.min_lot_size = min_lot_size;
            order_book.max_lot_size = max_lot_size;

            // Note:
            // Already existed limit orders are not changed even if they don't meet the requirements of new attributes.
            // They stay in order book until they are executed, canceled or expired.
            // All new limit orders must meet the requirements of new attributes.

            <OrderBooks<T>>::set(order_book_id, Some(order_book));
            Self::deposit_event(Event::<T>::OrderBookUpdated {
                order_book_id,
                dex_id,
            });
            Ok(().into())
        }

        #[pallet::call_index(3)]
        #[pallet::weight(<T as Config>::WeightInfo::change_orderbook_status())]
        pub fn change_orderbook_status(
            origin: OriginFor<T>,
            order_book_id: OrderBookId<AssetIdOf<T>>,
            status: OrderBookStatus,
        ) -> DispatchResult {
            T::StatusUpdateOrigin::ensure_origin(origin)?;
            let dex_id = <OrderBooks<T>>::mutate(order_book_id, |order_book| {
                let order_book = order_book.as_mut().ok_or(Error::<T>::UnknownOrderBook)?;
                order_book.status = status;
                Ok::<_, Error<T>>(order_book.dex_id)
            })?;
            Self::deposit_event(Event::<T>::OrderBookStatusChanged {
                order_book_id,
                dex_id,
                new_status: status,
            });
            Ok(().into())
        }

        #[pallet::call_index(4)]
        #[pallet::weight(<T as Config>::WeightInfo::place_limit_order())]
        pub fn place_limit_order(
            origin: OriginFor<T>,
            order_book_id: OrderBookId<AssetIdOf<T>>,
            price: OrderPrice,
            amount: OrderVolume,
            side: PriceVariant,
            lifespan: Option<MomentOf<T>>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            let mut order_book =
                <OrderBooks<T>>::get(order_book_id).ok_or(Error::<T>::UnknownOrderBook)?;
            let dex_id = order_book.dex_id;
            let order_id = order_book.next_order_id();
            let now = T::Time::now();
            let current_block = frame_system::Pallet::<T>::block_number();
            let lifespan = lifespan.unwrap_or(T::MAX_ORDER_LIFETIME);
            let order = LimitOrder::<T>::new(
                order_id,
                who.clone(),
                side,
                price,
                amount,
                now,
                lifespan,
                current_block,
            );

            let mut data = CacheDataLayer::<T>::new();
            let (market_input, deal_input) =
                order_book.place_limit_order::<Self, Self, Self>(order, &mut data)?;

            data.commit();
            <OrderBooks<T>>::insert(order_book_id, order_book);

            match (market_input, deal_input) {
                (None, Some(..)) => {
                    Self::deposit_event(Event::<T>::LimitOrderConvertedToMarketOrder {
                        order_book_id,
                        dex_id,
                        owner_id: who,
                    })
                }
                (Some(..), None) => Self::deposit_event(Event::<T>::LimitOrderPlaced {
                    order_book_id,
                    dex_id,
                    order_id,
                    owner_id: who,
                }),
                (Some(limit_order_input), Some(market_order_input)) => {
                    Self::deposit_event(Event::<T>::LimitOrderIsSplitIntoMarketOrderAndLimitOrder {
                        order_book_id,
                        dex_id,
                        owner_id: who,
                        market_order_input,
                        limit_order_id: order_id,
                        limit_order_input,
                    })
                }
                _ => {
                    // should never happen
                    return Err(Error::<T>::InvalidOrderAmount.into());
                }
            }
            Ok(().into())
        }

        #[pallet::call_index(5)]
        #[pallet::weight(<T as Config>::WeightInfo::cancel_limit_order())]
        pub fn cancel_limit_order(
            origin: OriginFor<T>,
            order_book_id: OrderBookId<AssetIdOf<T>>,
            order_id: T::OrderId,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            let mut data = CacheDataLayer::<T>::new();
            let order = data.get_limit_order(&order_book_id, order_id)?;

            ensure!(order.owner == who, Error::<T>::Unauthorized);

            let order_book =
                <OrderBooks<T>>::get(order_book_id).ok_or(Error::<T>::UnknownOrderBook)?;
            let dex_id = order_book.dex_id;

            order_book.cancel_limit_order::<Self, Self, Self>(order, &mut data)?;
            data.commit();
            Self::deposit_event(Event::<T>::LimitOrderCanceled {
                order_book_id,
                dex_id,
                order_id,
                owner_id: who,
            });
            Ok(().into())
        }
    }
}

impl<T: Config> CurrencyLocker<T::AccountId, T::AssetId, T::DEXId, DispatchError> for Pallet<T> {
    fn lock_liquidity(
        dex_id: T::DEXId,
        account: &T::AccountId,
        order_book_id: OrderBookId<T::AssetId>,
        asset_id: &T::AssetId,
        amount: OrderVolume,
    ) -> Result<(), DispatchError> {
        let tech_account = Self::tech_account_for_order_book(dex_id, order_book_id);
        technical::Pallet::<T>::transfer_in(asset_id, account, &tech_account, amount.into())
    }
}

impl<T: Config> CurrencyUnlocker<T::AccountId, T::AssetId, T::DEXId, DispatchError> for Pallet<T> {
    fn unlock_liquidity(
        dex_id: T::DEXId,
        account: &T::AccountId,
        order_book_id: OrderBookId<T::AssetId>,
        asset_id: &T::AssetId,
        amount: OrderVolume,
    ) -> Result<(), DispatchError> {
        let tech_account = Self::tech_account_for_order_book(dex_id, order_book_id);
        technical::Pallet::<T>::transfer_out(asset_id, &tech_account, account, amount.into())
    }

    fn unlock_liquidity_batch(
        dex_id: T::DEXId,
        order_book_id: OrderBookId<T::AssetId>,
        asset_id: &T::AssetId,
        receivers: &BTreeMap<T::AccountId, OrderVolume>,
    ) -> Result<(), DispatchError> {
        let tech_account = Self::tech_account_for_order_book(dex_id, order_book_id);
        for (account, amount) in receivers.iter() {
            technical::Pallet::<T>::transfer_out(asset_id, &tech_account, account, *amount)?;
        }
        Ok(())
    }
}

impl<T: Config> Pallet<T> {
    pub fn tech_account_for_order_book(
        dex_id: T::DEXId,
        order_book_id: OrderBookId<AssetIdOf<T>>,
    ) -> <T as technical::Config>::TechAccountId {
        let trading_pair: TradingPair<AssetIdOf<T>> = order_book_id.into();
        // Same as in xyk accounts
        let tech_pair = trading_pair.map(|a| a.into());
        <T as technical::Config>::TechAccountId::to_order_tech_unit_from_dex_and_trading_pair(
            dex_id, tech_pair,
        )
    }

    /// Validity of asset ids (for example, to have the same base asset
    /// for dex and pair) should be done beforehand
    pub fn register_tech_account(
        dex_id: T::DEXId,
        order_book_id: OrderBookId<AssetIdOf<T>>,
    ) -> Result<(), DispatchError> {
        let tech_account = Self::tech_account_for_order_book(dex_id, order_book_id);
        technical::Pallet::<T>::register_tech_account_id(tech_account)
    }

    /// Validity of asset ids (for example, to have the same base asset
    /// for dex and pair) should be done beforehand
    pub fn deregister_tech_account(
        dex_id: T::DEXId,
        order_book_id: OrderBookId<AssetIdOf<T>>,
    ) -> Result<(), DispatchError> {
        let tech_account = Self::tech_account_for_order_book(dex_id, order_book_id);
        technical::Pallet::<T>::deregister_tech_account_id(tech_account)
    }

    pub fn assemble_order_book_id(
        dex_id: &T::DEXId,
        input_asset_id: &AssetIdOf<T>,
        output_asset_id: &AssetIdOf<T>,
    ) -> Option<OrderBookId<AssetIdOf<T>>> {
        if input_asset_id == output_asset_id {
            return None;
        }

        let Ok(dex_info) = T::DexInfoProvider::get_dex_info(&dex_id) else {
            return None;
        };

        let order_book_id = match dex_info.base_asset_id {
            input if input == *input_asset_id => OrderBookId::<T::AssetId> {
                base: *output_asset_id,
                quote: input,
            },
            output if output == *output_asset_id => OrderBookId::<T::AssetId> {
                base: *input_asset_id,
                quote: output,
            },
            _ => {
                return None;
            }
        };

        Some(order_book_id)
    }
}

impl<T: Config> LiquiditySource<T::DEXId, T::AccountId, T::AssetId, Balance, DispatchError>
    for Pallet<T>
{
    fn can_exchange(
        dex_id: &T::DEXId,
        input_asset_id: &T::AssetId,
        output_asset_id: &T::AssetId,
    ) -> bool {
        let Some(order_book_id) = Self::assemble_order_book_id(dex_id, input_asset_id, output_asset_id) else {
            return false;
        };

        let Some(order_book) = <OrderBooks<T>>::get(order_book_id) else {
            return false;
        };

        order_book.status == OrderBookStatus::Trade
    }

    fn quote(
        dex_id: &T::DEXId,
        input_asset_id: &T::AssetId,
        output_asset_id: &T::AssetId,
        amount: QuoteAmount<Balance>,
        _deduce_fee: bool,
    ) -> Result<(SwapOutcome<Balance>, Weight), DispatchError> {
        let Some(order_book_id) = Self::assemble_order_book_id(dex_id, input_asset_id, output_asset_id) else {
            return Err(Error::<T>::UnknownOrderBook.into());
        };

        let order_book = <OrderBooks<T>>::get(order_book_id).ok_or(Error::<T>::UnknownOrderBook)?;
        let mut data = CacheDataLayer::<T>::new();

        let deal_info =
            order_book.calculate_deal(input_asset_id, output_asset_id, amount, &mut data)?;

        ensure!(deal_info.is_valid(), Error::<T>::PriceCalculationFailed);

        let fee = 0; // todo (m.tagirov)

        match amount {
            QuoteAmount::WithDesiredInput { .. } => Ok((
                SwapOutcome::new(*deal_info.output_amount.value(), fee),
                Self::quote_weight(),
            )),
            QuoteAmount::WithDesiredOutput { .. } => Ok((
                SwapOutcome::new(*deal_info.input_amount.value(), fee),
                Self::quote_weight(),
            )),
        }
    }

    fn exchange(
        sender: &T::AccountId,
        receiver: &T::AccountId,
        dex_id: &T::DEXId,
        input_asset_id: &T::AssetId,
        output_asset_id: &T::AssetId,
        desired_amount: SwapAmount<Balance>,
    ) -> Result<(SwapOutcome<Balance>, Weight), DispatchError> {
        let Some(order_book_id) = Self::assemble_order_book_id(dex_id, input_asset_id, output_asset_id) else {
            return Err(Error::<T>::UnknownOrderBook.into());
        };

        let order_book = <OrderBooks<T>>::get(order_book_id).ok_or(Error::<T>::UnknownOrderBook)?;
        let mut data = CacheDataLayer::<T>::new();

        let deal_info = order_book.calculate_deal(
            input_asset_id,
            output_asset_id,
            desired_amount.into(),
            &mut data,
        )?;

        ensure!(deal_info.is_valid(), Error::<T>::PriceCalculationFailed);

        match desired_amount {
            SwapAmount::WithDesiredInput { min_amount_out, .. } => {
                ensure!(
                    *deal_info.output_amount.value() >= min_amount_out,
                    Error::<T>::SlippageLimitExceeded
                );
            }
            SwapAmount::WithDesiredOutput { max_amount_in, .. } => {
                ensure!(
                    *deal_info.input_amount.value() <= max_amount_in,
                    Error::<T>::SlippageLimitExceeded
                );
            }
        }

        let to = if sender == receiver {
            None
        } else {
            Some(receiver.clone())
        };

        let order = MarketOrder::<T>::new(
            sender.clone(),
            deal_info.side,
            order_book_id,
            deal_info.base_amount(),
            to,
        );

        let (input_amount, output_amount) =
            order_book.execute_market_order::<Self, Self, Self>(order, &mut data)?;

        let fee = 0; // todo (m.tagirov)

        let result = match desired_amount {
            SwapAmount::WithDesiredInput { min_amount_out, .. } => {
                ensure!(
                    *output_amount.value() >= min_amount_out,
                    Error::<T>::SlippageLimitExceeded
                );
                SwapOutcome::new(*output_amount.value(), fee)
            }
            SwapAmount::WithDesiredOutput { max_amount_in, .. } => {
                ensure!(
                    *input_amount.value() <= max_amount_in,
                    Error::<T>::SlippageLimitExceeded
                );
                SwapOutcome::new(*input_amount.value(), fee)
            }
        };

        data.commit();

        Ok((result, Self::exchange_weight()))
    }

    fn check_rewards(
        _dex_id: &T::DEXId,
        _input_asset_id: &T::AssetId,
        _output_asset_id: &T::AssetId,
        _input_amount: Balance,
        _output_amount: Balance,
    ) -> Result<(Vec<(Balance, T::AssetId, RewardReason)>, Weight), DispatchError> {
        Ok((Vec::new(), Weight::zero())) // no rewards for Order Book
    }

    fn quote_without_impact(
        dex_id: &T::DEXId,
        input_asset_id: &T::AssetId,
        output_asset_id: &T::AssetId,
        amount: QuoteAmount<Balance>,
        _deduce_fee: bool,
    ) -> Result<SwapOutcome<Balance>, DispatchError> {
        let Some(order_book_id) = Self::assemble_order_book_id(dex_id, input_asset_id, output_asset_id) else {
            return Err(Error::<T>::UnknownOrderBook.into());
        };

        let order_book = <OrderBooks<T>>::get(order_book_id).ok_or(Error::<T>::UnknownOrderBook)?;
        let mut data = CacheDataLayer::<T>::new();

        let side = order_book.get_side(input_asset_id, output_asset_id)?;

        let Some((price, _)) = (match side {
            PriceVariant::Buy => order_book.best_ask(&mut data),
            PriceVariant::Sell => order_book.best_bid(&mut data),
        }) else {
            return Err(Error::<T>::NotEnoughLiquidityInOrderBook.into());
        };

        let target_amount = match amount {
            QuoteAmount::WithDesiredInput { desired_amount_in } => match side {
                // User wants to swap a known amount of the `quote` asset for the `base` asset.
                // Necessary to return `base` amount.
                // Divide the `quote` amount by the price and align the `base` amount.
                PriceVariant::Buy => order_book.align_amount(
                    (FixedWrapper::from(desired_amount_in) / FixedWrapper::from(price))
                        .try_into_balance()
                        .map_err(|_| Error::<T>::AmountCalculationFailed)?,
                ),

                // User wants to swap a known amount of the `base` asset for the `quote` asset.
                // Necessary to return `quote` amount.
                // Align the `base` amount and then multiply by the price.
                PriceVariant::Sell => {
                    (FixedWrapper::from(order_book.align_amount(desired_amount_in))
                        * FixedWrapper::from(price))
                    .try_into_balance()
                    .map_err(|_| Error::<T>::AmountCalculationFailed)?
                }
            },

            QuoteAmount::WithDesiredOutput { desired_amount_out } => match side {
                // User wants to swap the `quote` asset for a known amount of the `base` asset.
                // Necessary to return `quote` amount.
                // Align the `base` amount and then multiply by the price.
                PriceVariant::Buy => {
                    (FixedWrapper::from(order_book.align_amount(desired_amount_out))
                        * FixedWrapper::from(price))
                    .try_into_balance()
                    .map_err(|_| Error::<T>::AmountCalculationFailed)?
                }

                // User wants to swap the `base` asset for a known amount of the `quote` asset.
                // Necessary to return `base` amount.
                PriceVariant::Sell => order_book.align_amount(
                    (FixedWrapper::from(desired_amount_out) / FixedWrapper::from(price))
                        .try_into_balance()
                        .map_err(|_| Error::<T>::AmountCalculationFailed)?,
                ),
            },
        };

        ensure!(
            target_amount > OrderVolume::zero(),
            Error::<T>::InvalidOrderAmount
        );

        let fee = 0; // todo (m.tagirov)

        Ok(SwapOutcome::new(target_amount, fee))
    }

    fn quote_weight() -> Weight {
        <T as Config>::WeightInfo::quote()
    }

    fn exchange_weight() -> Weight {
        <T as Config>::WeightInfo::exchange()
    }

    fn check_rewards_weight() -> Weight {
        Weight::zero()
    }
}

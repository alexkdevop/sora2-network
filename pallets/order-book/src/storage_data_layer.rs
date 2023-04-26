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

use crate::{
    AggregatedAsks, AggregatedBids, Asks, Bids, Config, DataLayer, Error, LimitOrder, LimitOrders,
    MarketSide, OrderBookId, OrderPrice, OrderVolume, PriceOrders, UserLimitOrders, UserOrders,
};
use assets::AssetIdOf;
use common::PriceVariant;
use frame_support::ensure;
use frame_support::sp_runtime::DispatchError;
use sp_runtime::traits::Zero;
use sp_std::marker::PhantomData;

pub struct StorageDataLayer<T: Config> {
    _phantom: PhantomData<T>,
}

impl<T: Config> StorageDataLayer<T> {
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

impl<T: Config> StorageDataLayer<T> {
    fn remove_from_bids(
        order_book_id: &OrderBookId<AssetIdOf<T>>,
        order: &LimitOrder<T>,
    ) -> Result<(), ()> {
        let mut bids = <Bids<T>>::try_get(order_book_id, order.price).map_err(|_| ())?;
        bids.retain(|x| *x != order.id);
        if bids.is_empty() {
            <Bids<T>>::remove(order_book_id, order.price);
        } else {
            <Bids<T>>::set(order_book_id, order.price, Some(bids));
        }
        Ok(())
    }

    fn remove_from_asks(
        order_book_id: &OrderBookId<AssetIdOf<T>>,
        order: &LimitOrder<T>,
    ) -> Result<(), ()> {
        let mut asks = <Asks<T>>::try_get(order_book_id, order.price).map_err(|_| ())?;
        asks.retain(|x| *x != order.id);
        if asks.is_empty() {
            <Asks<T>>::remove(order_book_id, order.price);
        } else {
            <Asks<T>>::set(order_book_id, order.price, Some(asks));
        }
        Ok(())
    }

    fn add_to_aggregated_bids(
        order_book_id: &OrderBookId<AssetIdOf<T>>,
        price: &OrderPrice,
        value: &OrderVolume,
    ) -> Result<(), ()> {
        let mut bids = <AggregatedBids<T>>::get(order_book_id);
        let volume = bids
            .get(price)
            .map(|x| *x)
            .unwrap_or_default()
            .checked_add(*value)
            .ok_or(())?;
        bids.try_insert(*price, volume).map_err(|_| ())?;
        <AggregatedBids<T>>::set(order_book_id, bids);
        Ok(())
    }

    fn sub_from_aggregated_bids(
        order_book_id: &OrderBookId<AssetIdOf<T>>,
        price: &OrderPrice,
        value: &OrderVolume,
    ) -> Result<(), ()> {
        let mut agg_bids = <AggregatedBids<T>>::try_get(order_book_id).map_err(|_| ())?;
        let volume = agg_bids
            .get(price)
            .map(|x| *x)
            .ok_or(())?
            .checked_sub(*value)
            .ok_or(())?;
        if volume.is_zero() {
            agg_bids.remove(price).ok_or(())?;
        } else {
            agg_bids.try_insert(*price, volume).map_err(|_| ())?;
        }
        <AggregatedBids<T>>::set(order_book_id, agg_bids);
        Ok(())
    }

    fn add_to_aggregated_asks(
        order_book_id: &OrderBookId<AssetIdOf<T>>,
        price: &OrderPrice,
        value: &OrderVolume,
    ) -> Result<(), ()> {
        let mut asks = <AggregatedAsks<T>>::get(order_book_id);
        let volume = asks
            .get(price)
            .map(|x| *x)
            .unwrap_or_default()
            .checked_add(*value)
            .ok_or(())?;
        asks.try_insert(*price, volume).map_err(|_| ())?;
        <AggregatedAsks<T>>::set(order_book_id, asks);
        Ok(())
    }

    fn sub_from_aggregated_asks(
        order_book_id: &OrderBookId<AssetIdOf<T>>,
        price: &OrderPrice,
        value: &OrderVolume,
    ) -> Result<(), ()> {
        let mut agg_asks = <AggregatedAsks<T>>::try_get(order_book_id).map_err(|_| ())?;
        let volume = agg_asks
            .get(price)
            .map(|x| *x)
            .ok_or(())?
            .checked_sub(*value)
            .ok_or(())?;
        if volume.is_zero() {
            agg_asks.remove(price).ok_or(())?;
        } else {
            agg_asks.try_insert(*price, volume).map_err(|_| ())?;
        }
        <AggregatedAsks<T>>::set(order_book_id, agg_asks);
        Ok(())
    }
}

impl<T: Config> DataLayer<T> for StorageDataLayer<T> {
    fn get_limit_order(
        &mut self,
        order_book_id: &OrderBookId<AssetIdOf<T>>,
        order_id: T::OrderId,
    ) -> Result<LimitOrder<T>, DispatchError> {
        if let Some(order) = <LimitOrders<T>>::get(order_book_id, order_id) {
            Ok(order)
        } else {
            Err(Error::<T>::UnknownLimitOrder.into())
        }
    }

    fn get_all_limit_orders(
        &mut self,
        order_book_id: &OrderBookId<AssetIdOf<T>>,
    ) -> Vec<LimitOrder<T>> {
        let mut orders = Vec::new();
        for order in <LimitOrders<T>>::iter_prefix_values(order_book_id) {
            orders.push(order);
        }
        orders
    }

    fn insert_limit_order(
        &mut self,
        order_book_id: &OrderBookId<AssetIdOf<T>>,
        order: LimitOrder<T>,
    ) -> Result<(), DispatchError> {
        if <LimitOrders<T>>::contains_key(order_book_id, order.id) {
            return Err(Error::<T>::LimitOrderAlreadyExists.into());
        }

        match order.side {
            PriceVariant::Buy => {
                <Bids<T>>::try_append(order_book_id, order.price, order.id)
                    .map_err(|_| Error::<T>::LimitOrderStorageOverflow)?;

                Self::add_to_aggregated_bids(order_book_id, &order.price, &order.amount)
                    .map_err(|_| Error::<T>::LimitOrderStorageOverflow)?;
            }
            PriceVariant::Sell => {
                <Asks<T>>::try_append(order_book_id, order.price, order.id)
                    .map_err(|_| Error::<T>::LimitOrderStorageOverflow)?;

                Self::add_to_aggregated_asks(order_book_id, &order.price, &order.amount)
                    .map_err(|_| Error::<T>::LimitOrderStorageOverflow)?;
            }
        }

        <UserLimitOrders<T>>::try_append(&order.owner, order_book_id, order.id)
            .map_err(|_| Error::<T>::LimitOrderStorageOverflow)?;

        <LimitOrders<T>>::insert(order_book_id, order.id, order);

        Ok(())
    }

    fn update_limit_order_amount(
        &mut self,
        order_book_id: &OrderBookId<AssetIdOf<T>>,
        order_id: T::OrderId,
        new_amount: OrderVolume,
    ) -> Result<(), DispatchError> {
        let mut order = <LimitOrders<T>>::try_get(order_book_id, order_id)
            .map_err(|_| Error::<T>::UnknownLimitOrder)?;
        if new_amount == order.amount {
            // nothing to update
            return Ok(());
        }
        if new_amount.is_zero() {
            return self.delete_limit_order(order_book_id, order_id);
        }
        ensure!(order.amount > new_amount, Error::<T>::UpdateLimitOrderError);

        let delta = order.amount - new_amount;

        match order.side {
            PriceVariant::Buy => {
                Self::sub_from_aggregated_bids(order_book_id, &order.price, &delta)
                    .map_err(|_| Error::<T>::UpdateLimitOrderError)?;
            }
            PriceVariant::Sell => {
                Self::sub_from_aggregated_asks(order_book_id, &order.price, &delta)
                    .map_err(|_| Error::<T>::UpdateLimitOrderError)?;
            }
        }

        order.amount = new_amount;
        <LimitOrders<T>>::insert(order_book_id, order_id, order);
        Ok(())
    }

    fn delete_limit_order(
        &mut self,
        order_book_id: &OrderBookId<AssetIdOf<T>>,
        order_id: T::OrderId,
    ) -> Result<(), DispatchError> {
        let order =
            <LimitOrders<T>>::take(order_book_id, order_id).ok_or(Error::<T>::UnknownLimitOrder)?;

        let mut user_orders = <UserLimitOrders<T>>::try_get(&order.owner, order_book_id)
            .map_err(|_| Error::<T>::DeleteLimitOrderError)?;
        user_orders.retain(|x| *x != order.id);
        if user_orders.is_empty() {
            <UserLimitOrders<T>>::remove(&order.owner, order_book_id)
        } else {
            <UserLimitOrders<T>>::set(&order.owner, order_book_id, Some(user_orders));
        }

        match order.side {
            PriceVariant::Buy => {
                Self::remove_from_bids(order_book_id, &order)
                    .map_err(|_| Error::<T>::DeleteLimitOrderError)?;
                Self::sub_from_aggregated_bids(order_book_id, &order.price, &order.amount)
                    .map_err(|_| Error::<T>::DeleteLimitOrderError)?;
            }
            PriceVariant::Sell => {
                Self::remove_from_asks(order_book_id, &order)
                    .map_err(|_| Error::<T>::DeleteLimitOrderError)?;
                Self::sub_from_aggregated_asks(order_book_id, &order.price, &order.amount)
                    .map_err(|_| Error::<T>::DeleteLimitOrderError)?;
            }
        }

        Ok(())
    }

    fn get_bids(
        &mut self,
        order_book_id: &OrderBookId<AssetIdOf<T>>,
        price: &OrderPrice,
    ) -> Option<PriceOrders<T::OrderId, T::MaxLimitOrdersForPrice>> {
        <Bids<T>>::get(order_book_id, price)
    }

    fn get_asks(
        &mut self,
        order_book_id: &OrderBookId<AssetIdOf<T>>,
        price: &OrderPrice,
    ) -> Option<PriceOrders<T::OrderId, T::MaxLimitOrdersForPrice>> {
        <Asks<T>>::get(order_book_id, price)
    }

    fn get_aggregated_bids(
        &mut self,
        order_book_id: &OrderBookId<AssetIdOf<T>>,
    ) -> MarketSide<T::MaxSidePriceCount> {
        <AggregatedBids<T>>::get(order_book_id)
    }

    fn get_aggregated_asks(
        &mut self,
        order_book_id: &OrderBookId<AssetIdOf<T>>,
    ) -> MarketSide<T::MaxSidePriceCount> {
        <AggregatedAsks<T>>::get(order_book_id)
    }

    fn get_user_limit_orders(
        &mut self,
        account: &T::AccountId,
        order_book_id: &OrderBookId<AssetIdOf<T>>,
    ) -> Option<UserOrders<T::OrderId, T::MaxOpenedLimitOrdersPerUser>> {
        <UserLimitOrders<T>>::get(account, order_book_id)
    }
}

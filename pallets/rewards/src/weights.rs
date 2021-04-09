//! Autogenerated weights for rewards
//!
//! THIS FILE WAS AUTO-GENERATED USING THE SUBSTRATE BENCHMARK CLI VERSION 3.0.0
//! DATE: 2021-04-03, STEPS: [20, ], REPEAT: 10, LOW RANGE: [], HIGH RANGE: []
//! EXECUTION: Some(Wasm), WASM-EXECUTION: Compiled, CHAIN: Some("dev"), DB CACHE: 128

// Executed Command:
// target\release\framenode.exe
// benchmark
// --chain
// dev
// --execution
// wasm
// --wasm-execution
// compiled
// --pallet
// rewards
// --extrinsic=*
// --steps
// 20
// --repeat
// 10
// --raw
// --output
// ./

use core::marker::PhantomData;

use frame_support::traits::Get;
use frame_support::weights::Weight;

use common::prelude::constants::EXTRINSIC_FIXED_WEIGHT;

pub struct WeightInfo<T>(PhantomData<T>);

impl<T: frame_system::Config> crate::WeightInfo for WeightInfo<T> {
    fn claim() -> Weight {
        (21_316_821_000 as Weight)
            .saturating_add(T::DbWeight::get().reads(16 as Weight))
            .saturating_add(T::DbWeight::get().writes(8 as Weight))
    }
}

impl crate::WeightInfo for () {
    fn claim() -> Weight {
        EXTRINSIC_FIXED_WEIGHT
    }
}

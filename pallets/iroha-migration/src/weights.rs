//! Autogenerated weights for iroha_migration
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
// iroha_migration
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
    fn migrate() -> Weight {
        (853_397_000 as Weight)
            .saturating_add(T::DbWeight::get().reads(13 as Weight))
            .saturating_add(T::DbWeight::get().writes(6 as Weight))
    }
    fn on_initialize() -> Weight {
        (215_881_000 as Weight)
            .saturating_add(T::DbWeight::get().reads(14 as Weight))
            .saturating_add(T::DbWeight::get().writes(8 as Weight))
    }
}

impl crate::WeightInfo for () {
    fn migrate() -> Weight {
        EXTRINSIC_FIXED_WEIGHT
    }

    fn on_initialize() -> Weight {
        EXTRINSIC_FIXED_WEIGHT
    }
}

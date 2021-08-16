//! Autogenerated weights for referral_system
//!
//! THIS FILE WAS AUTO-GENERATED USING THE SUBSTRATE BENCHMARK CLI VERSION 3.0.0
//! DATE: 2021-08-13, STEPS: [20, ], REPEAT: 10, LOW RANGE: [], HIGH RANGE: []
//! EXECUTION: Some(Wasm), WASM-EXECUTION: Compiled, CHAIN: Some("main-coded"), DB CACHE: 128

// Executed Command:
// target/debug/framenode
// benchmark
// --chain
// main-coded
// --execution
// wasm
// --wasm-execution
// compiled
// --pallet
// referral_system
// --extrinsic=*
// --steps
// 20
// --repeat
// 10
// --raw
// --output
// ./

use frame_support::traits::Get;
use frame_support::weights::Weight;
use sp_std::marker::PhantomData;

pub struct WeightInfo<T>(PhantomData<T>);
impl<T: frame_system::Config> crate::WeightInfo for WeightInfo<T> {
    fn reserve() -> Weight {
        (2_099_200_000 as Weight)
            .saturating_add(T::DbWeight::get().reads(4 as Weight))
            .saturating_add(T::DbWeight::get().writes(3 as Weight))
    }
    fn set_referrer() -> Weight {
        (1_250_900_000 as Weight)
            .saturating_add(T::DbWeight::get().reads(3 as Weight))
            .saturating_add(T::DbWeight::get().writes(3 as Weight))
    }
}

impl crate::WeightInfo for () {
    fn reserve() -> Weight {
        0
    }

    fn set_referrer() -> Weight {
        0
    }
}

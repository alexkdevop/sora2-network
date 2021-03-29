use crate::contract::{functions, FUNCTIONS, RECEIVE_BY_ETHEREUM_ASSET_ADDRESS_ID};
use crate::mock::*;
use crate::requests::{
    encode_outgoing_request_eth_call, ChangePeersContract, IncomingAddToken,
    IncomingChangePeersCompat, IncomingMigrate, IncomingPrepareForMigration, IncomingTransfer,
    OutgoingAddAsset, OutgoingAddPeer, OutgoingAddPeerCompat, OutgoingAddToken, OutgoingMigrate,
    OutgoingPrepareForMigration, OutgoingRemovePeer, OutgoingRemovePeerCompat,
};
use crate::types::{Bytes, Log, Transaction};
use crate::{
    majority, types, Address, AssetConfig, AssetKind, BridgeStatus, ContractEvent,
    IncomingPreRequest, IncomingRequest, IncomingRequestKind, OffchainRequest, OutgoingRequest,
    OutgoingTransfer, RequestStatus, SignatureParams,
};
use codec::{Decode, Encode};
use common::prelude::Balance;
use common::{
    balance, eth, AssetId, AssetId32, AssetSymbol, DEFAULT_BALANCE_PRECISION, DOT, KSM, USDT, VAL,
    XOR,
};
use frame_support::sp_runtime::app_crypto::sp_core::crypto::AccountId32;
use frame_support::sp_runtime::app_crypto::sp_core::{self, ecdsa, sr25519, Pair, Public};
use frame_support::sp_runtime::traits::IdentifyAccount;
use frame_support::storage::TransactionOutcome;
use frame_support::{assert_err, assert_noop, assert_ok, ensure};
use hex_literal::hex;
use rustc_hex::FromHex;
use secp256k1::{PublicKey, SecretKey};
use sp_core::{H160, H256};
use sp_std::collections::btree_set::BTreeSet;
use sp_std::prelude::*;
use std::str::FromStr;

type Error = crate::Error<Runtime>;
type Assets = assets::Pallet<Runtime>;

const ETH_NETWORK_ID: u32 = 0;

fn get_signature_params(signature: &ecdsa::Signature) -> SignatureParams {
    let encoded = signature.encode();
    let mut params = SignatureParams::decode(&mut &encoded[..]).expect("Wrong signature format");
    params.v += 27;
    params
}

#[test]
fn parses_event() {
    let (mut ext, _) = ExtBuilder::default().build();
    ext.execute_with(|| {
        let mut log = Log::default();
        log.topics = vec![types::H256(hex!("85c0fa492ded927d3acca961da52b0dda1debb06d8c27fe189315f06bb6e26c8"))];
        log.data = Bytes(hex!("111111111111111111111111111111111111111111111111111111111111111100000000000000000000000000000000000000000000000246ddf9797668000000000000000000000000000022222222222222222222222222222222222222220200040000000000000000000000000000000000000000000000000000000011").to_vec());
        assert_eq!(
            EthBridge::parse_main_event(&[log], IncomingRequestKind::Transfer).unwrap(),
            ContractEvent::Deposit(
                AccountId32::from(hex!("1111111111111111111111111111111111111111111111111111111111111111")),
                balance!(42),
                H160::from(&hex!("2222222222222222222222222222222222222222")),
                H256(hex!("0200040000000000000000000000000000000000000000000000000000000011"))
            )
        )
    });
}

fn last_event() -> Option<Event> {
    frame_system::Module::<Runtime>::events()
        .pop()
        .map(|x| x.event)
}

fn no_event() -> bool {
    frame_system::Module::<Runtime>::events().pop().is_none()
}

fn approve_request(state: &State, request: OutgoingRequest<Runtime>) -> Result<(), Option<Event>> {
    let request_hash = request.hash();
    let encoded = request.to_eth_abi(request_hash).unwrap();
    System::reset_events();
    let net_id = request.network_id();
    let mut approvals = BTreeSet::new();
    let keypairs = &state.networks[&net_id].ocw_keypairs;
    for (i, (_signer, account_id, seed)) in keypairs.iter().enumerate() {
        let secret = SecretKey::parse_slice(seed).unwrap();
        let public = PublicKey::from_secret_key(&secret);
        let msg = eth::prepare_message(encoded.as_raw());
        let sig_pair = secp256k1::sign(&msg, &secret);
        let signature = sig_pair.into();
        let signature_params = get_signature_params(&signature);
        approvals.insert(signature_params.clone());
        let additional_sigs = if crate::PendingPeer::<Runtime>::get(net_id).is_some() {
            1
        } else {
            0
        };
        let sigs_needed = majority(keypairs.len()) + additional_sigs;
        let current_status =
            crate::RequestStatuses::<Runtime>::get(net_id, &request.hash()).unwrap();
        ensure!(
            EthBridge::approve_request(
                Origin::signed(account_id.clone()),
                ecdsa::Public::from_slice(&public.serialize_compressed()),
                request.clone(),
                encoded.clone(),
                signature_params
            )
            .is_ok(),
            None
        );
        if current_status == RequestStatus::Pending && i + 1 == sigs_needed {
            match last_event().ok_or(None)? {
                Event::eth_bridge(bridge_event) => match bridge_event {
                    crate::Event::ApprovalsCollected(e, a) => {
                        assert_eq!(e, encoded);
                        assert_eq!(a, approvals);
                    }
                    e => {
                        assert_ne!(
                            crate::RequestsQueue::<Runtime>::get(net_id)
                                .last()
                                .map(|x| x.hash()),
                            Some(request.hash())
                        );
                        return Err(Some(Event::eth_bridge(e)));
                    }
                },
                e => panic!("Unexpected event: {:?}", e),
            }
        } else {
            assert!(no_event());
        }
        System::reset_events();
    }
    assert_ne!(
        crate::RequestsQueue::<Runtime>::get(net_id)
            .last()
            .map(|x| x.hash()),
        Some(request.hash())
    );
    Ok(())
}

fn last_outgoing_request(net_id: u32) -> Option<OutgoingRequest<Runtime>> {
    let request = crate::RequestsQueue::<Runtime>::get(net_id)
        .last()
        .cloned()?;
    match request {
        OffchainRequest::Outgoing(r, _) => Some(r),
        _ => panic!("Unexpected request type"),
    }
}

fn approve_last_request(
    state: &State,
    net_id: u32,
) -> Result<OutgoingRequest<Runtime>, Option<Event>> {
    let request = crate::RequestsQueue::<Runtime>::get(net_id).pop().unwrap();
    let outgoing_request = match request {
        OffchainRequest::Outgoing(r, _) => r,
        _ => panic!("Unexpected request type"),
    };
    approve_request(state, outgoing_request.clone())?;
    Ok(outgoing_request)
}

fn approve_next_request(
    state: &State,
    net_id: u32,
) -> Result<OutgoingRequest<Runtime>, Option<Event>> {
    let request = crate::RequestsQueue::<Runtime>::get(net_id).remove(0);
    let outgoing_request = match request {
        OffchainRequest::Outgoing(r, _) => r,
        _ => panic!("Unexpected request type"),
    };
    approve_request(state, outgoing_request.clone())?;
    Ok(outgoing_request)
}

fn request_incoming(
    account_id: AccountId,
    tx_hash: H256,
    kind: IncomingRequestKind,
    net_id: u32,
) -> Result<H256, Event> {
    assert_ok!(EthBridge::request_from_sidechain(
        Origin::signed(account_id),
        tx_hash,
        kind,
        net_id
    ));
    let requests_queue = crate::RequestsQueue::get(net_id);
    let last_request: &OffchainRequest<Runtime> = requests_queue.last().unwrap();
    match last_request {
        OffchainRequest::Incoming(..) => (),
        _ => panic!("Invalid off-chain request"),
    }
    let hash = last_request.hash();
    assert_eq!(
        crate::RequestStatuses::<Runtime>::get(net_id, &hash).unwrap(),
        RequestStatus::Pending
    );
    Ok(hash)
}

fn assert_incoming_request_done(
    state: &State,
    incoming_request: IncomingRequest<Runtime>,
) -> Result<(), Option<Event>> {
    let net_id = incoming_request.network_id();
    let bridge_acc_id = state.networks[&net_id].config.bridge_account_id.clone();
    let req_hash = incoming_request.hash();
    assert_eq!(
        crate::RequestsQueue::<Runtime>::get(net_id)
            .last()
            .unwrap()
            .hash()
            .0,
        req_hash.0
    );
    assert_ok!(EthBridge::register_incoming_request(
        Origin::signed(bridge_acc_id.clone()),
        incoming_request.clone()
    ));
    assert_ne!(
        crate::RequestsQueue::<Runtime>::get(net_id)
            .last()
            .map(|x| x.hash().0),
        Some(req_hash.0)
    );
    assert!(crate::PendingIncomingRequests::<Runtime>::get(net_id).contains(&req_hash));
    assert_eq!(
        crate::IncomingRequests::get(net_id, &req_hash).unwrap(),
        incoming_request
    );
    assert_ok!(EthBridge::finalize_incoming_request(
        Origin::signed(bridge_acc_id.clone()),
        req_hash,
        net_id,
    ));
    assert_eq!(
        crate::RequestStatuses::<Runtime>::get(net_id, &req_hash).unwrap(),
        RequestStatus::Done
    );
    assert!(crate::PendingIncomingRequests::<Runtime>::get(net_id).is_empty());
    Ok(())
}

fn assert_incoming_request_registration_failed(
    state: &State,
    incoming_request: IncomingRequest<Runtime>,
    error: crate::Error<Runtime>,
) -> Result<(), Event> {
    let net_id = incoming_request.network_id();
    let bridge_acc_id = state.networks[&net_id].config.bridge_account_id.clone();
    assert_eq!(
        crate::RequestsQueue::<Runtime>::get(net_id)
            .last()
            .unwrap()
            .hash()
            .0,
        incoming_request.hash().0
    );
    assert_err!(
        EthBridge::register_incoming_request(
            Origin::signed(bridge_acc_id.clone()),
            incoming_request.clone()
        ),
        error
    );
    Ok(())
}

#[test]
fn should_approve_outgoing_transfer() {
    let (mut ext, state) = ExtBuilder::default().build();

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        Assets::mint_to(&XOR.into(), &alice, &alice, 100000u32.into()).unwrap();
        assert_eq!(
            Assets::total_balance(&XOR.into(), &alice).unwrap(),
            100000u32.into()
        );
        assert_ok!(EthBridge::transfer_to_sidechain(
            Origin::signed(alice.clone()),
            XOR.into(),
            Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
            100_u32.into(),
            net_id,
        ));
        assert_eq!(
            Assets::total_balance(&XOR.into(), &alice).unwrap(),
            99900u32.into()
        );
        approve_last_request(&state, net_id).expect("request wasn't approved");
    });
}

#[test]
fn should_reserve_and_burn_sidechain_asset_in_outgoing_transfer() {
    let net_id = ETH_NETWORK_ID;
    let mut builder = ExtBuilder::new();
    builder.add_network(
        vec![AssetConfig::Sidechain {
            id: USDT.into(),
            sidechain_id: H160(hex!("dAC17F958D2ee523a2206206994597C13D831ec7")),
            owned: false,
            precision: 18,
        }],
        None,
        None,
    );
    let (mut ext, state) = builder.build();

    ext.execute_with(|| {
        let bridge_acc = &state.networks[&net_id].config.bridge_account_id;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        Assets::mint_to(&USDT.into(), &alice, &alice, 100000u32.into()).unwrap();
        assert_ok!(EthBridge::transfer_to_sidechain(
            Origin::signed(alice.clone()),
            USDT.into(),
            Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
            100_u32.into(),
            net_id,
        ));
        assert_eq!(
            Assets::free_balance(&USDT.into(), &bridge_acc).unwrap(),
            0u32.into()
        );
        // Sidechain asset was reserved.
        assert_eq!(
            Assets::total_balance(&USDT.into(), &bridge_acc).unwrap(),
            100u32.into()
        );
        approve_last_request(&state, net_id).expect("request wasn't approved");
        // Sidechain asset was burnt.
        assert_eq!(
            Assets::total_balance(&USDT.into(), &bridge_acc).unwrap(),
            0u32.into()
        );
        assert_eq!(
            Assets::free_balance(&USDT.into(), &bridge_acc).unwrap(),
            Assets::total_balance(&USDT.into(), &bridge_acc).unwrap()
        );
    });
}

#[test]
fn should_reserve_and_unreserve_thischain_asset_in_outgoing_transfer() {
    let net_id = ETH_NETWORK_ID;
    let mut builder = ExtBuilder::new();
    builder.add_network(
        vec![AssetConfig::Thischain { id: PSWAP.into() }],
        None,
        None,
    );
    let (mut ext, state) = builder.build();

    ext.execute_with(|| {
        let bridge_acc = &state.networks[&net_id].config.bridge_account_id;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        Assets::mint_to(&PSWAP.into(), &alice, &alice, 100000u32.into()).unwrap();
        assert_ok!(EthBridge::transfer_to_sidechain(
            Origin::signed(alice.clone()),
            PSWAP.into(),
            Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
            100_u32.into(),
            net_id,
        ));
        assert_eq!(
            Assets::free_balance(&PSWAP.into(), &bridge_acc).unwrap(),
            0u32.into()
        );
        // Thischain asset was reserved.
        assert_eq!(
            Assets::total_balance(&PSWAP.into(), &bridge_acc).unwrap(),
            100u32.into()
        );
        approve_last_request(&state, net_id).expect("request wasn't approved");
        // Thischain asset was unreserved.
        assert_eq!(
            Assets::total_balance(&PSWAP.into(), &bridge_acc).unwrap(),
            100u32.into()
        );
        assert_eq!(
            Assets::free_balance(&PSWAP.into(), &bridge_acc).unwrap(),
            Assets::total_balance(&PSWAP.into(), &bridge_acc).unwrap()
        );
    });
}

#[test]
fn should_mint_and_burn_sidechain_asset() {
    let (mut ext, state) = ExtBuilder::default().build();

    #[track_caller]
    fn check_invariant(asset_id: &AssetId32<AssetId>, val: u32) {
        assert_eq!(Assets::total_issuance(asset_id).unwrap(), val.into());
    }

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        let token_address = Address::from(hex!("7d7ff6f42e928de241282b9606c8e98ea48526e2"));
        EthBridge::register_sidechain_asset(
            token_address,
            18,
            AssetSymbol(b"TEST".to_vec()),
            net_id,
        )
        .unwrap();
        let (asset_id, asset_kind) =
            EthBridge::get_asset_by_raw_asset_id(H256::zero(), &token_address, net_id)
                .unwrap()
                .unwrap();
        assert_eq!(asset_kind, AssetKind::Sidechain);
        check_invariant(&asset_id, 0);
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[1u8; 32]),
            IncomingRequestKind::Transfer,
            net_id,
        )
        .unwrap();
        let incoming_transfer = IncomingRequest::Transfer(crate::IncomingTransfer {
            from: Address::from([1; 20]),
            to: alice.clone(),
            asset_id,
            asset_kind,
            amount: 100u32.into(),
            tx_hash,
            at_height: 1,
            timepoint: Default::default(),
            network_id: ETH_NETWORK_ID,
            should_take_fee: false,
        });
        assert_incoming_request_done(&state, incoming_transfer.clone()).unwrap();
        check_invariant(&asset_id, 100);
        assert_ok!(EthBridge::transfer_to_sidechain(
            Origin::signed(alice.clone()),
            asset_id,
            Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
            100_u32.into(),
            net_id,
        ));
        approve_last_request(&state, net_id).expect("request wasn't approved");
        check_invariant(&asset_id, 0);
    });
}

#[test]
fn should_not_burn_or_mint_sidechain_owned_asset() {
    let (mut ext, state) = ExtBuilder::default().build();

    #[track_caller]
    fn check_invariant() {
        assert_eq!(
            Assets::total_issuance(&XOR.into()).unwrap(),
            balance!(350000)
        );
    }

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        assert_eq!(
            EthBridge::registered_asset(net_id, AssetId32::from(XOR)).unwrap(),
            AssetKind::SidechainOwned
        );
        check_invariant();
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[1u8; 32]),
            IncomingRequestKind::Transfer,
            net_id,
        )
        .unwrap();
        let incoming_transfer = IncomingRequest::Transfer(crate::IncomingTransfer {
            from: Address::from([1; 20]),
            to: alice.clone(),
            asset_id: XOR.into(),
            asset_kind: AssetKind::SidechainOwned,
            amount: 100u32.into(),
            tx_hash,
            at_height: 1,
            timepoint: Default::default(),
            network_id: ETH_NETWORK_ID,
            should_take_fee: false,
        });
        assert_incoming_request_done(&state, incoming_transfer.clone()).unwrap();
        check_invariant();
        assert_ok!(EthBridge::transfer_to_sidechain(
            Origin::signed(alice.clone()),
            XOR.into(),
            Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
            100_u32.into(),
            net_id,
        ));
        approve_last_request(&state, net_id).expect("request wasn't approved");
        check_invariant();
    });
}

#[test]
fn should_not_transfer() {
    let (mut ext, _) = ExtBuilder::default().build();

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        assert_err!(
            EthBridge::transfer_to_sidechain(
                Origin::signed(alice.clone()),
                KSM.into(),
                Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
                100_u32.into(),
                net_id,
            ),
            Error::UnsupportedToken
        );
        assert!(EthBridge::transfer_to_sidechain(
            Origin::signed(alice.clone()),
            XOR.into(),
            Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
            100_000_000_u32.into(),
            net_id,
        )
        .is_err());
    });
}

#[test]
fn should_register_outgoing_transfer() {
    let (mut ext, _state) = ExtBuilder::default().build();

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        Assets::mint_to(&XOR.into(), &alice, &alice, 100000u32.into()).unwrap();
        assert_ok!(EthBridge::transfer_to_sidechain(
            Origin::signed(alice.clone()),
            XOR.into(),
            Address::from([1; 20]),
            100u32.into(),
            net_id,
        ));
        let outgoing_transfer = OutgoingTransfer::<Runtime> {
            from: alice.clone(),
            to: Address::from([1; 20]),
            asset_id: XOR.into(),
            amount: 100_u32.into(),
            nonce: 3,
            network_id: ETH_NETWORK_ID,
            timepoint: bridge_multisig::Pallet::<Runtime>::timepoint(),
        };
        let last_request = crate::RequestsQueue::get(net_id).pop().unwrap();
        match last_request {
            OffchainRequest::Outgoing(OutgoingRequest::Transfer(r), _) => {
                assert_eq!(r, outgoing_transfer)
            }
            _ => panic!("Invalid off-chain request"),
        }
    });
}

#[test]
fn should_not_accept_duplicated_incoming_transfer() {
    let (mut ext, _state) = ExtBuilder::default().build();

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        assert_ok!(EthBridge::request_from_sidechain(
            Origin::signed(alice.clone()),
            H256::from_slice(&[1u8; 32]),
            IncomingRequestKind::Transfer,
            net_id,
        ));
        assert_err!(
            EthBridge::request_from_sidechain(
                Origin::signed(alice.clone()),
                H256::from_slice(&[1u8; 32]),
                IncomingRequestKind::Transfer,
                net_id,
            ),
            Error::DuplicatedRequest
        );
    });
}

#[test]
fn should_not_accept_approved_incoming_transfer() {
    let (mut ext, state) = ExtBuilder::default().build();

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[1u8; 32]),
            IncomingRequestKind::Transfer,
            net_id,
        )
        .unwrap();
        let incoming_transfer = IncomingRequest::Transfer(crate::IncomingTransfer {
            from: Address::from([1; 20]),
            to: alice.clone(),
            asset_id: XOR.into(),
            asset_kind: AssetKind::Thischain,
            amount: 100u32.into(),
            tx_hash,
            at_height: 1,
            timepoint: Default::default(),
            network_id: ETH_NETWORK_ID,
            should_take_fee: false,
        });
        assert_incoming_request_done(&state, incoming_transfer.clone()).unwrap();
        assert_err!(
            EthBridge::request_from_sidechain(
                Origin::signed(alice.clone()),
                H256::from_slice(&[1u8; 32]),
                IncomingRequestKind::Transfer,
                net_id,
            ),
            Error::DuplicatedRequest
        );
    });
}

#[test]
fn should_success_incoming_transfer() {
    let (mut ext, state) = ExtBuilder::default().build();
    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[1u8; 32]),
            IncomingRequestKind::Transfer,
            net_id,
        )
        .unwrap();
        let incoming_transfer = IncomingRequest::Transfer(crate::IncomingTransfer {
            from: Address::from([1; 20]),
            to: alice.clone(),
            asset_id: XOR.into(),
            asset_kind: AssetKind::Thischain,
            amount: 100u32.into(),
            tx_hash,
            at_height: 1,
            timepoint: Default::default(),
            network_id: ETH_NETWORK_ID,
            should_take_fee: false,
        });
        assert_eq!(
            Assets::total_balance(&XOR.into(), &alice).unwrap(),
            0u32.into()
        );
        assert_incoming_request_done(&state, incoming_transfer.clone()).unwrap();
        assert_eq!(
            Assets::total_balance(&XOR.into(), &alice).unwrap(),
            100u32.into()
        );
    });
}

#[test]
fn should_cancel_incoming_transfer() {
    let mut builder = ExtBuilder::new();
    let net_id = builder.add_network(
        vec![AssetConfig::Sidechain {
            id: XOR.into(),
            sidechain_id: sp_core::H160::from_str("40fd72257597aa14c7231a7b1aaa29fce868f677")
                .unwrap(),
            owned: true,
            precision: DEFAULT_BALANCE_PRECISION,
        }],
        Some(vec![(XOR.into(), Balance::from(100u32))]),
        None,
    );
    let (mut ext, state) = builder.build();
    ext.execute_with(|| {
        let bridge_acc_id = state.networks[&net_id].config.bridge_account_id.clone();
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        Assets::mint_to(&XOR.into(), &alice, &alice, 100000u32.into()).unwrap();
        let bob = get_account_id_from_seed::<sr25519::Public>("Bob");
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[1u8; 32]),
            IncomingRequestKind::Transfer,
            net_id,
        )
        .unwrap();
        let incoming_transfer = IncomingRequest::Transfer(crate::IncomingTransfer {
            from: Address::from([1; 20]),
            to: alice.clone(),
            asset_id: XOR.into(),
            asset_kind: AssetKind::Thischain,
            amount: 100u32.into(),
            tx_hash,
            at_height: 1,
            timepoint: Default::default(),
            network_id: ETH_NETWORK_ID,
            should_take_fee: false,
        });
        assert_ok!(EthBridge::register_incoming_request(
            Origin::signed(bridge_acc_id.clone()),
            incoming_transfer.clone()
        ));
        assert_eq!(
            Assets::total_balance(&XOR.into(), &alice).unwrap(),
            100000u32.into()
        );
        Assets::unreserve(XOR.into(), &bridge_acc_id, 100u32.into()).unwrap();
        Assets::transfer_from(&XOR.into(), &bridge_acc_id, &bob, 100u32.into()).unwrap();
        assert_ok!(EthBridge::finalize_incoming_request(
            Origin::signed(bridge_acc_id.clone()),
            tx_hash,
            net_id,
        ));
        assert_eq!(
            crate::RequestStatuses::<Runtime>::get(net_id, incoming_transfer.hash()).unwrap(),
            RequestStatus::Failed
        );
        assert_eq!(
            Assets::total_balance(&XOR.into(), &alice).unwrap(),
            100000u32.into()
        );
    });
}

#[test]
fn should_fail_incoming_transfer() {
    let (mut ext, state) = ExtBuilder::default().build();
    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let bridge_acc_id = state.networks[&net_id].config.bridge_account_id.clone();
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        Assets::mint_to(&XOR.into(), &alice, &alice, 100000u32.into()).unwrap();
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[1u8; 32]),
            IncomingRequestKind::Transfer,
            net_id,
        )
        .unwrap();
        let incoming_transfer = IncomingRequest::Transfer(crate::IncomingTransfer {
            from: Address::from([1; 20]),
            to: alice.clone(),
            asset_id: XOR.into(),
            asset_kind: AssetKind::Thischain,
            amount: 100u32.into(),
            tx_hash,
            at_height: 1,
            timepoint: Default::default(),
            network_id: ETH_NETWORK_ID,
            should_take_fee: false,
        });
        assert_ok!(EthBridge::register_incoming_request(
            Origin::signed(bridge_acc_id.clone()),
            incoming_transfer.clone()
        ));
        assert!(crate::PendingIncomingRequests::<Runtime>::get(net_id).contains(&tx_hash));
        assert_eq!(
            crate::IncomingRequests::get(net_id, &tx_hash).unwrap(),
            incoming_transfer
        );
        assert_eq!(
            Assets::total_balance(&XOR.into(), &alice).unwrap(),
            100000u32.into()
        );
        assert_ok!(EthBridge::abort_request(
            Origin::signed(bridge_acc_id),
            tx_hash,
            Error::Other.into(),
            net_id,
        ));
        assert_eq!(
            crate::RequestStatuses::<Runtime>::get(net_id, &tx_hash).unwrap(),
            RequestStatus::Failed
        );
        assert!(crate::PendingIncomingRequests::<Runtime>::get(net_id).is_empty());
        assert_eq!(
            Assets::total_balance(&XOR.into(), &alice).unwrap(),
            100000u32.into()
        );
    });
}

#[test]
fn should_take_fee_in_incoming_transfer() {
    let (mut ext, state) = ExtBuilder::default().build();
    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[1u8; 32]),
            IncomingRequestKind::Transfer,
            net_id,
        )
        .unwrap();
        let incoming_transfer = IncomingRequest::Transfer(crate::IncomingTransfer {
            from: Address::from([1; 20]),
            to: alice.clone(),
            asset_id: AssetId::XOR.into(),
            asset_kind: AssetKind::SidechainOwned,
            amount: balance!(100),
            tx_hash,
            at_height: 1,
            timepoint: Default::default(),
            network_id: ETH_NETWORK_ID,
            should_take_fee: true,
        });
        assert_eq!(
            assets::Module::<Runtime>::total_balance(&AssetId::XOR.into(), &alice).unwrap(),
            0u32.into()
        );
        assert_incoming_request_done(&state, incoming_transfer.clone()).unwrap();
        assert_eq!(
            assets::Module::<Runtime>::total_balance(&AssetId::XOR.into(), &alice).unwrap(),
            balance!(99.9993).into()
        );
    });
}

#[test]
fn should_fail_take_fee_in_incoming_transfer() {
    let (mut ext, state) = ExtBuilder::default().build();
    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[1u8; 32]),
            IncomingRequestKind::Transfer,
            net_id,
        )
        .unwrap();
        let incoming_transfer = IncomingRequest::Transfer(crate::IncomingTransfer {
            from: Address::from([1; 20]),
            to: alice.clone(),
            asset_id: AssetId::XOR.into(),
            asset_kind: AssetKind::SidechainOwned,
            amount: 100u32.into(),
            tx_hash,
            at_height: 1,
            timepoint: Default::default(),
            network_id: ETH_NETWORK_ID,
            should_take_fee: true,
        });
        assert_incoming_request_registration_failed(
            &state,
            incoming_transfer.clone(),
            Error::UnableToPayFees,
        )
        .unwrap();
    });
}

#[test]
fn should_fail_registering_incoming_request_if_preparation_failed() {
    let net_id = ETH_NETWORK_ID;
    let mut builder = ExtBuilder::default();
    builder.add_currency(net_id, AssetConfig::Thischain { id: PSWAP.into() });
    let (mut ext, state) = builder.build();

    ext.execute_with(|| {
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[1u8; 32]),
            IncomingRequestKind::Transfer,
            net_id,
        )
        .unwrap();
        let incoming_transfer = IncomingRequest::Transfer(crate::IncomingTransfer {
            from: Address::from([1; 20]),
            to: alice.clone(),
            asset_id: PSWAP.into(),
            asset_kind: AssetKind::Thischain,
            amount: 100u32.into(),
            tx_hash,
            at_height: 1,
            timepoint: Default::default(),
            network_id: net_id,
            should_take_fee: false,
        });
        let bridge_acc_id = state.networks[&net_id].config.bridge_account_id.clone();
        assert_err!(
            EthBridge::register_incoming_request(
                Origin::signed(bridge_acc_id.clone()),
                incoming_transfer.clone()
            ),
            tokens::Error::<Runtime>::BalanceTooLow
        );
        assert!(!crate::PendingIncomingRequests::<Runtime>::get(net_id).contains(&tx_hash));
        assert!(crate::IncomingRequests::<Runtime>::get(net_id, &tx_hash).is_none());
        assert_eq!(
            crate::RequestStatuses::<Runtime>::get(net_id, &tx_hash).unwrap(),
            RequestStatus::Failed
        );
    });
}

#[test]
fn should_register_and_find_asset_ids() {
    let (mut ext, _state) = ExtBuilder::default().build();
    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        // gets a known asset
        let (asset_id, asset_kind) = EthBridge::get_asset_by_raw_asset_id(
            H256(AssetId32::<AssetId>::from_asset_id(AssetId::XOR).code),
            &Address::zero(),
            net_id,
        )
        .unwrap()
        .unwrap();
        assert_eq!(asset_id, XOR.into());
        assert_eq!(asset_kind, AssetKind::Thischain);
        let token_address = Address::from(hex!("7d7ff6f42e928de241282b9606c8e98ea48526e2"));
        // registers unknown token
        assert!(
            EthBridge::get_asset_by_raw_asset_id(H256::zero(), &token_address, net_id)
                .unwrap()
                .is_none()
        );
        // gets registered asset ID, associated with the token
        EthBridge::register_sidechain_asset(
            token_address,
            18,
            AssetSymbol(b"TEST".to_vec()),
            net_id,
        )
        .unwrap();
        let (asset_id, asset_kind) =
            EthBridge::get_asset_by_raw_asset_id(H256::zero(), &token_address, net_id)
                .unwrap()
                .unwrap();
        assert_eq!(
            asset_id,
            AssetId32::from_bytes(hex!(
                "00998577153deb622b5d7faabf23846281a8b074e1d4eebd31bca9dbe2c23006"
            ))
        );
        assert_eq!(asset_kind, AssetKind::Sidechain);
        assert_eq!(
            EthBridge::registered_sidechain_token(net_id, &asset_id).unwrap(),
            token_address
        );
        assert_eq!(
            EthBridge::registered_sidechain_asset(net_id, &token_address).unwrap(),
            asset_id
        );
    });
}

#[test]
fn should_convert_to_eth_address() {
    let (mut ext, _) = ExtBuilder::default().build();
    ext.execute_with(|| {
        let account_id = PublicKey::parse_slice(
            &"03b27380932f3750c416ba38c967c4e63a8c9778bac4d28a520e499525f170ae85"
                .from_hex::<Vec<u8>>()
                .unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(
            eth::public_key_to_eth_address(&account_id),
            Address::from_str("8589c3814C3c1d4d2f5C21B74c6A00fb15E5166E").unwrap()
        );
    });
}

#[test]
fn should_add_asset() {
    let (mut ext, state) = ExtBuilder::default().build();

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        let asset_id = Assets::register_from(
            &alice,
            AssetSymbol(b"TEST".to_vec()),
            18,
            Balance::from(0u32),
            true,
        )
        .unwrap();
        assert_ok!(EthBridge::add_asset(
            Origin::signed(alice.clone()),
            asset_id,
            net_id,
        ));
        assert!(EthBridge::registered_asset(net_id, asset_id).is_none());
        approve_last_request(&state, net_id).expect("request wasn't approved");
        assert_eq!(
            EthBridge::registered_asset(net_id, asset_id).unwrap(),
            AssetKind::Thischain
        );
    });
}

#[test]
fn should_add_token() {
    let (mut ext, state) = ExtBuilder::default().build();

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let token_address = Address::from(hex!("e88f8313e61a97cec1871ee37fbbe2a8bf3ed1e4"));
        let ticker = "TEST".into();
        let name = "Runtime Token".into();
        let decimals = 18;
        assert_ok!(EthBridge::add_sidechain_token(
            Origin::signed(state.authority_account_id.clone()),
            token_address,
            ticker,
            name,
            decimals,
            ETH_NETWORK_ID,
        ));
        assert!(EthBridge::registered_sidechain_asset(net_id, &token_address).is_none());
        approve_last_request(&state, net_id).expect("request wasn't approved");
        let asset_id_opt = EthBridge::registered_sidechain_asset(net_id, &token_address);
        assert!(asset_id_opt.is_some());
        assert_eq!(
            EthBridge::registered_asset(net_id, asset_id_opt.unwrap()).unwrap(),
            AssetKind::Sidechain
        );
    });
}

// TODO: enable authority account check
#[ignore]
#[test]
fn should_not_add_token_if_not_bridge_account() {
    let (mut ext, _state) = ExtBuilder::default().build();

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let bob = get_account_id_from_seed::<sr25519::Public>("Bob");
        let token_address = Address::from(hex!("e88f8313e61a97cec1871ee37fbbe2a8bf3ed1e4"));
        let ticker = "TEST".into();
        let name = "Runtime Token".into();
        let decimals = 18;
        assert_err!(
            EthBridge::add_sidechain_token(
                Origin::signed(bob),
                token_address,
                ticker,
                name,
                decimals,
                net_id,
            ),
            Error::Forbidden
        );
    });
}

#[test]
fn should_add_peer_in_eth_network() {
    let (mut ext, state) = ExtBuilder::default().build();

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let bridge_acc_id = state.networks[&net_id].config.bridge_account_id.clone();
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        let kp = ecdsa::Pair::from_string("//OCW5", None).unwrap();
        let signer = AccountPublic::from(kp.public());
        let public = PublicKey::from_secret_key(&SecretKey::parse_slice(&kp.seed()).unwrap());

        // outgoing request part
        let new_peer_id = signer.into_account();
        let new_peer_address = eth::public_key_to_eth_address(&public);
        assert_ok!(EthBridge::add_peer(
            Origin::signed(state.authority_account_id.clone()),
            new_peer_id.clone(),
            new_peer_address,
            net_id,
        ));
        assert_eq!(
            crate::PendingPeer::<Runtime>::get(net_id).unwrap(),
            new_peer_id
        );
        approve_next_request(&state, net_id).expect("request wasn't approved");
        assert_eq!(
            crate::PendingPeer::<Runtime>::get(net_id).unwrap(),
            new_peer_id
        );
        assert_eq!(
            crate::PeerAccountId::<Runtime>::get(&net_id, &new_peer_address),
            new_peer_id
        );
        assert_eq!(
            crate::PeerAddress::<Runtime>::get(net_id, &new_peer_id),
            new_peer_address
        );
        approve_next_request(&state, net_id).expect("request wasn't approved");
        // incoming request part
        // peer is added to Bridge contract
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[1u8; 32]),
            IncomingRequestKind::AddPeer,
            net_id,
        )
        .unwrap();
        let incoming_request = IncomingRequest::ChangePeers(crate::IncomingChangePeers {
            peer_account_id: new_peer_id.clone(),
            peer_address: new_peer_address,
            added: true,
            tx_hash,
            at_height: 1,
            timepoint: Default::default(),
            network_id: net_id,
        });
        assert_incoming_request_done(&state, incoming_request.clone()).unwrap();
        assert!(!crate::Peers::<Runtime>::get(net_id).contains(&new_peer_id));
        // peer is added to XOR contract
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[2u8; 32]),
            IncomingRequestKind::AddPeerCompat,
            net_id,
        )
        .unwrap();
        let incoming_request =
            IncomingRequest::ChangePeersCompat(crate::IncomingChangePeersCompat {
                peer_account_id: new_peer_id.clone(),
                peer_address: new_peer_address,
                added: true,
                contract: ChangePeersContract::XOR,
                tx_hash,
                at_height: 2,
                timepoint: Default::default(),
                network_id: net_id,
            });
        assert_incoming_request_done(&state, incoming_request.clone()).unwrap();
        assert!(!crate::Peers::<Runtime>::get(net_id).contains(&new_peer_id));
        // peer is added to VAL contract
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[3u8; 32]),
            IncomingRequestKind::AddPeerCompat,
            net_id,
        )
        .unwrap();
        let incoming_request =
            IncomingRequest::ChangePeersCompat(crate::IncomingChangePeersCompat {
                peer_account_id: new_peer_id.clone(),
                peer_address: new_peer_address,
                added: true,
                contract: ChangePeersContract::VAL,
                tx_hash,
                at_height: 3,
                timepoint: Default::default(),
                network_id: net_id,
            });
        assert!(!crate::Peers::<Runtime>::get(net_id).contains(&new_peer_id));
        assert!(crate::PendingPeer::<Runtime>::get(net_id).is_some());
        assert_incoming_request_done(&state, incoming_request.clone()).unwrap();
        assert!(crate::PendingPeer::<Runtime>::get(net_id).is_none());
        assert!(crate::Peers::<Runtime>::get(net_id).contains(&new_peer_id));
        assert!(bridge_multisig::Accounts::<Runtime>::get(&bridge_acc_id)
            .unwrap()
            .is_signatory(&new_peer_id));
    });
}

#[test]
fn should_add_peer_in_simple_networks() {
    let mut builder = ExtBuilder::default();
    let net_id = builder.add_network(vec![], None, Some(4));
    let (mut ext, state) = builder.build();

    ext.execute_with(|| {
        let bridge_acc_id = state.networks[&net_id].config.bridge_account_id.clone();
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        let kp = ecdsa::Pair::from_string("//OCW5", None).unwrap();
        let signer = AccountPublic::from(kp.public());
        let public = PublicKey::from_secret_key(&SecretKey::parse_slice(&kp.seed()).unwrap());

        // outgoing request part
        let new_peer_id = signer.into_account();
        let new_peer_address = eth::public_key_to_eth_address(&public);
        assert_ok!(EthBridge::add_peer(
            Origin::signed(state.authority_account_id.clone()),
            new_peer_id.clone(),
            new_peer_address,
            net_id,
        ));
        assert_eq!(
            crate::PendingPeer::<Runtime>::get(net_id).unwrap(),
            new_peer_id
        );
        approve_next_request(&state, net_id).expect("request wasn't approved");
        assert_eq!(
            crate::PendingPeer::<Runtime>::get(net_id).unwrap(),
            new_peer_id
        );
        assert_eq!(
            crate::PeerAccountId::<Runtime>::get(&net_id, &new_peer_address),
            new_peer_id
        );
        assert_eq!(
            crate::PeerAddress::<Runtime>::get(net_id, &new_peer_id),
            new_peer_address
        );
        // incoming request part
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[1u8; 32]),
            IncomingRequestKind::AddPeer,
            net_id,
        )
        .unwrap();
        let incoming_request = IncomingRequest::ChangePeers(crate::IncomingChangePeers {
            peer_account_id: new_peer_id.clone(),
            peer_address: new_peer_address,
            added: true,
            tx_hash,
            at_height: 1,
            timepoint: Default::default(),
            network_id: net_id,
        });
        assert!(!crate::Peers::<Runtime>::get(net_id).contains(&new_peer_id));
        assert_incoming_request_done(&state, incoming_request.clone()).unwrap();
        assert!(crate::PendingPeer::<Runtime>::get(net_id).is_none());
        assert!(crate::Peers::<Runtime>::get(net_id).contains(&new_peer_id));
        assert!(bridge_multisig::Accounts::<Runtime>::get(&bridge_acc_id)
            .unwrap()
            .is_signatory(&new_peer_id));
    });
}

#[test]
fn should_remove_peer_in_simple_network() {
    let mut builder = ExtBuilder::default();
    let net_id = builder.add_network(vec![], None, Some(5));
    let (mut ext, state) = builder.build();

    ext.execute_with(|| {
        let extended_network_config = &state.networks[&net_id];
        let bridge_acc_id = extended_network_config.config.bridge_account_id.clone();
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        let (_, peer_id, seed) = &extended_network_config.ocw_keypairs[4];
        let public = PublicKey::from_secret_key(&SecretKey::parse_slice(&seed[..]).unwrap());

        // outgoing request part
        assert_ok!(EthBridge::remove_peer(
            Origin::signed(state.authority_account_id.clone()),
            peer_id.clone(),
            net_id,
        ));
        assert_eq!(
            &crate::PendingPeer::<Runtime>::get(net_id).unwrap(),
            peer_id
        );
        assert!(crate::Peers::<Runtime>::get(net_id).contains(&peer_id));
        approve_next_request(&state, net_id).expect("request wasn't approved");
        assert_eq!(
            &crate::PendingPeer::<Runtime>::get(net_id).unwrap(),
            peer_id
        );
        assert!(!crate::Peers::<Runtime>::get(net_id).contains(&peer_id));
        assert!(!bridge_multisig::Accounts::<Runtime>::get(&bridge_acc_id)
            .unwrap()
            .is_signatory(&peer_id));

        // incoming request part
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[1u8; 32]),
            IncomingRequestKind::RemovePeer,
            net_id,
        )
        .unwrap();
        let peer_address = eth::public_key_to_eth_address(&public);
        let incoming_request = IncomingRequest::ChangePeers(crate::IncomingChangePeers {
            peer_account_id: peer_id.clone(),
            peer_address,
            added: false,
            tx_hash,
            at_height: 1,
            timepoint: Default::default(),
            network_id: net_id,
        });
        assert_incoming_request_done(&state, incoming_request.clone()).unwrap();
        assert!(crate::PendingPeer::<Runtime>::get(net_id).is_none());
        assert!(!crate::Peers::<Runtime>::get(net_id).contains(&peer_id));
        assert!(!bridge_multisig::Accounts::<Runtime>::get(&bridge_acc_id)
            .unwrap()
            .is_signatory(&peer_id));
    });
}

#[test]
fn should_remove_peer_in_eth_network() {
    let mut builder = ExtBuilder::new();
    builder.add_network(vec![], None, Some(5));
    let (mut ext, state) = builder.build();

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let extended_network_config = &state.networks[&net_id];
        let bridge_acc_id = extended_network_config.config.bridge_account_id.clone();
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        let (_, peer_id, seed) = &extended_network_config.ocw_keypairs[4];
        let public = PublicKey::from_secret_key(&SecretKey::parse_slice(&seed[..]).unwrap());

        // outgoing request part
        assert_ok!(EthBridge::remove_peer(
            Origin::signed(state.authority_account_id.clone()),
            peer_id.clone(),
            net_id,
        ));
        assert_eq!(
            &crate::PendingPeer::<Runtime>::get(net_id).unwrap(),
            peer_id
        );
        assert!(crate::Peers::<Runtime>::get(net_id).contains(&peer_id));
        approve_next_request(&state, net_id).expect("request wasn't approved");
        assert_eq!(
            &crate::PendingPeer::<Runtime>::get(net_id).unwrap(),
            peer_id
        );
        assert!(!crate::Peers::<Runtime>::get(net_id).contains(&peer_id));
        assert!(!bridge_multisig::Accounts::<Runtime>::get(&bridge_acc_id)
            .unwrap()
            .is_signatory(&peer_id));

        // incoming request part
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[1u8; 32]),
            IncomingRequestKind::RemovePeer,
            net_id,
        )
        .unwrap();
        let peer_address = eth::public_key_to_eth_address(&public);
        let incoming_request = IncomingRequest::ChangePeers(crate::IncomingChangePeers {
            peer_account_id: peer_id.clone(),
            peer_address,
            added: false,
            tx_hash,
            at_height: 1,
            timepoint: Default::default(),
            network_id: net_id,
        });
        assert_incoming_request_done(&state, incoming_request.clone()).unwrap();
        assert!(!crate::Peers::<Runtime>::get(net_id).contains(&peer_id));
        // peer is added to XOR contract
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[2u8; 32]),
            IncomingRequestKind::AddPeerCompat,
            net_id,
        )
        .unwrap();
        let incoming_request =
            IncomingRequest::ChangePeersCompat(crate::IncomingChangePeersCompat {
                peer_account_id: peer_id.clone(),
                peer_address,
                added: false,
                contract: ChangePeersContract::XOR,
                tx_hash,
                at_height: 2,
                timepoint: Default::default(),
                network_id: net_id,
            });
        assert_incoming_request_done(&state, incoming_request.clone()).unwrap();
        assert!(!crate::Peers::<Runtime>::get(net_id).contains(&peer_id));
        // peer is added to VAL contract
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[3u8; 32]),
            IncomingRequestKind::AddPeerCompat,
            net_id,
        )
        .unwrap();
        let incoming_request =
            IncomingRequest::ChangePeersCompat(crate::IncomingChangePeersCompat {
                peer_account_id: peer_id.clone(),
                peer_address,
                added: false,
                contract: ChangePeersContract::VAL,
                tx_hash,
                at_height: 3,
                timepoint: Default::default(),
                network_id: net_id,
            });
        assert!(!crate::Peers::<Runtime>::get(net_id).contains(&peer_id));
        assert_incoming_request_done(&state, incoming_request.clone()).unwrap();
        assert!(crate::PendingPeer::<Runtime>::get(net_id).is_none());
        assert!(!crate::Peers::<Runtime>::get(net_id).contains(&peer_id));
        assert!(!bridge_multisig::Accounts::<Runtime>::get(&bridge_acc_id)
            .unwrap()
            .is_signatory(&peer_id));
    });
}

#[test]
#[ignore]
fn should_not_allow_add_and_remove_peer_only_to_authority() {
    let mut builder = ExtBuilder::new();
    builder.add_network(vec![], None, Some(5));
    let (mut ext, state) = builder.build();

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let bob = get_account_id_from_seed::<sr25519::Public>("Bob");
        let (_, peer_id, _) = &state.networks[&net_id].ocw_keypairs[4];
        // TODO: enable authority account check
        assert_err!(
            EthBridge::remove_peer(Origin::signed(bob.clone()), peer_id.clone(), net_id),
            Error::Forbidden
        );
        assert_err!(
            EthBridge::add_peer(
                Origin::signed(bob.clone()),
                peer_id.clone(),
                Address::from(&hex!("2222222222222222222222222222222222222222")),
                net_id,
            ),
            Error::Forbidden
        );
    });
}

#[test]
fn should_not_allow_changing_peers_simultaneously() {
    let mut builder = ExtBuilder::new();
    builder.add_network(vec![], None, Some(5));
    let (mut ext, state) = builder.build();

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let (_, peer_id, seed) = &state.networks[&net_id].ocw_keypairs[4];
        let public = PublicKey::from_secret_key(&SecretKey::parse_slice(&seed[..]).unwrap());
        let address = eth::public_key_to_eth_address(&public);
        assert_ok!(EthBridge::remove_peer(
            Origin::signed(state.authority_account_id.clone()),
            peer_id.clone(),
            net_id,
        ));
        approve_next_request(&state, net_id).expect("request wasn't approved");
        assert_err!(
            EthBridge::remove_peer(
                Origin::signed(state.authority_account_id.clone()),
                peer_id.clone(),
                net_id,
            ),
            Error::UnknownPeerId
        );
        assert_err!(
            EthBridge::add_peer(
                Origin::signed(state.authority_account_id.clone()),
                peer_id.clone(),
                address,
                net_id,
            ),
            Error::TooManyPendingPeers
        );
    });
}

#[test]
#[ignore]
fn should_cancel_ready_outgoing_request() {
    let (mut ext, state) = ExtBuilder::default().build();
    let _ = FUNCTIONS.get_or_init(functions);
    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        // Sending request part
        Assets::mint_to(&XOR.into(), &alice, &alice, 100u32.into()).unwrap();
        assert_eq!(
            Assets::total_balance(&XOR.into(), &alice).unwrap(),
            100u32.into()
        );
        assert_ok!(EthBridge::transfer_to_sidechain(
            Origin::signed(alice.clone()),
            XOR.into(),
            Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
            100_u32.into(),
            net_id,
        ));
        assert_eq!(
            Assets::total_balance(&XOR.into(), &alice).unwrap(),
            0u32.into()
        );
        let outgoing_req = approve_last_request(&state, net_id).expect("request wasn't approved");

        // Cancelling request part
        let tx_hash = H256::from_slice(&[1u8; 32]);
        let request_hash = request_incoming(
            alice.clone(),
            tx_hash,
            IncomingRequestKind::CancelOutgoingRequest,
            net_id,
        )
        .unwrap();
        let tx_input = encode_outgoing_request_eth_call::<Runtime>(
            *RECEIVE_BY_ETHEREUM_ASSET_ADDRESS_ID.get().unwrap(),
            &outgoing_req,
        )
        .unwrap();
        let incoming_transfer =
            IncomingRequest::CancelOutgoingRequest(crate::IncomingCancelOutgoingRequest {
                request: outgoing_req.clone(),
                initial_request_hash: request_hash,
                tx_input: tx_input.clone(),
                tx_hash,
                at_height: 1,
                timepoint: Default::default(),
                network_id: ETH_NETWORK_ID,
            });

        assert_incoming_request_done(&state, incoming_transfer.clone()).unwrap();
        assert_eq!(
            Assets::total_balance(&XOR.into(), &alice).unwrap(),
            100u32.into()
        );
    });
}

#[test]
#[ignore]
fn should_fail_cancel_ready_outgoing_request_with_wrong_approvals() {
    let (mut ext, state) = ExtBuilder::default().build();
    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        // Sending request part
        Assets::mint_to(&XOR.into(), &alice, &alice, 100u32.into()).unwrap();
        assert_eq!(
            Assets::total_balance(&XOR.into(), &alice).unwrap(),
            100u32.into()
        );
        assert_ok!(EthBridge::transfer_to_sidechain(
            Origin::signed(alice.clone()),
            XOR.into(),
            Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
            100_u32.into(),
            net_id,
        ));
        assert_eq!(
            Assets::total_balance(&XOR.into(), &alice).unwrap(),
            0u32.into()
        );
        let outgoing_req = approve_last_request(&state, net_id).expect("request wasn't approved");

        // Cancelling request part
        let tx_hash = H256::from_slice(&[1u8; 32]);
        let request_hash = request_incoming(
            alice.clone(),
            tx_hash,
            IncomingRequestKind::CancelOutgoingRequest,
            net_id,
        )
        .unwrap();
        let tx_input = encode_outgoing_request_eth_call::<Runtime>(
            *RECEIVE_BY_ETHEREUM_ASSET_ADDRESS_ID.get().unwrap(),
            &outgoing_req,
        )
        .unwrap();
        let incoming_transfer =
            IncomingRequest::CancelOutgoingRequest(crate::IncomingCancelOutgoingRequest {
                request: outgoing_req.clone(),
                initial_request_hash: request_hash,
                tx_input: tx_input.clone(),
                tx_hash,
                at_height: 1,
                timepoint: Default::default(),
                network_id: ETH_NETWORK_ID,
            });

        // Insert some signature
        crate::RequestApprovals::<Runtime>::mutate(net_id, outgoing_req.hash(), |v| {
            v.insert(SignatureParams {
                r: [1; 32],
                s: [1; 32],
                v: 0,
            })
        });
        assert_incoming_request_registration_failed(
            &state,
            incoming_transfer.clone(),
            Error::InvalidContractInput,
        )
        .unwrap();
        assert_eq!(
            Assets::total_balance(&XOR.into(), &alice).unwrap(),
            0u32.into()
        );
    });
}

#[test]
#[ignore]
fn should_fail_cancel_unfinished_outgoing_request() {
    let (mut ext, state) = ExtBuilder::default().build();
    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        // Sending request part
        Assets::mint_to(&XOR.into(), &alice, &alice, 100u32.into()).unwrap();
        assert_eq!(
            Assets::total_balance(&XOR.into(), &alice).unwrap(),
            100u32.into()
        );
        assert_ok!(EthBridge::transfer_to_sidechain(
            Origin::signed(alice.clone()),
            XOR.into(),
            Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
            100_u32.into(),
            net_id,
        ));
        assert_eq!(
            Assets::total_balance(&XOR.into(), &alice).unwrap(),
            0u32.into()
        );
        let outgoing_req = last_outgoing_request(net_id).expect("request wasn't found");

        // Cancelling request part
        let tx_hash = H256::from_slice(&[1u8; 32]);
        let request_hash = request_incoming(
            alice.clone(),
            tx_hash,
            IncomingRequestKind::CancelOutgoingRequest,
            net_id,
        )
        .unwrap();
        let tx_input = encode_outgoing_request_eth_call::<Runtime>(
            *RECEIVE_BY_ETHEREUM_ASSET_ADDRESS_ID.get().unwrap(),
            &outgoing_req,
        )
        .unwrap();
        let incoming_transfer =
            IncomingRequest::CancelOutgoingRequest(crate::IncomingCancelOutgoingRequest {
                request: outgoing_req,
                initial_request_hash: request_hash,
                tx_input,
                tx_hash,
                at_height: 1,
                timepoint: Default::default(),
                network_id: ETH_NETWORK_ID,
            });
        assert_incoming_request_registration_failed(
            &state,
            incoming_transfer.clone(),
            Error::RequestIsNotReady,
        )
        .unwrap();
        assert_eq!(
            Assets::total_balance(&XOR.into(), &alice).unwrap(),
            0u32.into()
        );
    });
}

#[test]
fn should_mark_request_as_done() {
    let (mut ext, state) = ExtBuilder::default().build();
    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        Assets::mint_to(&XOR.into(), &alice, &alice, 100u32.into()).unwrap();
        assert_ok!(EthBridge::transfer_to_sidechain(
            Origin::signed(alice.clone()),
            XOR.into(),
            Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
            100_u32.into(),
            net_id,
        ));
        let outgoing_req = approve_last_request(&state, net_id).expect("request wasn't approved");
        let outgoing_req_hash = outgoing_req.hash();
        let _request_hash = request_incoming(
            alice.clone(),
            outgoing_req_hash,
            IncomingRequestKind::MarkAsDone,
            net_id,
        )
        .unwrap();
        assert_ok!(EthBridge::finalize_mark_as_done(
            Origin::signed(state.networks[&net_id].config.bridge_account_id.clone()),
            outgoing_req_hash,
            net_id,
        ));
        assert_eq!(
            crate::RequestStatuses::<Runtime>::get(net_id, outgoing_req_hash).unwrap(),
            RequestStatus::Done
        );
    });
}

#[test]
fn should_not_mark_request_as_done() {
    let (mut ext, state) = ExtBuilder::default().build();
    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        Assets::mint_to(&XOR.into(), &alice, &alice, 100u32.into()).unwrap();
        assert_ok!(EthBridge::transfer_to_sidechain(
            Origin::signed(alice.clone()),
            XOR.into(),
            Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
            100_u32.into(),
            net_id,
        ));
        let outgoing_req = last_outgoing_request(net_id).expect("request wasn't approved");
        let outgoing_req_hash = outgoing_req.hash();
        assert_noop!(
            EthBridge::request_from_sidechain(
                Origin::signed(alice.clone()),
                outgoing_req_hash,
                IncomingRequestKind::MarkAsDone,
                net_id
            ),
            Error::RequestIsNotReady
        );
        assert_noop!(
            EthBridge::finalize_mark_as_done(
                Origin::signed(state.networks[&net_id].config.bridge_account_id.clone()),
                outgoing_req_hash,
                net_id,
            ),
            Error::RequestIsNotReady
        );
        // incoming requests can't be made done
        let req_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[1u8; 32]),
            IncomingRequestKind::Transfer,
            net_id,
        )
        .unwrap();
        let incoming_transfer = IncomingRequest::Transfer(crate::IncomingTransfer {
            from: Address::from([1; 20]),
            to: alice.clone(),
            asset_id: XOR.into(),
            asset_kind: AssetKind::Thischain,
            amount: 100u32.into(),
            tx_hash: req_hash,
            at_height: 1,
            timepoint: Default::default(),
            network_id: net_id,
            should_take_fee: false,
        });
        assert_incoming_request_done(&state, incoming_transfer.clone()).unwrap();
        assert_noop!(
            EthBridge::finalize_mark_as_done(
                Origin::signed(state.networks[&net_id].config.bridge_account_id.clone()),
                req_hash,
                net_id,
            ),
            Error::RequestIsNotReady
        );
    });
}

#[test]
fn should_fail_request_to_unknown_network() {
    let (mut ext, _state) = ExtBuilder::default().build();

    ext.execute_with(|| {
        let net_id = 3;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        let asset_id = XOR.into();
        Assets::mint_to(&asset_id, &alice, &alice, 100u32.into()).unwrap();
        assert_noop!(
            EthBridge::transfer_to_sidechain(
                Origin::signed(alice.clone()),
                asset_id,
                Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
                100_u32.into(),
                net_id,
            ),
            Error::UnknownNetwork
        );

        assert_noop!(
            EthBridge::add_asset(Origin::signed(alice.clone()), asset_id, net_id,),
            Error::UnknownNetwork
        );

        assert_noop!(
            EthBridge::request_from_sidechain(
                Origin::signed(alice),
                H256::from_slice(&[1u8; 32]),
                IncomingRequestKind::Transfer,
                net_id
            ),
            Error::UnknownNetwork
        );
    });
}

#[test]
fn should_reserve_owned_asset_on_different_networks() {
    let mut builder = ExtBuilder::default();
    let net_id_0 = ETH_NETWORK_ID;
    let net_id_1 = builder.add_network(vec![], None, None);
    let (mut ext, state) = builder.build();

    ext.execute_with(|| {
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        let asset_id = XOR.into();
        Assets::mint_to(&asset_id, &alice, &alice, 100u32.into()).unwrap();
        Assets::mint_to(
            &asset_id,
            &alice,
            &state.networks[&net_id_0].config.bridge_account_id,
            100u32.into(),
        )
        .unwrap();
        Assets::mint_to(
            &asset_id,
            &alice,
            &state.networks[&net_id_1].config.bridge_account_id,
            100u32.into(),
        )
        .unwrap();
        let supply = Assets::total_issuance(&asset_id).unwrap();
        assert_ok!(EthBridge::transfer_to_sidechain(
            Origin::signed(alice.clone()),
            asset_id,
            Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
            50_u32.into(),
            net_id_0,
        ));
        approve_last_request(&state, net_id_0).expect("request wasn't approved");
        assert_ok!(EthBridge::add_asset(
            Origin::signed(alice.clone()),
            asset_id,
            net_id_1,
        ));
        approve_last_request(&state, net_id_1).expect("request wasn't approved");
        assert_ok!(EthBridge::transfer_to_sidechain(
            Origin::signed(alice.clone()),
            asset_id,
            Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
            50_u32.into(),
            net_id_1,
        ));
        approve_last_request(&state, net_id_1).expect("request wasn't approved");
        assert_eq!(Assets::total_issuance(&asset_id).unwrap(), supply);

        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[1u8; 32]),
            IncomingRequestKind::Transfer,
            net_id_0,
        )
        .unwrap();
        let incoming_transfer = IncomingRequest::Transfer(crate::IncomingTransfer {
            from: Address::from([1; 20]),
            to: alice.clone(),
            asset_id,
            asset_kind: AssetKind::Thischain,
            amount: 50u32.into(),
            tx_hash,
            at_height: 1,
            timepoint: Default::default(),
            network_id: net_id_0,
            should_take_fee: false,
        });
        assert_incoming_request_done(&state, incoming_transfer.clone()).unwrap();
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[2; 32]),
            IncomingRequestKind::Transfer,
            net_id_1,
        )
        .unwrap();
        let incoming_transfer = IncomingRequest::Transfer(crate::IncomingTransfer {
            from: Address::from([1; 20]),
            to: alice.clone(),
            asset_id,
            asset_kind: AssetKind::Thischain,
            amount: 50u32.into(),
            tx_hash,
            at_height: 1,
            timepoint: Default::default(),
            network_id: net_id_1,
            should_take_fee: false,
        });
        assert_incoming_request_done(&state, incoming_transfer.clone()).unwrap();
        assert_eq!(Assets::total_issuance(&asset_id).unwrap(), supply);
    });
}

#[test]
fn should_handle_sidechain_and_thischain_asset_on_different_networks() {
    let mut builder = ExtBuilder::default();
    let net_id_0 = ETH_NETWORK_ID;
    let net_id_1 = builder.add_network(vec![], None, None);
    let (mut ext, state) = builder.build();

    ext.execute_with(|| {
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        // Register token on the first network.
        let token_address = Address::from(hex!("e88f8313e61a97cec1871ee37fbbe2a8bf3ed1e4"));
        assert_ok!(EthBridge::add_sidechain_token(
            Origin::signed(state.authority_account_id.clone()),
            token_address,
            "TEST".into(),
            "Runtime Token".into(),
            18,
            net_id_0,
        ));
        approve_last_request(&state, net_id_0).expect("request wasn't approved");
        let asset_id = EthBridge::registered_sidechain_asset(net_id_0, &token_address)
            .expect("Asset wasn't found.");
        assert_eq!(
            EthBridge::registered_asset(net_id_0, asset_id).unwrap(),
            AssetKind::Sidechain
        );

        // Register the newly generated asset in the second network
        assert_ok!(EthBridge::add_asset(
            Origin::signed(alice.clone()),
            asset_id,
            net_id_1,
        ));
        approve_last_request(&state, net_id_1).expect("request wasn't approved");
        assert_eq!(
            EthBridge::registered_asset(net_id_1, asset_id).unwrap(),
            AssetKind::Thischain
        );
        Assets::mint_to(
            &asset_id,
            &state.networks[&net_id_0].config.bridge_account_id,
            &state.networks[&net_id_1].config.bridge_account_id,
            100u32.into(),
        )
        .unwrap();
        let supply = Assets::total_issuance(&asset_id).unwrap();
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[1u8; 32]),
            IncomingRequestKind::Transfer,
            net_id_0,
        )
        .unwrap();
        let incoming_transfer = IncomingRequest::Transfer(crate::IncomingTransfer {
            from: Address::from([1; 20]),
            to: alice.clone(),
            asset_id,
            asset_kind: AssetKind::Sidechain,
            amount: 50u32.into(),
            tx_hash,
            at_height: 1,
            timepoint: Default::default(),
            network_id: net_id_0,
            should_take_fee: false,
        });
        assert_incoming_request_done(&state, incoming_transfer.clone()).unwrap();

        assert_ok!(EthBridge::transfer_to_sidechain(
            Origin::signed(alice.clone()),
            asset_id,
            Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
            50_u32.into(),
            net_id_1,
        ));
        approve_last_request(&state, net_id_1).expect("request wasn't approved");

        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[2; 32]),
            IncomingRequestKind::Transfer,
            net_id_1,
        )
        .unwrap();
        let incoming_transfer = IncomingRequest::Transfer(crate::IncomingTransfer {
            from: Address::from([1; 20]),
            to: alice.clone(),
            asset_id,
            asset_kind: AssetKind::Thischain,
            amount: 50u32.into(),
            tx_hash,
            at_height: 1,
            timepoint: Default::default(),
            network_id: net_id_1,
            should_take_fee: false,
        });
        assert_incoming_request_done(&state, incoming_transfer.clone()).unwrap();

        assert_ok!(EthBridge::transfer_to_sidechain(
            Origin::signed(alice.clone()),
            asset_id,
            Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
            50_u32.into(),
            net_id_0,
        ));
        approve_last_request(&state, net_id_0).expect("request wasn't approved");
        assert_eq!(Assets::total_issuance(&asset_id).unwrap(), supply);
    });
}

#[test]
fn should_migrate() {
    let (mut ext, state) = ExtBuilder::default().build();

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");

        // preparation phase
        assert_ok!(EthBridge::prepare_for_migration(
            Origin::signed(state.authority_account_id.clone()),
            net_id,
        ));
        approve_last_request(&state, net_id).expect("request wasn't approved");
        assert_eq!(
            crate::BridgeStatuses::<Runtime>::get(net_id).unwrap(),
            BridgeStatus::Initialized
        );
        let contract_address = crate::BridgeContractAddress::<Runtime>::get(net_id);

        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[10; 32]),
            IncomingRequestKind::PrepareForMigration,
            net_id,
        )
        .unwrap();
        let incoming_transfer =
            IncomingRequest::PrepareForMigration(crate::IncomingPrepareForMigration {
                tx_hash,
                at_height: 1,
                timepoint: Default::default(),
                network_id: net_id,
            });
        assert_incoming_request_done(&state, incoming_transfer.clone()).unwrap();
        assert_eq!(
            crate::BridgeStatuses::<Runtime>::get(net_id).unwrap(),
            BridgeStatus::Migrating
        );

        // Disallow outgoing requests (except `Migrate` request)
        assert_noop!(
            EthBridge::transfer_to_sidechain(
                Origin::signed(alice.clone()),
                XOR.into(),
                Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
                100_u32.into(),
                net_id,
            ),
            Error::ContractIsInMigrationStage
        );

        // migration phase
        let new_contract_address = Address::from([2u8; 20]);
        let erc20_native_tokens = vec![Address::from([11u8; 20]), Address::from([22u8; 20])];
        assert_ok!(EthBridge::migrate(
            Origin::signed(state.authority_account_id.clone()),
            new_contract_address,
            erc20_native_tokens,
            net_id,
        ));
        approve_last_request(&state, net_id).expect("request wasn't approved");
        assert_eq!(
            crate::BridgeStatuses::<Runtime>::get(net_id).unwrap(),
            BridgeStatus::Migrating
        );
        assert_eq!(
            crate::BridgeContractAddress::<Runtime>::get(net_id),
            contract_address
        );

        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[20; 32]),
            IncomingRequestKind::Migrate,
            net_id,
        )
        .unwrap();
        let incoming_transfer = IncomingRequest::Migrate(crate::IncomingMigrate {
            new_contract_address,
            tx_hash,
            at_height: 2,
            timepoint: Default::default(),
            network_id: net_id,
        });
        assert_incoming_request_done(&state, incoming_transfer.clone()).unwrap();
        assert_eq!(
            crate::BridgeStatuses::<Runtime>::get(net_id).unwrap(),
            BridgeStatus::Initialized
        );
        assert_eq!(
            crate::BridgeContractAddress::<Runtime>::get(net_id),
            new_contract_address
        );
    });
}

#[test]
fn should_not_allow_duplicate_migration_requests() {
    let (mut ext, state) = ExtBuilder::default().build();

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");

        // preparation phase
        assert_ok!(EthBridge::prepare_for_migration(
            Origin::signed(state.authority_account_id.clone()),
            net_id,
        ));
        approve_last_request(&state, net_id).expect("request wasn't approved");

        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[10; 32]),
            IncomingRequestKind::PrepareForMigration,
            net_id,
        )
        .unwrap();
        let incoming_transfer =
            IncomingRequest::PrepareForMigration(crate::IncomingPrepareForMigration {
                tx_hash,
                at_height: 1,
                timepoint: Default::default(),
                network_id: net_id,
            });
        assert_incoming_request_done(&state, incoming_transfer.clone()).unwrap();

        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[100; 32]),
            IncomingRequestKind::PrepareForMigration,
            net_id,
        )
        .unwrap();
        let incoming_transfer =
            IncomingRequest::PrepareForMigration(crate::IncomingPrepareForMigration {
                tx_hash,
                at_height: 2,
                timepoint: Default::default(),
                network_id: net_id,
            });
        assert_incoming_request_registration_failed(
            &state,
            incoming_transfer.clone(),
            Error::ContractIsAlreadyInMigrationStage,
        )
        .unwrap();

        // migration phase
        let new_contract_address = Address::from([2u8; 20]);
        let erc20_native_tokens = vec![Address::from([11u8; 20]), Address::from([22u8; 20])];
        assert_ok!(EthBridge::migrate(
            Origin::signed(state.authority_account_id.clone()),
            new_contract_address,
            erc20_native_tokens.clone(),
            net_id,
        ));
        approve_last_request(&state, net_id).expect("request wasn't approved");

        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[20; 32]),
            IncomingRequestKind::Migrate,
            net_id,
        )
        .unwrap();
        let incoming_transfer = IncomingRequest::Migrate(crate::IncomingMigrate {
            new_contract_address,
            tx_hash,
            at_height: 2,
            timepoint: Default::default(),
            network_id: net_id,
        });
        assert_incoming_request_done(&state, incoming_transfer.clone()).unwrap();

        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[200; 32]),
            IncomingRequestKind::Migrate,
            net_id,
        )
        .unwrap();
        let incoming_transfer = IncomingRequest::Migrate(crate::IncomingMigrate {
            new_contract_address,
            tx_hash,
            at_height: 2,
            timepoint: Default::default(),
            network_id: net_id,
        });
        assert_incoming_request_registration_failed(
            &state,
            incoming_transfer.clone(),
            Error::ContractIsNotInMigrationStage,
        )
        .unwrap();
    });
}

#[test]
fn should_ensure_known_contract() {
    let (mut ext, _state) = ExtBuilder::default().build();

    ext.execute_with(|| {
        assert_ok!(EthBridge::ensure_known_contract(
            EthBridge::xor_master_contract_address(),
            ETH_NETWORK_ID,
            IncomingRequestKind::Transfer,
        ));
        assert_ok!(EthBridge::ensure_known_contract(
            EthBridge::val_master_contract_address(),
            ETH_NETWORK_ID,
            IncomingRequestKind::Transfer,
        ));
        assert_ok!(EthBridge::ensure_known_contract(
            crate::BridgeContractAddress::<Runtime>::get(ETH_NETWORK_ID),
            ETH_NETWORK_ID,
            IncomingRequestKind::Transfer,
        ));
        assert_err!(
            EthBridge::ensure_known_contract(
                EthBridge::xor_master_contract_address(),
                100,
                IncomingRequestKind::Transfer,
            ),
            Error::UnknownContractAddress
        );
    });
}

#[test]
fn should_parse_add_peer_on_old_contract() {
    let (mut ext, state) = ExtBuilder::default().build();
    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");

        let kp = ecdsa::Pair::from_string("//Bob", None).unwrap();
        let signer = AccountPublic::from(kp.public());
        let public = PublicKey::from_secret_key(&SecretKey::parse_slice(&kp.seed()).unwrap());
        let new_peer_id = signer.into_account();
        let new_peer_address = eth::public_key_to_eth_address(&public);
        assert_ok!(EthBridge::add_peer(
            Origin::signed(state.authority_account_id.clone()),
            new_peer_id.clone(),
            new_peer_address,
            net_id,
        ));
        approve_next_request(&state, net_id).expect("request wasn't approved");
        approve_next_request(&state, net_id).expect("request wasn't approved");

        let tx_hash = H256([1; 32]);
        // add peer
        let incoming_request = IncomingPreRequest::<Runtime> {
            author: alice.clone(),
            hash: tx_hash,
            timepoint: Default::default(),
            kind: IncomingRequestKind::AddPeer,
            network_id: net_id,
        };
        let tx = Transaction {
            input: Bytes(hex!("ca70cf6e00000000000000000000000025451a4de12dccc2d166922fa938e900fcc4ed24441b7425bbf44fe617047e8f4cea8c47be35c8828257aa5793c08167e7c715eb00000000000000000000000000000000000000000000000000000000000000a000000000000000000000000000000000000000000000000000000000000000e000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000000001000000000000000000000000000000000000000000000000000000000000008900000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000").to_vec()),
            block_number: Some(1u64.into()),
            to: Some(types::H160(EthBridge::xor_master_contract_address().0)),
            ..Default::default()
        };
        let inc_req = EthBridge::parse_old_incoming_request_method_call(incoming_request, tx).unwrap();
        assert_eq!(
            inc_req,
            IncomingRequest::ChangePeersCompat(IncomingChangePeersCompat {
                peer_account_id: new_peer_id.clone(),
                peer_address: new_peer_address,
                added: true,
                contract: ChangePeersContract::XOR,
                tx_hash,
                at_height: 1,
                timepoint: Default::default(),
                network_id: net_id
            })
        );
    });
}

#[test]
fn should_parse_remove_peer_on_old_contract() {
    let (mut ext, state) = ExtBuilder::default().build();
    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");

        let kp = ecdsa::Pair::from_string("//Bob", None).unwrap();
        let signer = AccountPublic::from(kp.public());
        let public = PublicKey::from_secret_key(&SecretKey::parse_slice(&kp.seed()).unwrap());
        let new_peer_id = signer.into_account();
        let new_peer_address = eth::public_key_to_eth_address(&public);
        let tx_hash = H256([1; 32]);
        assert_ok!(EthBridge::force_add_peer(Origin::root(), new_peer_id.clone(), new_peer_address, net_id));
        assert_ok!(EthBridge::remove_peer(
            Origin::signed(state.authority_account_id.clone()),
            new_peer_id.clone(),
            net_id,
        ));

        let incoming_request = IncomingPreRequest::<Runtime> {
            author: alice.clone(),
            hash: tx_hash,
            timepoint: Default::default(),
            kind: IncomingRequestKind::RemovePeer,
            network_id: net_id,
        };
        let tx = Transaction {
            input: Bytes(hex!("89c39baf00000000000000000000000025451a4de12dccc2d166922fa938e900fcc4ed24451d32cbef7d41bbc741949402308c03fe43ab4efe4aa8f83c21e732c9e1ca1c00000000000000000000000000000000000000000000000000000000000000a000000000000000000000000000000000000000000000000000000000000000c000000000000000000000000000000000000000000000000000000000000000e0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000").to_vec()),
            block_number: Some(1u64.into()),
            to: Some(types::H160(EthBridge::val_master_contract_address().0)),
            ..Default::default()
        };
        assert_eq!(
            EthBridge::parse_old_incoming_request_method_call(incoming_request, tx).unwrap(),
            IncomingRequest::ChangePeersCompat(IncomingChangePeersCompat {
                peer_account_id: new_peer_id,
                peer_address: new_peer_address,
                added: false,
                contract: ChangePeersContract::VAL,
                tx_hash,
                at_height: 1,
                timepoint: Default::default(),
                network_id: net_id
            })
        );
    });
}

#[test]
fn should_use_different_abi_when_sending_xor_val_on_non_eth_network() {
    let (mut ext, _state) = ExtBuilder::default().build();
    ext.execute_with(|| {
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        for asset_id in &[XOR, VAL] {
            let transfer_to_eth = OutgoingTransfer::<Runtime> {
                from: alice.clone(),
                to: Address::from([1; 20]),
                asset_id: *asset_id,
                amount: 100_u32.into(),
                nonce: 0,
                network_id: ETH_NETWORK_ID,
                timepoint: Default::default(),
            };
            let transfer_to_non_eth = OutgoingTransfer::<Runtime> {
                from: alice.clone(),
                to: Address::from([1; 20]),
                asset_id: *asset_id,
                amount: 100_u32.into(),
                nonce: 0,
                network_id: 100,
                timepoint: Default::default(),
            };
            assert_ne!(
                transfer_to_eth.to_eth_abi(H256::zero()).unwrap().raw,
                transfer_to_non_eth.to_eth_abi(H256::zero()).unwrap().raw
            );
        }
    });
}

#[test]
fn should_cancel_outgoing_prepared_requests() {
    let net_id = ETH_NETWORK_ID;
    let builder = ExtBuilder::default();
    let (mut ext, state) = builder.build();
    ext.execute_with(|| {
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        let bridge_acc = &state.networks[&net_id].config.bridge_account_id;
        Assets::register_asset_id(
            alice.clone(),
            DOT,
            AssetSymbol::from_str("DOT").unwrap(),
            18,
            0,
            true,
        )
        .unwrap();
        Assets::mint_to(&XOR.into(), &alice, &alice, 100u32.into()).unwrap();
        Assets::mint_to(&XOR.into(), &alice, bridge_acc, 100u32.into()).unwrap();
        let ocw0_account_id = &state.networks[&net_id].ocw_keypairs[0].1;
        // Paris (preparation requests, testable request).
        let requests: Vec<(Vec<OffchainRequest<Runtime>>, OffchainRequest<Runtime>)> = vec![
            (
                vec![],
                OutgoingTransfer {
                    from: alice.clone(),
                    to: Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
                    asset_id: XOR.into(),
                    amount: 1_u32.into(),
                    nonce: 0,
                    network_id: net_id,
                    timepoint: Default::default(),
                }
                .into(),
            ),
            (
                vec![],
                OutgoingAddAsset {
                    author: alice.clone(),
                    asset_id: DOT.into(),
                    nonce: 0,
                    network_id: net_id,
                    timepoint: Default::default(),
                }
                .into(),
            ),
            (
                vec![],
                OutgoingAddToken {
                    author: alice.clone(),
                    token_address: Address::from([100u8; 20]),
                    name: "TEST".into(),
                    ticker: "TST".into(),
                    decimals: 18,
                    nonce: 0,
                    network_id: net_id,
                    timepoint: Default::default(),
                }
                .into(),
            ),
            (
                vec![],
                OutgoingAddPeer {
                    author: alice.clone(),
                    peer_address: Address::from([10u8; 20]),
                    nonce: 0,
                    network_id: net_id,
                    peer_account_id: AccountId32::new([10u8; 32]),
                    timepoint: Default::default(),
                }
                .into(),
            ),
            (
                vec![OutgoingAddPeer {
                    author: alice.clone(),
                    peer_address: Address::from([10u8; 20]),
                    nonce: 0,
                    network_id: net_id,
                    peer_account_id: AccountId32::new([10u8; 32]),
                    timepoint: Default::default(),
                }
                .into()],
                OutgoingAddPeerCompat {
                    author: alice.clone(),
                    peer_address: Address::from([10u8; 20]),
                    nonce: 0,
                    network_id: net_id,
                    peer_account_id: AccountId32::new([10u8; 32]),
                    timepoint: Default::default(),
                }
                .into(),
            ),
            (
                vec![],
                OutgoingRemovePeer {
                    author: alice.clone(),
                    peer_address: crate::PeerAddress::<Runtime>::get(&net_id, &ocw0_account_id),
                    nonce: 0,
                    network_id: net_id,
                    peer_account_id: ocw0_account_id.clone(),
                    timepoint: Default::default(),
                }
                .into(),
            ),
            (
                vec![OutgoingRemovePeer {
                    author: alice.clone(),
                    peer_address: crate::PeerAddress::<Runtime>::get(&net_id, &ocw0_account_id),
                    nonce: 0,
                    network_id: net_id,
                    peer_account_id: ocw0_account_id.clone(),
                    timepoint: Default::default(),
                }
                .into()],
                OutgoingRemovePeerCompat {
                    author: alice.clone(),
                    peer_address: crate::PeerAddress::<Runtime>::get(&net_id, &ocw0_account_id),
                    nonce: 0,
                    network_id: net_id,
                    peer_account_id: ocw0_account_id.clone(),
                    timepoint: Default::default(),
                }
                .into(),
            ),
            (
                vec![],
                OutgoingPrepareForMigration {
                    author: alice.clone(),
                    nonce: 0,
                    network_id: net_id,
                    timepoint: Default::default(),
                }
                .into(),
            ),
            (
                vec![OutgoingPrepareForMigration {
                    author: alice.clone(),
                    nonce: 0,
                    network_id: net_id,
                    timepoint: Default::default(),
                }
                .into()],
                OutgoingMigrate {
                    author: alice.clone(),
                    new_contract_address: Default::default(),
                    erc20_native_tokens: vec![],
                    nonce: 0,
                    network_id: net_id,
                    timepoint: Default::default(),
                }
                .into(),
            ),
        ];
        for (preparations, mut request) in requests {
            frame_support::storage::with_transaction(|| {
                for mut preparation_request in preparations {
                    preparation_request.validate().unwrap();
                    preparation_request.prepare().unwrap();
                    // preparation_request.finalize().unwrap();
                }
                // Save the current storage root hash, apply transaction preparation,
                // cancel it and compare with the final root hash.
                frame_system::Pallet::<Runtime>::reset_events();
                let state_hash_before = frame_support::storage_root();
                request.validate().unwrap();
                request.prepare().unwrap();
                request.cancel().unwrap();
                frame_system::Pallet::<Runtime>::reset_events();
                let state_hash_after = frame_support::storage_root();
                assert_eq!(state_hash_before, state_hash_after);
                TransactionOutcome::Rollback(())
            });
        }
    });
}

#[test]
fn should_cancel_incoming_prepared_requests() {
    let net_id = ETH_NETWORK_ID;
    let mut builder = ExtBuilder::default();
    builder.add_currency(net_id, AssetConfig::Thischain { id: DOT.into() });
    builder.add_currency(
        net_id,
        AssetConfig::Sidechain {
            id: USDT.into(),
            sidechain_id: H160(hex!("dAC17F958D2ee523a2206206994597C13D831ec7")),
            owned: false,
            precision: 18,
        },
    );
    let (mut ext, state) = builder.build();
    ext.execute_with(|| {
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        let bridge_acc = &state.networks[&net_id].config.bridge_account_id;
        Assets::mint_to(&XOR.into(), &alice, &alice, 100u32.into()).unwrap();
        Assets::mint_to(&XOR.into(), &alice, bridge_acc, 100u32.into()).unwrap();
        Assets::mint_to(&DOT.into(), &alice, bridge_acc, 100u32.into()).unwrap();
        // Paris (preparation requests, testable request).
        let requests: Vec<(Vec<IncomingRequest<Runtime>>, IncomingRequest<Runtime>)> = vec![
            (
                vec![],
                IncomingTransfer {
                    from: Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
                    to: alice.clone(),
                    asset_id: XOR.into(),
                    asset_kind: AssetKind::SidechainOwned,
                    amount: 1_u32.into(),
                    tx_hash: Default::default(),
                    network_id: net_id,
                    timepoint: Default::default(),
                    at_height: 0,
                    should_take_fee: false,
                }
                .into(),
            ),
            (
                vec![],
                IncomingTransfer {
                    from: Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
                    to: alice.clone(),
                    asset_id: DOT.into(),
                    asset_kind: AssetKind::Thischain,
                    amount: 1_u32.into(),
                    tx_hash: Default::default(),
                    network_id: net_id,
                    timepoint: Default::default(),
                    at_height: 0,
                    should_take_fee: false,
                }
                .into(),
            ),
            (
                vec![],
                IncomingTransfer {
                    from: Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
                    to: alice.clone(),
                    asset_id: USDT.into(),
                    asset_kind: AssetKind::Sidechain,
                    amount: 1_u32.into(),
                    tx_hash: Default::default(),
                    network_id: net_id,
                    timepoint: Default::default(),
                    at_height: 0,
                    should_take_fee: false,
                }
                .into(),
            ),
            (
                vec![],
                IncomingAddToken {
                    token_address: Address::from([100; 20]),
                    asset_id: KSM.into(),
                    precision: 18,
                    symbol: Default::default(),
                    tx_hash: Default::default(),
                    network_id: net_id,
                    timepoint: Default::default(),
                    at_height: 0,
                }
                .into(),
            ),
            (
                vec![],
                IncomingPrepareForMigration {
                    tx_hash: Default::default(),
                    network_id: net_id,
                    timepoint: Default::default(),
                    at_height: 0,
                }
                .into(),
            ),
            (
                vec![IncomingPrepareForMigration {
                    tx_hash: Default::default(),
                    network_id: net_id,
                    timepoint: Default::default(),
                    at_height: 0,
                }
                .into()],
                IncomingMigrate {
                    new_contract_address: Default::default(),
                    tx_hash: Default::default(),
                    network_id: net_id,
                    timepoint: Default::default(),
                    at_height: 0,
                }
                .into(),
            ),
            // TODO: test incoming 'cancel outgoing request'
        ];
        for (preparations, request) in requests {
            frame_support::storage::with_transaction(|| {
                for preparation_request in preparations {
                    preparation_request.prepare().unwrap();
                    preparation_request.finalize().unwrap();
                }
                // Save the current storage root hash, apply transaction preparation,
                // cancel it and compare with the final root hash.
                frame_system::Pallet::<Runtime>::reset_events();
                let state_hash_before = frame_support::storage_root();
                request.prepare().unwrap();
                request.cancel().unwrap();
                frame_system::Pallet::<Runtime>::reset_events();
                let state_hash_after = frame_support::storage_root();
                assert_eq!(state_hash_before, state_hash_after);
                TransactionOutcome::Rollback(())
            });
        }
    });
}

#[test]
fn should_convert_amount_for_a_token_with_non_default_precision() {
    let (mut ext, state) = ExtBuilder::default().build();

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        let token_address = Address::from(hex!("e88f8313e61a97cec1871ee37fbbe2a8bf3ed1e4"));
        let ticker = "USDT".into();
        let name = "Tether USD".into();
        let decimals = 6;
        assert_ok!(EthBridge::add_sidechain_token(
            Origin::signed(state.authority_account_id.clone()),
            token_address,
            ticker,
            name,
            decimals,
            net_id,
        ));
        assert!(EthBridge::registered_sidechain_asset(net_id, &token_address).is_none());
        approve_last_request(&state, net_id).expect("request wasn't approved");
        let asset_id = EthBridge::registered_sidechain_asset(net_id, &token_address)
            .expect("failed to register sidechain asset");
        assert_eq!(
            EthBridge::registered_asset(net_id, &asset_id).unwrap(),
            AssetKind::Sidechain
        );
        assert_eq!(
            EthBridge::sidechain_asset_precision(net_id, &asset_id),
            decimals
        );
        assert_eq!(
            Assets::get_asset_info(&asset_id).1,
            DEFAULT_BALANCE_PRECISION
        );
        // Incoming transfer part.
        assert_eq!(
            Assets::total_balance(&asset_id, &alice).unwrap(),
            balance!(0)
        );
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[1; 32]),
            IncomingRequestKind::Transfer,
            net_id,
        )
        .unwrap();
        let sidechain_amount = 1 * 10_u128.pow(decimals as u32);
        let incoming_trasfer = IncomingRequest::try_from_contract_event(
            ContractEvent::Deposit(alice.clone(), sidechain_amount, token_address, H256::zero()),
            IncomingPreRequest::new(
                alice.clone(),
                tx_hash,
                Default::default(),
                IncomingRequestKind::Transfer,
                net_id,
            ),
            1,
            tx_hash,
        )
        .unwrap();
        assert_incoming_request_done(&state, incoming_trasfer).unwrap();
        assert_eq!(
            Assets::total_balance(&asset_id, &alice).unwrap(),
            balance!(1)
        );
        // Outgoing transfer part.
        assert_ok!(EthBridge::transfer_to_sidechain(
            Origin::signed(alice.clone()),
            asset_id.clone(),
            Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
            balance!(1),
            net_id,
        ));
        let outgoing_transfer =
            match approve_last_request(&state, net_id).expect("request wasn't approved") {
                OutgoingRequest::Transfer(transfer) => transfer,
                _ => unreachable!(),
            };
        assert_eq!(outgoing_transfer.amount, balance!(1));
        assert_eq!(
            outgoing_transfer.sidechain_amount().unwrap().0,
            sidechain_amount
        );
        assert_eq!(
            Assets::total_balance(&asset_id, &alice).unwrap(),
            balance!(0)
        );
    });
}

#[test]
fn should_fail_convert_amount_for_a_token_with_non_default_precision() {
    let (mut ext, state) = ExtBuilder::default().build();

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        let token_address = Address::from(hex!("e88f8313e61a97cec1871ee37fbbe2a8bf3ed1e4"));
        let ticker = "USDT".into();
        let name = "Tether USD".into();
        let decimals = 6;
        assert_ok!(EthBridge::add_sidechain_token(
            Origin::signed(state.authority_account_id.clone()),
            token_address,
            ticker,
            name,
            decimals,
            net_id,
        ));
        assert!(EthBridge::registered_sidechain_asset(net_id, &token_address).is_none());
        approve_last_request(&state, net_id).expect("request wasn't approved");
        let asset_id = EthBridge::registered_sidechain_asset(net_id, &token_address)
            .expect("failed to register sidechain asset");
        assert_eq!(
            Assets::total_balance(&asset_id, &alice).unwrap(),
            balance!(0)
        );
        let tx_hash = request_incoming(
            alice.clone(),
            H256::from_slice(&[1; 32]),
            IncomingRequestKind::Transfer,
            net_id,
        )
        .unwrap();
        let sidechain_amount = 1_000_000_000_000_000_000_000 * 10_u128.pow(decimals as u32);
        let incoming_trasfer_result = IncomingRequest::try_from_contract_event(
            ContractEvent::Deposit(alice.clone(), sidechain_amount, token_address, H256::zero()),
            IncomingPreRequest::new(
                alice.clone(),
                tx_hash,
                Default::default(),
                IncomingRequestKind::Transfer,
                net_id,
            ),
            1,
            tx_hash,
        );
        assert_eq!(
            incoming_trasfer_result,
            Err(Error::UnsupportedAssetPrecision)
        );
    });
}

#[test]
fn should_fail_tranfer_amount_with_dust_for_a_token_with_non_default_precision() {
    let (mut ext, state) = ExtBuilder::default().build();

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let alice = get_account_id_from_seed::<sr25519::Public>("Alice");
        let token_address = Address::from(hex!("e88f8313e61a97cec1871ee37fbbe2a8bf3ed1e4"));
        let ticker = "USDT".into();
        let name = "Tether USD".into();
        let decimals = 6;
        assert_ok!(EthBridge::add_sidechain_token(
            Origin::signed(state.authority_account_id.clone()),
            token_address,
            ticker,
            name,
            decimals,
            net_id,
        ));
        assert!(EthBridge::registered_sidechain_asset(net_id, &token_address).is_none());
        approve_last_request(&state, net_id).expect("request wasn't approved");
        let asset_id = EthBridge::registered_sidechain_asset(net_id, &token_address)
            .expect("failed to register sidechain asset");
        assert_eq!(
            Assets::total_balance(&asset_id, &alice).unwrap(),
            balance!(0)
        );
        Assets::mint_to(
            &asset_id,
            &state.networks[&net_id].config.bridge_account_id,
            &alice,
            balance!(0.1000009),
        )
        .unwrap();
        assert_noop!(
            EthBridge::transfer_to_sidechain(
                Origin::signed(alice.clone()),
                asset_id.clone(),
                Address::from_str("19E7E376E7C213B7E7e7e46cc70A5dD086DAff2A").unwrap(),
                balance!(0.1000009),
                net_id,
            ),
            Error::NonZeroDust
        );
    });
}

#[test]
fn should_not_allow_registering_sidechain_token_with_big_precision() {
    let (mut ext, state) = ExtBuilder::default().build();

    ext.execute_with(|| {
        let net_id = ETH_NETWORK_ID;
        let token_address = Address::from(hex!("e88f8313e61a97cec1871ee37fbbe2a8bf3ed1e4"));
        let ticker = "USDT".into();
        let name = "Tether USD".into();
        let decimals = DEFAULT_BALANCE_PRECISION + 1;
        assert_noop!(
            EthBridge::add_sidechain_token(
                Origin::signed(state.authority_account_id.clone()),
                token_address,
                ticker,
                name,
                decimals,
                net_id,
            ),
            Error::UnsupportedAssetPrecision
        );
    });
}

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

use super::AssetInfo;
use crate::cli::prelude::*;
use bridge_types::H160;
use std::path::PathBuf;
use substrate_gen::BridgeSignatureVersion;

#[derive(Args, Clone, Debug)]
pub struct Command {
    #[clap(flatten)]
    sub: SubstrateClient,
    /// Bridge network id
    #[clap(short, long)]
    network: u32,
    /// Bridge contract address
    #[clap(short, long)]
    contract: H160,
    /// Assets to migrate
    #[clap(short, long)]
    input: PathBuf,
}

impl Command {
    pub(super) async fn run(&self) -> AnyResult<()> {
        let sub = self.sub.get_signed_substrate().await?;

        let file = std::fs::OpenOptions::new().read(true).open(&self.input)?;
        let infos: Vec<AssetInfo> = serde_json::from_reader(file)?;
        let mut addresses = vec![];
        for info in infos {
            if info.kind == "0x01" {
                if let Some(address) = info.address {
                    addresses.push(address);
                }
            }
        }

        info!("Send migrate extrinsic");

        sub.api()
            .tx()
            .sign_and_submit_then_watch_default(
                &runtime::tx()
                    .sudo()
                    .sudo(sub_types::framenode_runtime::RuntimeCall::EthBridge(
                        sub_types::eth_bridge::pallet::Call::migrate {
                            new_contract_address: self.contract,
                            erc20_native_tokens: addresses,
                            network_id: self.network,
                            new_signature_version: BridgeSignatureVersion::V2,
                        },
                    )),
                &sub,
            )
            .await?
            .wait_for_in_block()
            .await?
            .wait_for_success()
            .await?;

        Ok(())
    }
}

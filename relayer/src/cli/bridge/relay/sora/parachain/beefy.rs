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

use crate::cli::prelude::*;
use crate::relay::parachain::RelayBuilder;

#[derive(Args, Clone, Debug)]
pub(crate) struct Command {
    #[clap(flatten)]
    sub: SubstrateClient,
    #[clap(flatten)]
    para: ParachainClient,
    /// Send all Beefy commitments
    #[clap(short, long)]
    send_unneeded_commitments: bool,
    /// Minimal block for commitment search
    #[clap(long, default_value = "1")]
    from_block: u32,
}

impl Command {
    pub(super) async fn run(&self) -> AnyResult<()> {
        let sender = self.sub.get_unsigned_substrate().await?;
        let receiver = self.para.get_signed_substrate().await?;
        let syncer = crate::relay::beefy_syncer::BeefySyncer::new();
        let beefy_relay = RelayBuilder::new()
            .with_sender_client(sender.clone())
            .with_receiver_client(receiver.clone())
            .with_syncer(syncer.clone())
            .build()
            .await
            .context("build sora to sora relay")?;
        let messages_relay = crate::relay::parachain_messages::RelayBuilder::new()
            .with_sender_client(sender)
            .with_receiver_client(receiver.unsigned())
            .with_syncer(syncer)
            .with_start_block(self.from_block)
            .build()
            .await
            .context("build sora to sora relay")?;
        tokio::try_join!(
            beefy_relay.run(!self.send_unneeded_commitments),
            messages_relay.run()
        )?;
        Ok(())
    }
}

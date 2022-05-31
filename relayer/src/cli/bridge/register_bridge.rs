use super::*;
use crate::ethereum::make_header;
use crate::prelude::*;
use bridge_types::H160;
use clap::*;
use ethers::prelude::Middleware;
use substrate_gen::runtime;

#[derive(Args, Clone, Debug)]
pub(crate) struct Command {
    #[clap(long, short)]
    descendants_until_final: u64,
    #[clap(long)]
    eth_app: H160,
    #[clap(long)]
    migration_app: Option<H160>,
}

impl Command {
    pub(super) async fn run(&self, args: &BaseArgs) -> AnyResult<()> {
        let eth = args.get_unsigned_ethereum().await?;
        let sub = args.get_signed_substrate().await?;

        let eth_app = ethereum_gen::ETHApp::new(self.eth_app, eth.inner());
        let basic_outbound_channel = eth_app.channels(0).call().await?.1;
        let incentivized_outbound_channel = eth_app.channels(1).call().await?.1;

        let network_id = eth.get_chainid().await?.as_u32();
        let number = eth.get_block_number().await? - self.descendants_until_final;
        let block = eth.get_block(number).await?.expect("block not found");
        let header = make_header(block);
        let result = sub
            .api()
            .tx()
            .sudo()
            .sudo(
                false,
                runtime::runtime_types::framenode_runtime::Call::EthereumLightClient(
                    runtime::runtime_types::ethereum_light_client::pallet::Call::register_network {
                        header,
                        network_id,
                        initial_difficulty: Default::default(),
                    },
                ),
            )?
            .sign_and_submit_then_watch_default(&sub)
            .await?
            .wait_for_in_block()
            .await?
            .wait_for_success()
            .await?;
        info!("Result: {:?}", result.iter().collect::<Vec<_>>());
        let result = sub
            .api()
            .tx()
            .sudo()
            .sudo(false,
                runtime::runtime_types::framenode_runtime::Call::BasicInboundChannel(
                    runtime::runtime_types::basic_channel::inbound::pallet::Call::register_channel {
                        network_id,
                        channel: basic_outbound_channel
                    },
                ),
            )?
            .sign_and_submit_then_watch_default(&sub)
            .await?
            .wait_for_in_block()
            .await?
            .wait_for_success()
            .await?;
        info!("Result: {:?}", result.iter().collect::<Vec<_>>());
        let result = sub
            .api()
            .tx()
            .sudo()
            .sudo(false,
                runtime::runtime_types::framenode_runtime::Call::IncentivizedInboundChannel(
                    runtime::runtime_types::incentivized_channel::inbound::pallet::Call::register_channel {
                        network_id,
                        channel: incentivized_outbound_channel
                    },
                ),
            )?
            .sign_and_submit_then_watch_default(&sub)
            .await?
            .wait_for_in_block()
            .await?
            .wait_for_success()
            .await?;
        info!("Result: {:?}", result.iter().collect::<Vec<_>>());
        let result = sub
            .api()
            .tx()
            .sudo()
            .sudo(false, runtime::runtime_types::framenode_runtime::Call::EthApp(
                runtime::runtime_types::eth_app::pallet::Call::register_network_with_existing_asset {
                    network_id,
                    contract: self.eth_app,
                    asset_id: common::ETH,
                },
            ))?
            .sign_and_submit_then_watch_default(&sub)
            .await?
            .wait_for_in_block()
            .await?
            .wait_for_success()
            .await?;
        info!("Result: {:?}", result.iter().collect::<Vec<_>>());
        if let Some(migration_app) = self.migration_app {
            let result = sub
                .api()
                .tx()
                .sudo()
                .sudo(
                    false,
                    runtime::runtime_types::framenode_runtime::Call::MigrationApp(
                        runtime::runtime_types::migration_app::pallet::Call::register_network {
                            network_id,
                            contract: migration_app,
                        },
                    ),
                )?
                .sign_and_submit_then_watch_default(&sub)
                .await?
                .wait_for_in_block()
                .await?
                .wait_for_success()
                .await?;
            info!("Result: {:?}", result.iter().collect::<Vec<_>>());
        }
        Ok(())
    }
}

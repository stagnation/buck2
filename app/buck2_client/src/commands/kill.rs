/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

use buck2_client_ctx::client_ctx::ClientCommandContext;
use buck2_client_ctx::common::CommonDaemonCommandOptions;
use buck2_client_ctx::daemon::client::connect::BuckdConnectOptions;
use buck2_client_ctx::subscribers::recorder::try_get_invocation_recorder;

#[derive(Debug, clap::Parser)]
#[clap(about = "Kill the buck daemon")]
pub struct KillCommand {}

impl KillCommand {
    pub fn exec(
        self,
        _matches: &clap::ArgMatches,
        ctx: ClientCommandContext<'_>,
    ) -> anyhow::Result<()> {
        ctx.with_runtime(async move |ctx| {
            let _log_on_drop = try_get_invocation_recorder(
                &ctx,
                CommonDaemonCommandOptions::default_ref(),
                "kill",
                std::env::args().collect(),
            )?;

            match ctx
                .connect_buckd(BuckdConnectOptions::existing_only_no_console())
                .await
            {
                Err(_) => {
                    buck2_client_ctx::eprintln!("no buckd server running")?;
                }
                Ok(mut client) => {
                    buck2_client_ctx::eprintln!("killing buckd server")?;
                    client
                        .with_flushing()
                        .kill("`buck kill` was invoked")
                        .await?;
                }
            }
            Ok(())
        })
    }
}

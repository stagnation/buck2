/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

use anyhow::Context;
use async_trait::async_trait;
use buck2_cli_proto::CounterWithExamples;
use buck2_cli_proto::TestRequest;
use buck2_cli_proto::TestSessionOptions;
use buck2_client_ctx::client_ctx::ClientCommandContext;
use buck2_client_ctx::common::CommonBuildConfigurationOptions;
use buck2_client_ctx::common::CommonBuildOptions;
use buck2_client_ctx::common::CommonCommandOptions;
use buck2_client_ctx::common::CommonConsoleOptions;
use buck2_client_ctx::common::CommonDaemonCommandOptions;
use buck2_client_ctx::daemon::client::BuckdClientConnector;
use buck2_client_ctx::daemon::client::NoPartialResultHandler;
use buck2_client_ctx::exit_result::ExitResult;
use buck2_client_ctx::final_console::FinalConsole;
use buck2_client_ctx::stdio::eprint_line;
use buck2_client_ctx::streaming::StreamingCommand;
use buck2_client_ctx::subscribers::superconsole::test::TestCounterColumn;
use gazebo::prelude::*;
use superconsole::Line;
use superconsole::Span;

use crate::commands::build::print_build_result;

fn print_error_counter(
    console: &FinalConsole,
    counter: &CounterWithExamples,
    error_type: &str,
    symbol: &str,
) -> anyhow::Result<()> {
    if counter.count > 0 {
        console.print_error(&format!("{} {}", counter.count, error_type))?;
        for test_name in &counter.example_tests {
            console.print_error(&format!("  {} {}", symbol, test_name))?;
        }
        if counter.count > counter.max {
            console.print_error(&format!(
                "  ...and {} more not shown...",
                counter.count - counter.max
            ))?;
        }
    }
    Ok(())
}
#[derive(Debug, clap::Parser)]
#[clap(name = "test", about = "Build and test the specified targets")]
pub struct TestCommand {
    #[clap(flatten)]
    common_opts: CommonCommandOptions,

    #[clap(flatten)]
    build_opts: CommonBuildOptions,

    #[clap(
        long = "exclude",
        multiple_values = true,
        help = "Labels on targets to exclude from tests"
    )]
    exclude: Vec<String>,

    #[clap(
        long = "include",
        alias = "labels",
        help = "Labels on targets to include from tests. Prefixing with `!` means to exclude. First match wins unless overridden by `always-exclude` flag.\n\
If include patterns are present, regardless of whether exclude patterns are present, then all targets are by default excluded unless explicitly included.",
        multiple_values = true
    )]
    include: Vec<String>,

    #[clap(
        long = "always-exclude",
        alias = "always_exclude",
        help = "Whether to always exclude if the label appears in `exclude`, regardless of which appears first"
    )]
    always_exclude: bool,

    #[clap(
        long = "build-filtered",
        help = "Whether to build tests that are excluded via labels."
    )]
    build_filtered_targets: bool, // TODO(bobyf) this flag should always override the buckconfig option when we use it

    /// This option is currently on by default, but will become a proper option in future (T110004971)
    #[clap(long = "keep-going")]
    #[allow(unused)]
    keep_going: bool,

    /// This option does nothing. It is here to keep compatibility with Buck1 and ci
    #[allow(unused)] // for v1 compat
    #[clap(long = "deep")]
    deep: bool,

    // ignored. only for e2e tests. compatibility with v1.
    #[clap(long = "xml")]
    #[allow(unused)] // for v1 compat
    xml: Option<String>,

    /// Will allow tests that are compatible with RE (setup to run from the repo root and
    /// use relative paths) to run from RE.
    #[clap(long, group = "re_options", alias = "unstable-allow-tests-on-re")]
    unstable_allow_compatible_tests_on_re: bool,

    /// Will run tests to on RE even if they are missing required settings (running from the root +
    /// relative paths). Those required settings just get overridden.
    #[clap(long, group = "re_options", alias = "unstable-force-tests-on-re")]
    unstable_allow_all_tests_on_re: bool,

    #[clap(name = "TARGET_PATTERNS", help = "Patterns to test")]
    patterns: Vec<String>,

    #[clap(
        name = "TEST_EXECUTOR_ARGS",
        help = "Additional arguments passed to the test executor",
        raw = true
    )]
    test_executor_args: Vec<String>,
}

#[async_trait]
impl StreamingCommand for TestCommand {
    const COMMAND_NAME: &'static str = "test";

    async fn exec_impl(
        self,
        buckd: &mut BuckdClientConnector,
        matches: &clap::ArgMatches,
        ctx: &mut ClientCommandContext<'_>,
    ) -> ExitResult {
        let context = ctx.client_context(
            &self.common_opts.config_opts,
            matches,
            self.sanitized_argv(),
        )?;
        let response = buckd
            .with_flushing()
            .test(
                TestRequest {
                    context: Some(context),
                    target_patterns: self
                        .patterns
                        .map(|pat| buck2_data::TargetPattern { value: pat.clone() }),
                    test_executor_args: self.test_executor_args,
                    excluded_labels: self.exclude,
                    included_labels: self.include,
                    always_exclude: self.always_exclude,
                    build_filtered_targets: self.build_filtered_targets,
                    // we don't currently have a different flag for this, so just use the build one.
                    concurrency: self.build_opts.num_threads.unwrap_or(0),
                    build_opts: Some(self.build_opts.to_proto()),
                    session_options: Some(TestSessionOptions {
                        allow_re: self.unstable_allow_compatible_tests_on_re
                            || self.unstable_allow_all_tests_on_re,
                        force_use_project_relative_paths: self.unstable_allow_all_tests_on_re,
                        force_run_from_project_root: self.unstable_allow_all_tests_on_re,
                    }),
                },
                ctx.stdin()
                    .console_interaction_stream(&self.common_opts.console_opts),
                &mut NoPartialResultHandler,
            )
            .await??;

        let statuses = response
            .test_statuses
            .as_ref()
            .expect("Daemon to not return empty statuses");

        let listing_failed = statuses
            .listing_failed
            .as_ref()
            .context("Missing `listing_failed`")?;
        let passed = statuses.passed.as_ref().context("Missing `passed`")?;
        let failed = statuses.failed.as_ref().context("Missing `failed`")?;
        let fatals = statuses.fatals.as_ref().context("Missing `fatals`")?;
        let skipped = statuses.skipped.as_ref().context("Missing `skipped`")?;

        let console = self.common_opts.console_opts.final_console();
        print_build_result(&console, &response.error_messages)?;
        if !response.error_messages.is_empty() {
            console.print_error(&format!("{} BUILDS FAILED", response.error_messages.len()))?;
        }

        // TODO(nmj): Might make sense for us to expose the event ctx, and use its
        //            handle_stdout method, instead of raw buck2_client::println!s here.
        // TODO: also remove the duplicate information when the above is done.

        let mut line = Line::default();
        line.push(Span::new_unstyled_lossy("Tests finished: "));
        if listing_failed.count > 0 {
            line.push(TestCounterColumn::LISTING_FAIL.to_span_from_test_statuses(statuses)?);
            line.push(Span::new_unstyled_lossy(". "));
        }
        let columns = [
            TestCounterColumn::PASS,
            TestCounterColumn::FAIL,
            TestCounterColumn::FATAL,
            TestCounterColumn::SKIP,
        ];
        for column in columns {
            line.push(column.to_span_from_test_statuses(statuses)?);
            line.push(Span::new_unstyled_lossy(". "));
        }
        line.push(Span::new_unstyled_lossy(format!(
            "{} builds failed",
            response.error_messages.len()
        )));
        eprint_line(&line)?;

        print_error_counter(&console, &listing_failed, "LISTINGS FAILED", "⚠")?;
        print_error_counter(&console, &failed, "TESTS FAILED", "✗")?;
        print_error_counter(&console, &fatals, "TESTS FATALS", "⚠")?;
        if passed.count + failed.count + fatals.count + skipped.count == 0 {
            console.print_warning("NO TESTS RAN")?;
        }

        if let Some(exit_code) = response.exit_code {
            ExitResult::status_extended(exit_code)
        } else {
            ExitResult::failure()
        }
    }

    fn console_opts(&self) -> &CommonConsoleOptions {
        &self.common_opts.console_opts
    }

    fn event_log_opts(&self) -> &CommonDaemonCommandOptions {
        &self.common_opts.event_log_opts
    }

    fn common_opts(&self) -> &CommonBuildConfigurationOptions {
        &self.common_opts.config_opts
    }
}

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under both the MIT license found in the
# LICENSE-MIT file in the root directory of this source tree and the Apache
# License, Version 2.0 found in the LICENSE-APACHE file in the root directory
# of this source tree.

load("@prelude//rust:rust_toolchain.bzl", "RustPlatformInfo", "RustToolchainInfo")

_DEFAULT_TRIPLE = select({
    "config//os:linux": select({
        "config//cpu:arm64": "aarch64-unknown-linux-gnu",
        "config//cpu:x86_64": "x86_64-unknown-linux-gnu",
    }),
    "config//os:macos": select({
        "config//cpu:arm64": "aarch64-apple-darwin",
        "config//cpu:x86_64": "x86_64-apple-darwin",
    }),
    "config//os:windows": select({
        # FIXME: rustup's default ABI for the host on Windows is MSVC, not GNU.
        # When you do `rustup install stable` that's the one you get. It makes
        # you opt in to GNU by `rustup install stable-gnu`. We should match that
        # default when we're able; but for now buck2 doesn't work with the MSVC
        # toolchain yet.
        "config//cpu:arm64": "aarch64-pc-windows-gnu",
        "config//cpu:x86_64": "x86_64-pc-windows-gnu",
    }),
})

def _system_rust_toolchain_impl(ctx):
    return [
        DefaultInfo(),
        RustToolchainInfo(
            allow_lints = ctx.attrs.allow_lints,
            clippy_driver = "clippy-driver",
            clippy_toml = ctx.attrs.clippy_toml[DefaultInfo].default_outputs[0] if ctx.attrs.clippy_toml else None,
            compiler = "rustc",
            concat_tool = ctx.attrs.concat_tool[RunInfo],
            default_edition = ctx.attrs.default_edition,
            deny_lints = ctx.attrs.deny_lints,
            extern_html_root_url_prefix = ctx.attrs.extern_html_root_url_prefix,
            failure_filter_action = ctx.attrs.failure_filter_action[RunInfo],
            pipelined = ctx.attrs.pipelined,
            report_unused_deps = ctx.attrs.report_unused_deps,
            rustc_action = ctx.attrs.rustc_action[RunInfo],
            rustc_binary_flags = ctx.attrs.rustc_binary_flags,
            rustc_check_flags = ctx.attrs.rustc_check_flags,
            rustc_flags = ctx.attrs.rustc_flags,
            rustc_target_triple = ctx.attrs.rustc_target_triple,
            rustc_test_flags = ctx.attrs.rustc_test_flags,
            rustdoc = "rustdoc",
            rustdoc_flags = ctx.attrs.rustdoc_flags,
            rustdoc_test_with_resources = ctx.attrs.rustdoc_test_with_resources[RunInfo],
            transitive_dependency_symlinks_tool = ctx.attrs.transitive_dependency_symlinks_tool[RunInfo],
            warn_lints = ctx.attrs.warn_lints,
        ),
        RustPlatformInfo(
            name = "x86_64",
        ),
    ]

system_rust_toolchain = rule(
    impl = _system_rust_toolchain_impl,
    attrs = {
        "allow_lints": attrs.list(attrs.string(), default = []),
        "clippy_toml": attrs.option(attrs.dep(providers = [DefaultInfo]), default = None),
        "concat_tool": attrs.default_only(attrs.dep(providers = [RunInfo], default = "prelude//rust/tools:concat")),
        "default_edition": attrs.option(attrs.string(), default = None),
        "deny_lints": attrs.list(attrs.string(), default = []),
        "extern_html_root_url_prefix": attrs.option(attrs.string(), default = None),
        "failure_filter_action": attrs.default_only(attrs.dep(providers = [RunInfo], default = "prelude//rust/tools:failure_filter_action")),
        "pipelined": attrs.bool(default = False),
        "report_unused_deps": attrs.bool(default = False),
        "rustc_action": attrs.default_only(attrs.dep(providers = [RunInfo], default = "prelude//rust/tools:rustc_action")),
        "rustc_binary_flags": attrs.list(attrs.string(), default = []),
        "rustc_check_flags": attrs.list(attrs.string(), default = []),
        "rustc_flags": attrs.list(attrs.string(), default = []),
        "rustc_target_triple": attrs.string(default = _DEFAULT_TRIPLE),
        "rustc_test_flags": attrs.list(attrs.string(), default = []),
        "rustdoc_flags": attrs.list(attrs.string(), default = []),
        "rustdoc_test_with_resources": attrs.default_only(attrs.dep(providers = [RunInfo], default = "prelude//rust/tools:rustdoc_test_with_resources")),
        "transitive_dependency_symlinks_tool": attrs.default_only(attrs.dep(providers = [RunInfo], default = "prelude//rust/tools:transitive_dependency_symlinks")),
        "warn_lints": attrs.list(attrs.string(), default = []),
    },
    is_toolchain_rule = True,
)

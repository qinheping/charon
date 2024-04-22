//! Charon is a tool which compiles Rust projects (by querying their MIR) to
//! an easy-to-use format called LLBC (Low-Level Borrow Calculus), which is
//! basically MIR cleaned up and where the control-flow has been reconstructed.
//! This AST is serialized as JSON files.
//!
//!
//! We structured the project by following the approach used by [rust-clippy](https://github.com/rust-lang/rust-clippy).
//! In order to query the results of the Rustc compiler, we need to implement
//! a driver which calls Rustc while giving it some callbacks.
//! The problem is that finding the proper arguments to call Rustc with can
//! be difficult. For instance, provided the project we want to analyse with
//! Charon has already been built (and in particular its dependencies), it is
//! very difficult to provide the proper `--extern` arguments, indicating
//! where to find the compiled external dependencies. For instance, even if
//! we look in the `target` folder, the compiled depedencies are decorated
//! with a hash, and we don't know where this hash comes from.
//! Computing those arguments is, however, Cargo's responsability. As a
//! consequence, we follow Clippy's approach by piggy-backing on Cargo.  We
//! call Cargo as if we were building the project, but set up the environment
//! variable `RUSTC_WORKSPACE_WRAPPER` so that Cargo calls `charon-driver`
//! instead of Rustc upon building the target project. More specifically:
//! Cargo will call Rustc to build the dependencies, *then* will call
//! charon-driver with the arguments it would have given to Rustc to build
//! the target project.
//! Upon being called, charon-driver (see `charon_driver`) will simply call
//! Rustc with the arguments it was provided (and a few minor modifications).
//! Also, in order to transmit options from cargo-charon (this file)
//! to charon-driver (`charon-driver`), we serialize those options and store
//! them in a specific environment variable, so that charon-driver can
//! deserialize them later and use them to guide the extraction in the
//! callbacks.

#![cfg_attr(feature = "deny-warnings", deny(warnings))]

// Don't link with the `charon_lib` crate so that the `charon` binary doesn't have to dynamically
// link to `librustc_driver.so` etc.
mod cli_options;
mod logger;

use clap::Parser;
use cli_options::{CliOpts, CHARON_ARGS};
use serde::Deserialize;
use std::env;
use std::path::PathBuf;
use std::process::Command;

// Store the toolchain details directly in the binary.
static PINNED_TOOLCHAIN: &str = include_str!("../rust-toolchain");

/// This struct is used to deserialize the "rust-toolchain" file.
#[derive(Deserialize)]
struct ToolchainFile {
    toolchain: Toolchain,
}

#[derive(Deserialize)]
struct Toolchain {
    channel: String,
    #[allow(dead_code)]
    // FIXME: ensure the right components are installed.
    components: Vec<String>,
}

fn get_pinned_toolchain() -> Toolchain {
    let file_contents: ToolchainFile = toml::from_str(PINNED_TOOLCHAIN).unwrap();
    file_contents.toolchain
}

// Nix overrides this with a path to the correct cargo.
static NIX_CARGO_OVERRIDE: Option<&str> = None;

pub fn main() {
    // Initialize the logger
    logger::initialize_logger();

    // Parse the command-line
    let options = CliOpts::parse();
    trace!("Arguments: {:?}", std::env::args());

    // Check that the options are meaningful
    assert!(
        !options.lib || options.bin.is_none(),
        "Can't use --lib and --bin at the same time"
    );

    assert!(
        !options.mir_promoted || !options.mir_optimized,
        "Can't use --mir_promoted and --mir_optimized at the same time"
    );

    assert!(
        !options.abort_on_error || !options.errors_as_warnings,
        "Can't use --abort-on-error and --errors-as-warnings at the same time"
    );

    if let Err(code) = process(&options) {
        std::process::exit(code);
    }
}

fn driver_path() -> PathBuf {
    let mut path = env::current_exe()
        .expect("current executable path invalid")
        .with_file_name("charon-driver");

    if cfg!(windows) {
        path.set_extension("exe");
    }

    path
}

fn process(options: &CliOpts) -> Result<(), i32> {
    let toolchain = get_pinned_toolchain();
    // FIXME: when using rustup, ensure the toolchain has the right components installed.
    let use_rustup = which::which("rustup").is_ok();
    let driver_path = driver_path();
    // For developping on nix, the flake dev env sets `NIX_CHARON_CARGO_PATH` to know where the right cargo is.
    let cargo_path_override = NIX_CARGO_OVERRIDE
        .map(|s| s.to_string())
        .or_else(|| env::var("NIX_CHARON_CARGO_PATH").ok());

    if !use_rustup && cargo_path_override.is_none() {
        panic!(
            "Can't find `rustup`; please install it with your system package manager \
            or from https://rustup.rs . \
            If you are using nix, make sure to be in the flake-defined environment \
            using `nix develop`.",
        )
    }

    let mut cmd;
    if let Some(cargo_path) = cargo_path_override {
        trace!("Using overriden cargo path: {cargo_path}");
        cmd = Command::new(cargo_path);
    } else {
        assert!(use_rustup); // Otherwise we panicked earlier.
        trace!("Using rustup-provided `cargo`.");
        cmd = Command::new("rustup");
        cmd.arg("run");
        cmd.arg(toolchain.channel);
        cmd.arg("cargo");
    }

    cmd.env("RUSTC_WORKSPACE_WRAPPER", driver_path);
    cmd.env(CHARON_ARGS, serde_json::to_string(&options).unwrap());

    // Compute the arguments of the command to call cargo
    //let cargo_subcommand = "build";
    let cargo_subcommand = "rustc";
    cmd.arg(cargo_subcommand);

    if options.lib {
        cmd.arg("--lib");
    }

    if options.bin.is_some() {
        cmd.arg("--bin");
        cmd.arg(options.bin.as_ref().unwrap().clone());
    }

    // Always compile in release mode: in effect, we want to analyze the released
    // code. Also, rustc inserts a lot of dynamic checks in debug mode, that we
    // have to clean.
    cmd.arg("--release");

    let exit_status = cmd
        .spawn()
        .expect("could not run cargo")
        .wait()
        .expect("failed to wait for cargo?");

    if exit_status.success() {
        Ok(())
    } else {
        Err(exit_status.code().unwrap_or(-1))
    }
}

# Omni Desktop Host

## Overview

Omni is a platform for building fully modular applications based around [WebAssembly](https://webassembly.org/) plugins. These plugins are meant to be very simplistic building blocks that can be easily switched out, built by either the app developers or 3rd parties. These may link together in a tree-like structure defining their own interfaces. It offers fine-grained permission management only allowing each plugin to do the bare minimum it needs to function.

**Note:** This project is in early alpha. Expect breaking changes, incomplete features, and potential instability.

## Quick Start

### Using Nix (Recommended)

If you have Nix installed, please use the provided `flake.nix` to set up everything you need:

```bash
nix develop
```

### Manual Setup

Running this project will require installing the following:

- [Rust toolchain](https://www.rust-lang.org/learn/get-started/)
- [Cap'n Proto](https://capnproto.org/install.html).

## Technical Details

### Design Rationale

- **WebAssembly**: Easy language-agnostic low-overhead sandboxing.
- **WIT**: Standardized [IDL](https://en.wikipedia.org/wiki/Interface_description_language) designed for the WebAssembly Component Model.
- **Cap'n Proto**: Provides efficient, zero-copy serialization ideal for network transmission and storing of plugin manifests.

### Plugin System

Plugins connect via abstract interfaces defined in the [WIT](https://github.com/WebAssembly/component-model/blob/main/design/mvp/WIT.md) format. These are not tied to any specific plugin, instead, each plugin defines a 'plug' pointing to an interface it implements, and optionally, a list of 'sockets', pointing to interfaces it expects to call into.

## Running Tests

For maintainability purposes, tests use TOML files for manifest declarations and WAT files for the plugin code where viable. These are not supported by default but are instead locked behind the `test` feature. To run tests, use the command bellow:

```bash
cargo test --features test
```

or when running specific suites use:

```bash
cargo test --features test --test <suite>
```
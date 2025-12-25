# Omni Desktop Host

## Overview

Omni is a platform for building fully modular applications based around [WebAssembly](https://webassembly.org/) plugins. These plugins are meant to be very simplistic building blocks that can be easily switched out, built by either the app developers or 3rd parties. These may link together in a tree-like structure defining their own interfaces. It offers fine-grained permission management only allowing each plugin to do the bare minimum it needs to function.

**Note:** This project is in early alpha. Expect breaking changes, incomplete features, and potential instability.

## Quick Start

Requires [Rust](https://www.rust-lang.org/) and [Cap'n Proto](https://capnproto.org/).

### Using Nix (Recommended)

If you have Nix installed, please use the provided `shell.nix` to set up everything you need:

```bash
nix-shell
```

### Manual Setup

Just install Cargo an Cap'n Proto I guess, you can figure it out.

## Features

- Modular plugin architecture for composable apps.
- [WebAssembly](https://webassembly.org/) isolation for security.
- Tree-like plugin dependencies.
- Fine-grained permission management.

## Plugin System

Plugins connect via abstract interfaces defined in the [WIT](https://github.com/WebAssembly/component-model/blob/main/design/mvp/WIT.md) format. These are not tied to any specific plugin, instead, each plugin defines a 'plug' pointing to an interface it implements, and optionally, a list of 'sockets', pointing to interfaces it expects to call into.

## Design Rationale

- **WebAssembly**: Easy language-agnostic low-overhead sandboxing.
- **WIT**: Standardized [IDL](https://en.wikipedia.org/wiki/Interface_description_language) designed for the WebAssembly Component Model.
- **Cap'n Proto**: Provides efficient, zero-copy serialization ideal for network transmission and storing of plugin manifests.
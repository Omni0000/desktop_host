# Agents Instructions

Follow these instructions strictly when working on the Omni Desktop Host codebase to maintain consistency, quality, and adherence to functional programming principles.

## General Guidelines
- Read the README before starting any work.
- Apply functional programming style: Avoid explicit for-loops; use iterators instead. Avoid `mut` unless it incurs a performance penalty; prefer mut abstractions from `@src/utils/`.

## Project Structure
- **src/**: Main source code.
  - lib.rs/main.rs: Entry points for library/binary.
  - initialisation/: Handles plugin discovery, loading, and tree construction.
    - discovery/: Discovers plugins and interfaces from filesystem, parses manifests and WIT files.
    - loading/: Loads plugins into wasmtime Engine, creates PluginTree with contexts and sockets.
    - types/: Defines IDs and types for plugins, interfaces, and cardinalities.
  - utils/: Functional utilities like MapScanTrait, Merge, and warning helpers (temporary until proper logging).
  - capnp.rs/exports.rs: Serialization and WebAssembly linker setup.
- **tests/**: Feature-gated integration tests with TOML/WAT fixtures.
- **appdata/**: Local runtime interfaces (WIT files) and plugins (WASM files), gitignored.
- **capnp/**: Schema definitions for manifests.

## Iterator and Collection Handling
- Prefer `.map_scan()` from `MapScanTrait` for stateful iterations over manual loops or `fold`.
- Use `.merge()` and `.merge_all()` from `Merge` for immutable vector extensions instead of `push` on mutable references.
- Leverage `pipe_trait` for fluent method chaining to promote composition over intermediate variables.

## Error Handling
- Prohibit `unwrap()` and `expect()`—use graceful error handling everywhere possible, propagating with `?` and structured types from `thiserror`.
- Do not use `anyhow` (despite its presence in dependencies due to `wit_parser`).

## Testing and Features
- Tests are in `./tests` and require `--features test` to run.
- They are not comprehensive and should not be relied on for ensuring correctness; they're a quick way to validate nothing is broken too badly.
- Run tests using TOML/WAT fixtures as per README with `cargo test --features test` every time a task is run to completion (there are no todos except for currently unreachable branches).
- Avoid relying on `src/` unit tests.

## Immutability Rule of Thumb
- Use `mut` only for performance-critical mutable borrows or when abstractions (e.g., `RwLock` for shared state) require it.
- Default to immutable bindings and ownership moves.

## Code Style
- Prefer slightly longer lines even if above the standard limit for very long functions/files, as it improves readability.
# Agents Instructions

Follow these instructions strictly when working on the Omni Desktop Host codebase to maintain consistency, quality, and adherence to functional programming principles.

## General Guidelines
- Read the README before starting any work.
- Apply functional programming style: Avoid explicit for-loops; use iterators instead. Avoid `mut` unless it incurs a performance penalty; prefer mut abstractions from `@src/utils/`.
- Important: Never create commits. After completing a task (including adding and running tests), inform the user that the work is ready for review and manual commit.

## Development Environment Setup

### Prerequisites
- Rust toolchain (2021 edition)
- Cap'n Proto for serialization
- Optional: Nix for reproducible development environment

### Using Nix (Recommended)
```bash
nix develop
```
This provides all necessary dependencies including Rust, Cap'n Proto, and development tools.

## Testing Commands

### Run All Tests
```bash
cargo test --features test -- --nocapture
```
Note: Tests are feature-gated behind the `test` flag and use TOML/WAT fixtures.

### Run Specific Test Suite
```bash
cargo test --features test --test <suite_name> -- --nocapture
```
Available test suites: `dispatching_tests`, `resource_tests`, `error_handling_tests`, `cardinality_tests`

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
- Prohibit `unwrap()` and `expect()` - use graceful error handling everywhere possible, propagating with `?` and structured types from `thiserror`.
- Do not use `anyhow` (despite its presence in dependencies due to `wit_parser`).
- Define custom error types using `thiserror` for structured error reporting. These should be specific to the operation you're making. If don't use global error types for the whole module. If a function can't produce some variant of the error type, then the error type should not be reused for that function. Either use a separate error or nesting if it makes sense.

## Testing and Features
- Tests are in `./tests` and require `--features test` to run.
- They are not comprehensive and should not be relied on for ensuring correctness; they're a quick way to validate nothing is broken too badly.
- Run tests using TOML/WAT fixtures as per README with `cargo test --features test` every time a task is run to completion (there are no todos except for currently unreachable branches).
- After implementing a feature, run existing tests to ensure no regressions. If tests fail or further changes are requested, iterate on the implementation. Once satisfied with the outcome, suggest test cases to add, implement them, and re-run tests. Only consider the task finished once all tests (existing and new) run successfully. Finally, offer to add documentation for the implemented functionality.

## Immutability Rule of Thumb
- Use `mut` only for performance-critical mutable borrows or when abstractions (e.g., `RwLock` for shared state) require it.
- Default to immutable bindings and ownership moves.

## Code Style Guidelines

### Imports and Modules
- Group imports by external crates, then internal modules ordered by source crate, then super then submodules
- Avoid wildcard imports even when importing many related items except for preludes 
- Keep module declarations at the top of files
- Do not use mod.rs files; put module files one directory higher with the same name as the module

### Naming Conventions
- do not ever use single letter names
- Use `snake_case` for variables, functions, and modules
- Use `PascalCase` for types, traits, and enum variants
- Use `SCREAMING_SNAKE_CASE` for constants
- Prefix error types with the operation: `PluginManifestReadError`, `DiscoveryFailure`

### Function Signatures
- Prefer descriptive parameter names over abbreviations
- Use `&` for immutable references, `&mut` only when necessary
- Return `Result<T, E>` for fallible operations with custom error types
- Use generics sparingly but effectively for reusable abstractions

### Type Definitions
- Define custom error enums using `thiserror` with descriptive error messages
- Use associated types in traits for better ergonomics
- Prefer struct composition over inheritance patterns

### Formatting
- Prefer slightly longer lines even if above the standard limit for very long functions/files, as it improves readability overall.
- Use consistent spacing around operators and after commas

### Unsafe Code
- Minimize unsafe code usage; only use when instructed to
- Document safety invariants with comments
- Prefer safe abstractions over direct unsafe operations

## Dependencies
- Keep dependencies minimal and well-maintained
- Review security advisories regularly
- Use workspace dependencies where appropriate

{
    inputs = {
        nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable" ;
        flake-utils.url = "github:numtide/flake-utils" ;
        rust-overlay = {
            url = "github:oxalica/rust-overlay" ;
            inputs = { nixpkgs.follows = "nixpkgs" ; };
        };
    };

	outputs = { nixpkgs, flake-utils, rust-overlay, ... }:
		flake-utils.lib.eachDefaultSystem ( system:
			let
				overlays = [( import rust-overlay )];
				pkgs = import nixpkgs { inherit system overlays ; };
			in {
				devShells.default = pkgs.mkShell {
					buildInputs = with pkgs; [
						rust-bin.stable.latest.default
						lld
						pkg-config
						clang
						capnproto
					];
				};
			}
		);
}

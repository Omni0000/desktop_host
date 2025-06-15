let
  pkgs = import <nixpkgs> {};
in
  pkgs.mkShell {
    buildInputs = with pkgs; [
      rustc
      cargo
      pkg-config
      clang # Or gcc
    ];
  }

let
  pkgs = import <nixpkgs> {};
in
  pkgs.mkShell {
    buildInputs = with pkgs; [
      rustc
      cargo
      lld
      pkg-config
      clang
      capnproto
    ];
  }

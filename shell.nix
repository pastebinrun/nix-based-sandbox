let
  moz_overlay = import (builtins.fetchTarball "https://github.com/mozilla/nixpkgs-mozilla/archive/master.tar.gz");
  pkgs = import <nixpkgs> { overlays = [ moz_overlay ]; };
in
with pkgs;
mkShell {
  packages = [
    bubblewrap

    dotnet-sdk_5
    ghc
    perl
    php
    rakudo
    sqlite

    # C/C++
    clang
    gcc

    # Go
    go
    gotools

    # Node.js
    nodejs
    nodePackages.typescript

    # Python
    black
    python
    
    # Rust
    rustChannels.stable.rust
  ];
  JAVA8 = openjdk8;
  JAVA11 = openjdk11;
  RUST_BETA = rustChannels.beta.rust;
  RUST_NIGHTLY = rustChannels.nightly.rust;
}

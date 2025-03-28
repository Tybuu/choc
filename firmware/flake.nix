{
  description = "A devShell example";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
  };
  #
  outputs = {
    self,
    nixpkgs,
    rust-overlay,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        overlays = [(import rust-overlay)];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        services = import flake-utils.services;
      in {
        devShells.default = with pkgs;
          mkShell {
            buildInputs = [
              openssl
              pkg-config
              cargo-binutils
              cargo-make
              probe-rs
              (rust-bin.stable.latest.default.override
                {
                  extensions = ["rust-src" "rust-analyzer" "llvm-tools"];
                  targets = ["thumbv7em-none-eabihf"];
                })
            ];
          };

        services.udev.extraRules = builtins.readFile "./69-probe-rs.rules";
      }
    );
}

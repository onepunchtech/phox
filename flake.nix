{
  description = "Phox — a dependently typed expression language";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };

        rust = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        rustPlatform = pkgs.makeRustPlatform {
          cargo = rust;
          rustc = rust;
        };

        phox-bin = rustPlatform.buildRustPackage {
          pname = "phox";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          doCheck = false;
        };

        tree-sitter-phox = pkgs.tree-sitter.buildGrammar {
          language = "phox";
          version = "0.1.0";
          src = ./tree-sitter-phox;
          generate = true;
        };

        phox-nvim = pkgs.vimUtils.buildVimPlugin {
          pname = "phox-nvim";
          version = "0.1.0";
          src = ./editor/nvim;
        };

        phox-vim = pkgs.writeShellScriptBin "phox-vim" ''
          exec nvim --cmd "set rtp^=${tree-sitter-phox},$PWD/editor/nvim" "$@"
        '';
      in {
        devShells.default = pkgs.mkShell {
          buildInputs = [
            rust
            pkgs.cargo-watch
            pkgs.tree-sitter
            pkgs.nodejs
            phox-vim
          ];
          PHOX_PARSER = "${tree-sitter-phox}";
          shellHook = ''
            export PHOX_NVIM="$PWD/editor/nvim"
          '';
        };

        packages = {
          phox = phox-bin;
          inherit tree-sitter-phox phox-nvim;
          default = phox-bin;
        };
      }
    );
}

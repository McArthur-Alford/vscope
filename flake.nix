{
  description = "Build a cargo project without extra checks";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    crane = {
      url = "github:ipetkov/crane";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        lib = nixpkgs.lib;

        llvm = pkgs.llvmPackages_latest;

        craneLib = crane.mkLib pkgs;

        commonArgs = rec {
          src = craneLib.cleanCargoSource ./.;
          strictDeps = true;

          nativeBuildInputs =
            (with pkgs; [
              # builder
              gnumake
              bear
              # debugger
              # fix headers not found
              clang-tools
              # LSP and compiler
              # Zig
              zig
              zls
              # Just!
              just
              # Random rust dependency dependencies
              pkg-config # For tokio
              fontconfig # For plotters
              hdf5-cpp
              hdf5
              highfive
              linuxKernel.packages.linux_6_10.perf
              flamegraph
            ])
            ++ (with llvm; [
              lldb
              clang
            ]);
          buildInputs =
            [
              # stdlib for cpp
              llvm.libcxx
            ]
            ++ (with pkgs; [
              hdf5-cpp
              hdf5
              highfive
            ]);
          CPATH = lib.makeSearchPathOutput "dev" "include" buildInputs;
        };

        my-crate = craneLib.buildPackage (
          commonArgs // { cargoArtifacts = craneLib.buildDepsOnly commonArgs; }
        );
      in
      {
        checks = {
          # inherit my-crate;
        };

        packages.default = my-crate;

        apps.default = flake-utils.lib.mkApp { drv = my-crate; };

        devShells.default = craneLib.devShell {
          name = "C";
          checks = self.checks.${system};

          # Additional dev-shell environment variables can be set directly
          # MY_CUSTOM_DEVELOPMENT_VAR = "something else";

          packages = [ pkgs.rust-analyzer ] ++ commonArgs.nativeBuildInputs;

          buildInputs = with pkgs; [ ] ++ commonArgs.buildInputs ++ commonArgs.nativeBuildInputs;

          CPATH = commonArgs.CPATH;
          CSPICE_DIR = "/home/mcarthur/cspice/cspice";
          LIBCLANG_PATH = "${llvm.libclang.lib}/lib";
        };
      }
    );
}

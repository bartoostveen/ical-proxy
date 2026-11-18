{
  description = "Ical proxy (made with messy Google Calendars in mind)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    crane.url = "github:ipetkov/crane";
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];

      imports = [
        inputs.treefmt-nix.flakeModule
      ];

      flake = {
        nixosModules.default = ./module.nix;
      };

      perSystem =
        {
          self',
          system,
          pkgs,
          src,
          craneLib,
          cargoArtifacts,
          lib,
          ...
        }:

        let
          nativeBuildInputs = [
            pkgs.pkg-config
          ];

          buildInputs = [ ];

          deps = nativeBuildInputs ++ buildInputs;

          commonArgs = {
            pname = "ical-proxy";
            version = "unstable";

            __structuredAttrs = true;
            strictDeps = true;

            inherit
              src
              nativeBuildInputs
              buildInputs
              ;

            meta.mainProgram = "ical-proxy";
          };

          args = commonArgs // {
            inherit cargoArtifacts;

            env.RUSTDOCFLAGS = "--deny warnings";

            cargoClippyExtraArgs = "--all-targets --all-features -- --deny warnings";
          };

          inherit (lib.fileset) toSource unions;
        in
        {
          _module.args = {
            craneLib = (inputs.crane.mkLib pkgs).overrideToolchain (
              pkgs: pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml
            );

            src = toSource {
              root = ./.;
              fileset = unions [
                ./Cargo.toml
                ./Cargo.lock
                (craneLib.fileset.commonCargoSources ./.)
              ];
            };

            cargoArtifacts = craneLib.buildDepsOnly commonArgs;

            pkgs = import inputs.nixpkgs {
              inherit system;
              overlays = [
                (import inputs.rust-overlay)
              ];
            };
          };

          treefmt = {
            programs.nixfmt.enable = true;
            programs.statix.enable = true;
            programs.deadnix.enable = true;
            programs.keep-sorted.enable = true;
            programs.rustfmt.enable = true;
            programs.taplo.enable = true;
            programs.shellcheck.enable = true;
            programs.prettier.enable = true;
            settings.formatter.prettier.excludes = [ "renovate.json" ];
            programs.yamlfmt.enable = true;
            settings.formatter.yamlfmt.excludes = [ ".forgejo/*" ];
          };

          packages = {
            default = craneLib.buildPackage args;
            ical-proxy = self'.packages.default;
          };

          checks = {
            inherit (self'.packages)
              ical-proxy
              ;

            docs = craneLib.cargoDoc args;
            format = craneLib.cargoFmt args;
            nextest = craneLib.cargoNextest (
              commonArgs
              // {
                inherit cargoArtifacts;
                partitions = 1;
                partitionType = "count";
                cargoNextestPartitionsExtraArgs = "--no-tests=pass";
              }
            );
            cargoFormat = craneLib.taploFmt (
              commonArgs
              // {
                src = pkgs.lib.sources.sourceFilesBySuffices src [ ".toml" ];
              }
            );
            clippy = craneLib.cargoClippy args;
          };

          devShells.default = craneLib.devShell {
            packages = deps ++ [
              pkgs.cargo-edit
              pkgs.cargo-nextest
            ];
          };
        };
    };
}

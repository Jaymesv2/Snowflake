{
  description = "Snowflake";
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    devshell.url = "github:numtide/devshell/master";
  };
  outputs = { self, nixpkgs, flake-utils, rust-overlay, devshell, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) devshell.overlay ];
        pkgs = import nixpkgs { inherit system overlays; };
        rustVersion = pkgs.rust-bin.stable.latest.default;

        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustVersion;
          rustc = rustVersion;
        };

        rustBuild = rustPlatform.buildRustPackage {
          pname = "snowflake"; # make this what ever your cargo.toml package.name is
          version = "0.1.0";
          src = ./.; # the folder with the cargo.toml
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.protobuf ]; # just for the host building the package
          buildInputs = [ /*pkgs.protoc*/ ]; # packages needed by the consumer
        };

        dockerImage = pkgs.dockerTools.buildImage {
          name = "snowflake";
          config = { Cmd = [ "${rustBuild}/bin/${rustBuild.pname}" ]; };
        };

      in
      {
        packages = {
          rustPackage = rustBuild;
          docker = dockerImage;
        };
        defaultPackage = rustBuild;
        /*devShell = pkgs.mkShell {
          buildInputs =
            [  pkgs.protobuf ];
        };*/
        devShell = import ./devshell.nix { inherit pkgs rustVersion; };
        formatter = pkgs.nixpkgs-fmt;
        checks = { };
      });
}

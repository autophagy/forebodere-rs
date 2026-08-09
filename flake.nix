{
  description = "Forebodere - A Discord quote bot";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    naersk.url = "github:nix-community/naersk/master";
    naersk.inputs.nixpkgs.follows = "nixpkgs";
    utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, utils, naersk }:
    utils.lib.eachDefaultSystem
      (system:
        let
          pkgs = import nixpkgs { inherit system; };
          naersk-lib = pkgs.callPackage naersk { };
        in
        rec {
          packages = {
            forebodere = naersk-lib.buildPackage {
              root = ./.;
              doCheck = true;
            };
            default = packages.forebodere;
          };

          devShell = with pkgs; mkShell {
            buildInputs = [ cargo rustc rustfmt rustPackages.clippy ];
            RUST_SRC_PATH = rustPlatform.rustLibSrc;
          };

          formatter = pkgs.nixpkgs-fmt;
        }) // {
      overlays = {
        forebodere = final: prev: { inherit (self.packages.${final.system}) forebodere; };
        default = self.overlays.forebodere;
      };

      nixosModules = {
        forebodere = { pkgs, ... }: {
          nixpkgs.overlays = [ self.overlays.default ];
          imports = [ ./module.nix ];
        };
        default = self.nixosModules.forebodere;
      };
    };
}

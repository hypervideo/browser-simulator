{ lib
, stdenv
, rustPlatform
, pkg-config
, libiconv
, openssl
, clang
, ffmpeg-headless
, llvmPackages
}:

let
  # Common configuration
  version = "0.3.1";
  src = ../.;
  cargoLock = {
    lockFile = ../Cargo.lock;
    outputHashes = {
      "chromiumoxide-0.7.0" = "sha256-FTv87IOcBATV+OFw3rMDrZTX1LN/ph5K+qwdqE4UYCc=";
    };
  };

  # Helper function to build simulator packages
  mkSimulatorPackage = { pname, description, buildInputs ? [ ], cargoBuildFlags ? [ ], env ? { } }:
    rustPlatform.buildRustPackage ({
      inherit pname version src cargoLock cargoBuildFlags;

      nativeBuildInputs = [ pkg-config ];

      buildInputs = (
        buildInputs ++ lib.optionals stdenv.isDarwin [ libiconv ]
      );

      meta = {
        inherit description;
        homepage = "https://github.com/hypervideo/browser-simulator";
      };
    } // env);
in
rec {
  hyper-client-simulator = mkSimulatorPackage {
    pname = "hyper-client-simulator";
    description = "Hyper browser client simulator";
    buildInputs = [ openssl clang ffmpeg-headless ];
    env.LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
  };

  client-simulator = hyper-client-simulator;
  default = hyper-client-simulator;
}

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
  version = "0.1.0";
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
        homepage = "https://github.com/hypervideo/hyper.video";
      };
    } // env);
in
rec {
  client-simulator = mkSimulatorPackage {
    pname = "client-simulator";
    description = "Hyper browser client simulator";
    buildInputs = [ openssl clang ffmpeg-headless ];
    env.LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
  };

  client-simulator-http = mkSimulatorPackage {
    pname = "client-simulator-http";
    description = "Hyper browser client simulator HTTP server";
    buildInputs = [ openssl clang ];
    cargoBuildFlags = [ "--package" "client-simulator-http" ];
    env.LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
  };

  client-simulator-orchestrator = mkSimulatorPackage {
    pname = "client-simulator-orchestrator";
    description = "Hyper browser client simulator orchestrator";
    buildInputs = [ openssl clang ];
    cargoBuildFlags = [ "--package" "client-simulator-orchestrator" ];
    env.LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";
  };

  client-simulator-stats-gatherer = mkSimulatorPackage {
    pname = "client-simulator-stats-gatherer";
    description = "Hyper browser client simulator stats gatherer";
    buildInputs = [ openssl ];
    cargoBuildFlags = [ "--package" "client-simulator-stats-gatherer" ];
  };

  default = client-simulator;
}

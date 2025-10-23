{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let pkgs = import nixpkgs { inherit system; };
      in
      {
        packages = {
          client-simulator = pkgs.rustPlatform.buildRustPackage {
            pname = "client-simulator";
            version = "0.1.0";
            src = ./.;

            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "chromiumoxide-0.7.0" = "sha256-FTv87IOcBATV+OFw3rMDrZTX1LN/ph5K+qwdqE4UYCc=";
              };
            };

            nativeBuildInputs = with pkgs; [
              pkg-config
            ];

            buildInputs = with pkgs; [
              openssl
              clang
              ffmpeg-headless
            ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];

            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

            meta = {
              description = "Hyper browser client simulator";
              homepage = "https://github.com/hypervideo/hyper.video";
            };
          };

          client-simulator-http = pkgs.rustPlatform.buildRustPackage {
            pname = "client-simulator-http";
            version = "0.1.0";
            src = ./.;

            cargoBuildFlags = [ "--package" "client-simulator-http" ];

            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "chromiumoxide-0.7.0" = "sha256-FTv87IOcBATV+OFw3rMDrZTX1LN/ph5K+qwdqE4UYCc=";
              };
            };

            nativeBuildInputs = with pkgs; [
              pkg-config
            ];

            buildInputs = with pkgs; [
              openssl
              clang
            ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];

            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

            meta = {
              description = "Hyper browser client simulator HTTP server";
              homepage = "https://github.com/hypervideo/hyper.video";
            };
          };

          client-simulator-orchestrator = pkgs.rustPlatform.buildRustPackage {
            pname = "client-simulator-orchestrator";
            version = "0.1.0";
            src = ./.;

            cargoBuildFlags = [ "--package" "client-simulator-orchestrator" ];

            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "chromiumoxide-0.7.0" = "sha256-FTv87IOcBATV+OFw3rMDrZTX1LN/ph5K+qwdqE4UYCc=";
              };
            };

            nativeBuildInputs = with pkgs; [
              pkg-config
            ];

            buildInputs = with pkgs; [
              openssl
              clang
            ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];

            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";

            meta = {
              description = "Hyper browser client simulator orchestrator";
              homepage = "https://github.com/hypervideo/hyper.video";
            };
          };

          client-simulator-stats-gatherer = pkgs.rustPlatform.buildRustPackage {
            pname = "client-simulator-stats-gatherer";
            version = "0.1.0";
            src = ./.;

            cargoBuildFlags = [ "--package" "client-simulator-stats-gatherer" ];

            cargoLock = {
              lockFile = ./Cargo.lock;
              outputHashes = {
                "chromiumoxide-0.7.0" = "sha256-FTv87IOcBATV+OFw3rMDrZTX1LN/ph5K+qwdqE4UYCc=";
              };
            };

            nativeBuildInputs = with pkgs; [
              pkg-config
            ];

            buildInputs = with pkgs; [
              openssl
            ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];

            meta = {
              description = "Hyper browser client simulator stats gatherer";
              homepage = "https://github.com/hypervideo/hyper.video";
            };
          };

          default = self.packages.${system}.client-simulator;
        };

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            rustc
            cargo
            clippy
            pkg-config
          ];

          buildInputs = with pkgs; [
            openssl
            clang
            ffmpeg-headless
          ] ++ (if pkgs.stdenv.isDarwin then [ libiconv ] else [ ]);

          packages = with pkgs; [
            rust-analyzer
            (rustfmt.override { asNightly = true; })
            cargo-nextest
          ]
          ++ lib.optionals pkgs.stdenv.isDarwin [ google-chrome ]
          ++ lib.optionals (!pkgs.stdenv.isDarwin) [ chromium ];

          RUST_BACKTRACE = "1";
          RUST_LOG = "debug";
          LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
        };
      }
    );
}

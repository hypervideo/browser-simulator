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

{inputs, ...}: {
  perSystem = {
    # keep-sorted start
    pkgs,
    system,
    # keep-sorted end
    ...
  }: let
    fenix = inputs.fenix.packages.${system};

    rustToolchain = fenix.combine [
      (fenix.latest.withComponents [
        # keep-sorted start
        "cargo"
        "clippy"
        "rust-src"
        "rustc"
        "rustfmt"
        # keep-sorted end
      ])
    ];

    cargoCrap = pkgs.rustPlatform.buildRustPackage rec {
      pname = "cargo-crap";
      version = "0.5.0";

      src = pkgs.fetchCrate {
        inherit pname version;
        hash = "sha256-5RhRFUh1w5/yItkmc3Vk1B6oyrmzKKl6EEZ3v0aBLwk=";
      };

      cargoHash = "sha256-vPdzZIeXsjICz5icPrr2LQ4GrcMSZe2nRa65iyzLH7Q=";

      # The published crate omits workspace fixtures required by its tests.
      doCheck = false;
    };
  in {
    devShells.default = pkgs.mkShell {
      packages = with pkgs; [
        rustToolchain
        pkgsCross.avr.buildPackages.gcc
        cargoCrap

        # keep-sorted start
        avrdude
        just
        ravedude
        # keep-sorted end

        # keep-sorted start
        cargo-audit
        cargo-mutants
        cargo-nextest
        # keep-sorted end
      ];
    };
  };
}

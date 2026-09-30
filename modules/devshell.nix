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
  in {
    devShells.default = pkgs.mkShell {
      packages = with pkgs; [
        rustToolchain
        pkgsCross.avr.buildPackages.gcc

        # keep-sorted start
        avrdude
        just
        ravedude
        # keep-sorted end
      ];
    };
  };
}

{inputs, ...}: {
  imports = [inputs.treefmt-nix.flakeModule];

  perSystem = _: {
    treefmt = {
      settings.global.excludes = [
        # keep-sorted start
        ".direnv/**"
        ".envrc"
        "target/**"
        # keep-sorted end
      ];

      programs = {
        # keep-sorted start
        alejandra.enable = true;
        deadnix.enable = true;
        nixf-diagnose.enable = true;
        statix.enable = true;
        # keep-sorted end

        # keep-sorted start
        keep-sorted.enable = true;
        rustfmt.enable = true;
        taplo.enable = true;
        # keep-sorted end
      };
    };
  };
}

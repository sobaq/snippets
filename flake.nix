{
  inputs.nixpkgs.url = "nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
  let pkgs = nixpkgs.legacyPackages.x86_64-linux;
      deps = with pkgs; with xorg; [
        sqlite
        libX11 libXcursor libxcb libXi libxkbcommon
        # glow
        libGL
      ];
  in {
    devShells.x86_64-linux.default = pkgs.mkShell {
      buildInputs = deps;
      LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath deps;
    };
  };
}
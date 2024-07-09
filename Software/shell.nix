{ pkgs ? import <nixpkgs> {} }:
pkgs.mkShell {
    nativeBuildInputs = with pkgs.buildPackages; [ 
        cargo
        rustc
        llvm
        clang
        pkg-config
        systemd
        xorg.libX11
        wayland
    ];
    LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
}

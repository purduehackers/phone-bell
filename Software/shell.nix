{ pkgs ? import <nixpkgs> {} }:
pkgs.mkShell {
    nativeBuildInputs = with pkgs.buildPackages; [ 
        cargo
        rustc
        llvm
        clang
        pkg-config
        systemd
        openssl
    ];
    LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
}

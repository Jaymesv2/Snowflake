{ pkgs, rustVersion}:

with pkgs;

# Configure your development environment.
#
# Documentation: https://github.com/numtide/devshell
devshell.mkShell {
  name = "Snowflake";
  motd = ''
    Hello
  '';
  commands = [
    /*{
      name = """;
      help = "";
      category = "";
      package = "";
    }*/
  ];

  env = [
    /*{
      name = "";
      value = "";
    }*/
  ];

  packages = [
    (rustVersion.override { extensions = [ "rust-src" ]; })
    protobuf
    binutils
    pkgconfig
    gcc
    glibc
    gmp.dev
    nixpkgs-fmt
  ];
}

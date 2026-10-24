# The verification image, built with Nix (spike, OI-TBD). See flake.nix for the pins.
{
  pkgs,
  pkgsQemu,
  pkgsGcc,
  root,
}:
let
  inherit (pkgs) lib;
  system = pkgs.stdenv.hostPlatform.system;
  aarch64 = pkgs.stdenv.hostPlatform.isAarch64;

  # --- cpp/toolchains.lock, read as the Containerfile's setup_cpp.py reads it -----------------
  lockLines = builtins.filter (l: builtins.match "[A-Z][A-Z0-9_]*=.*" l != null) (
    lib.splitString "\n" (builtins.readFile (root + "/cpp/toolchains.lock"))
  );
  lock = builtins.listToAttrs (
    map (
      l:
      let
        m = builtins.match "([A-Z][A-Z0-9_]*)=(.*)" l;
      in
      {
        name = builtins.elemAt m 0;
        value = builtins.elemAt m 1;
      }
    ) lockLines
  );
  arch = if aarch64 then "ARM64" else "X64";
  # An archive of the lock: the same URL, checked against the same SHA-256.
  archive =
    name: fileName:
    pkgs.fetchurl {
      url = lock."${name}_${arch}_URL";
      sha256 = lock."${name}_${arch}_SHA256";
      name = fileName;
    };

  # --- Rust: rust-toolchain.toml and tooling-nightly (RS-001, RS-002) -------------------------
  toolchainFile = (builtins.fromTOML (builtins.readFile (root + "/rust-toolchain.toml"))).toolchain;
  stableName = toolchainFile.channel;
  nightlyName = lib.removeSuffix "\n" (builtins.readFile (root + "/tooling-nightly"));
  # The Containerfile adds s390x for the big-endian check.
  stable = pkgs.rust-bin.stable.${stableName}.minimal.override {
    extensions = toolchainFile.components;
    targets = toolchainFile.targets ++ [ "s390x-unknown-linux-gnu" ];
  };
  nightly = pkgs.rust-bin.nightly.${lib.removePrefix "nightly-" nightlyName}.minimal.override {
    extensions = [
      "llvm-tools"
      "rust-src"
      "clippy"
    ];
  };

  # rustup's toolchain selection, which the checks use (`cargo +<nightly>`, `rustup run`), for
  # the two toolchains of the image: the first argument `+<name>`, else RUSTUP_TOOLCHAIN, else
  # the channel of rust-toolchain.toml. Any other toolchain is an error, as it would be in the
  # Containerfile's image, where rustup installs only these two.
  rustDispatch = pkgs.runCommand "rust-dispatch" { } ''
    mkdir -p $out/bin
    cat > $out/bin/rust-dispatch <<'EOF'
    #!${pkgs.bash}/bin/bash
    set -euo pipefail
    pick() {
      case "$1" in
        default|${stableName}) echo ${stable} ;;
        ${nightlyName}) echo ${nightly} ;;
        *) echo "toolchain '$1' is not installed in this image" >&2; return 1 ;;
      esac
    }
    name="''${0##*/}"
    if [ "$name" = rustup ]; then
      if [ "''${1:-}" != run ] || [ $# -lt 3 ]; then
        echo "rustup: this image has only 'rustup run <toolchain> <command>...'" >&2
        exit 1
      fi
      tc="$2"; shift 2
      root="$(pick "$tc")"
      export RUSTUP_TOOLCHAIN="$tc" PATH="$root/bin:$PATH"
      exec "$@"
    fi
    tc="''${RUSTUP_TOOLCHAIN:-default}"
    if [[ "''${1:-}" == +* ]]; then tc="''${1#+}"; shift; fi
    root="$(pick "$tc")"
    export RUSTUP_TOOLCHAIN="$tc" PATH="$root/bin:$PATH"
    exec "$root/bin/$name" "$@"
    EOF
    chmod 0755 $out/bin/rust-dispatch
    for t in cargo rustc rustdoc rustfmt cargo-fmt cargo-clippy clippy-driver rustup; do
      ln -s rust-dispatch $out/bin/$t
    done
  '';

  # Cargo tools at the Containerfile's versions, built with the pinned stable toolchain from
  # each crate's own Cargo.lock (`cargo install --locked`).
  rustPlatform = pkgs.makeRustPlatform {
    cargo = stable;
    rustc = stable;
  };
  crate =
    {
      pname,
      version,
      sha256,
      cargoHash,
      ...
    }@args:
    rustPlatform.buildRustPackage (
      {
        src = pkgs.fetchurl {
          url = "https://static.crates.io/crates/${pname}/${pname}-${version}.crate";
          name = "${pname}-${version}.tar.gz";
          inherit sha256;
        };
        doCheck = false;
      }
      // removeAttrs args [ "sha256" ]
    );
  cargoLlvmCov = crate {
    pname = "cargo-llvm-cov";
    version = "0.9.1";
    sha256 = "5e00647cc92940c4dd82249c480fe08fd3809767e89f056bcec46d8a7962ea5b";
    cargoHash = "sha256-uSMRu+OjdEyxYyGc65Z5vYOcHyOcgf1L3Kxc4QHRM2o=";
  };
  cargoDeny = crate {
    pname = "cargo-deny";
    version = "0.20.2";
    sha256 = "e528dfcbe739af7ce37a77d3d6df1b29dd6887b1c701d888820c0f16b864f737";
    cargoHash = "sha256-Zb6vQCnhhhL9Ducn9eh5P8Gfopl0lQPTXWW8Q0Y5xBQ=";
    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = [ pkgs.zstd ];
    env.ZSTD_SYS_USE_PKG_CONFIG = true;
  };
  cbindgen = crate {
    pname = "cbindgen";
    version = "0.29.4";
    sha256 = "2ecb53484c9c167ba674026b656d8a27d7657a58e6066aa902bfb1a4aa00ae20";
    cargoHash = "sha256-f6YoDoiVoh0BVPYHFO1FsdI4OCsF+LY72QaD57StdIQ=";
  };

  # --- QEMU 8.2.2 (QEMU_VERSION), only the machines and user modes the checks run -------------
  qemu = pkgsQemu.qemu.override {
    hostCpuTargets = [
      "arm-softmmu"
      "riscv32-softmmu"
      "sparc-softmmu"
      "s390x-linux-user"
    ];
    guestAgentSupport = false;
    numaSupport = false;
    alsaSupport = false;
    pulseSupport = false;
    pipewireSupport = false;
    sdlSupport = false;
    jackSupport = false;
    gtkSupport = false;
    vncSupport = false;
    smartcardSupport = false;
    spiceSupport = false;
    ncursesSupport = false;
    usbredirSupport = false;
    openGLSupport = false;
    virglSupport = false;
    rutabagaSupport = false;
    libiscsiSupport = false;
    tpmSupport = false;
    capstoneSupport = false;
    enableDocs = false;
  };

  # --- GCC 14.2.0 (GCC_VERSION) under the Ubuntu names the presets and checks use -------------
  gcc14 = pkgsGcc.gcc14;
  gccNames = pkgs.runCommand "gcc-14-names" { } ''
    mkdir -p $out/bin
    ln -s ${gcc14}/bin/gcc $out/bin/gcc-14
    ln -s ${gcc14}/bin/g++ $out/bin/g++-14
    ln -s ${gcc14}/bin/gcc $out/bin/cc
    ln -s ${gcc14}/bin/g++ $out/bin/c++
  '';

  # --- The archives of cpp/toolchains.lock, unpacked as setup_cpp.py unpacks them -------------
  # autoPatchelf points their ELF files at the image's C library: Nix has no /lib.
  hostTriple = if aarch64 then "aarch64-unknown-linux-gnu" else "x86_64-unknown-linux-gnu";
  llvmMajor = builtins.head (lib.splitString "." lock.LLVM_VERSION);
  llvmMembers = map (t: "bin/${t}") [
    "clang"
    "clang++"
    "clang-${llvmMajor}"
    "clang-tidy"
    "run-clang-tidy"
    "clang-format"
    "clang-scan-deps"
    "lld"
    "ld.lld"
    "llvm-cov"
    "llvm-profdata"
    "llvm-nm"
    "llvm-symbolizer"
    "llvm-readobj"
    "llvm-readelf"
    "llvm-objdump"
    "llvm-objcopy"
    "llvm-ar"
    "llvm-ranlib"
    "llvm-size"
    "llvm-cxxfilt"
  ] ++ [ "lib/clang" ];
  gccCc = gcc14.cc;
  glibc = pkgsGcc.glibc;
  # A Clang configuration file for the host triple only (a bare-metal --target does not load
  # it): where the C library and GCC 14.2.0's libstdc++ are, which the Ubuntu Clang finds under
  # /usr. Options from a configuration file are never reported as unused.
  clangCfg = pkgs.writeText "clang.cfg" ''
    --gcc-install-dir=${gccCc}/lib/gcc/${hostTriple}/${lock.GCC_VERSION}
    -idirafter ${glibc.dev}/include
    -B${glibc}/lib
    -L${glibc}/lib
    -L${gccCc.lib}/lib
    -Wl,-dynamic-linker=${pkgsGcc.stdenv.cc.bintools.dynamicLinker}
    -Wl,-rpath,${glibc}/lib
    -Wl,-rpath,${gccCc.lib}/lib
  '';
  llvm = pkgsGcc.stdenv.mkDerivation {
    pname = "llvm-release";
    version = lock.LLVM_VERSION;
    src = archive "LLVM" "LLVM-${lock.LLVM_VERSION}.tar.zst";
    nativeBuildInputs = [
      pkgsGcc.zstd
      pkgsGcc.autoPatchelfHook
    ];
    buildInputs = [
      gccCc.lib
      pkgsGcc.zlib
      pkgsGcc.zstd
      pkgsGcc.libxml2
      pkgsGcc.ncurses
      pkgsGcc.python3
    ];
    unpackPhase = ''
      top="LLVM-${lock.LLVM_VERSION}-Linux-${arch}"
      mkdir src
      zstd -dc --long=31 $src | tar -x -C src --strip-components=1 \
        ${lib.concatMapStringsSep " " (m: "\"$top/${m}\"") llvmMembers}
      cd src
    '';
    dontConfigure = true;
    dontBuild = true;
    dontStrip = true;
    installPhase = ''
      mkdir -p $out
      cp -a bin lib $out/
      cp ${clangCfg} $out/bin/${hostTriple}-clang.cfg
      cp ${clangCfg} $out/bin/${hostTriple}-clang++.cfg
    '';
  };
  unpacked =
    {
      name,
      fileName,
      stripTop ? true,
      ignoreMissing ? [ ],
    }:
    pkgsGcc.stdenv.mkDerivation {
      pname = lib.toLower name;
      version = lock."${name}_VERSION";
      src = archive name fileName;
      nativeBuildInputs = [
        pkgsGcc.autoPatchelfHook
        pkgsGcc.unzip
      ];
      buildInputs = [
        gccCc.lib
        pkgsGcc.zlib
        pkgsGcc.zstd
        pkgsGcc.ncurses
        pkgsGcc.expat
      ];
      autoPatchelfIgnoreMissingDeps = ignoreMissing;
      sourceRoot = ".";
      unpackPhase =
        if lib.hasSuffix ".zip" fileName then
          "unzip -q $src -d tool"
        else
          "mkdir tool && tar -xf $src -C tool ${lib.optionalString stripTop "--strip-components=1"}";
      dontConfigure = true;
      dontBuild = true;
      dontStrip = true;
      installPhase = "cp -a tool $out";
    };
  cmake = unpacked {
    name = "CMAKE";
    fileName = "cmake-${lock.CMAKE_VERSION}.tar.gz";
  };
  ninja = unpacked {
    name = "NINJA";
    fileName = "ninja.zip";
    stripTop = false;
  };
  # The debuggers need a Python the checks never use.
  armGnu = unpacked {
    name = "ARM_GNU";
    fileName = "arm-gnu-toolchain.tar.xz";
    ignoreMissing = [ "libpython3.8.so.1.0" ];
  };
  riscvGnu = unpacked {
    name = "RISCV_GNU";
    fileName = "riscv-none-elf-gcc.tar.gz";
  };

  # setup_cpp.py's GITHUB_PATH and GITHUB_ENV files, which entrypoint.sh applies.
  cppPath = pkgs.writeText "path" ''
    ${llvm}/bin
    ${armGnu}/bin
    ${riscvGnu}/bin
    ${cmake}/bin
    ${ninja}
  '';
  cppEnv = pkgs.writeText "env" ''
    ALDIS_LLVM_ROOT=${llvm}
    ALDIS_ARM_GNU_ROOT=${armGnu}
    ALDIS_RISCV_GNU_ROOT=${riscvGnu}
  '';

  # --- The image ------------------------------------------------------------------------------
  env = pkgs.buildEnv {
    name = "aldis-verify-env";
    paths = with pkgs; [
      bashInteractive
      coreutils
      findutils
      diffutils
      gnugrep
      gnused
      gawk
      gnutar
      gzip
      xz
      zstd
      unzip
      which
      util-linuxMinimal # flock
      procps
      git
      python3
      curl
      rustDispatch
      cargoLlvmCov
      cargoDeny
      cbindgen
      qemu
      gccNames
    ];
    pathsToLink = [ "/bin" ];
  };
  # The record lists every package with its version (VER-057); here, every store path.
  closure = pkgs.closureInfo { rootPaths = [ env cppPath cppEnv ]; };
  dpkgQuery = pkgs.writeShellScriptBin "dpkg-query" ''
    # The image's packages for the record (VER-057): every store path, as name=hash.
    while read -r p; do
      n="''${p#/nix/store/}"
      echo "''${n#*-}=''${n%%-*}"
    done < /opt/aldis-nix/store-paths
  '';
  gitconfig = pkgs.writeText "gitconfig" ''
    [safe]
    	directory = *
  '';
  image = pkgs.dockerTools.streamLayeredImage {
    name = "localhost/aldis-verify-nix";
    tag = "spike";
    contents = [
      env
      dpkgQuery
      pkgs.dockerTools.usrBinEnv
      pkgs.dockerTools.caCertificates
      pkgs.dockerTools.fakeNss
    ];
    extraCommands = ''
      mkdir -p tmp usr/local/bin opt/aldis-cpp opt/aldis-nix etc
      chmod 1777 tmp
      install -m 0755 ${root + "/xtask/verify/entrypoint.sh"} usr/local/bin/aldis-verify
      cp ${cppPath} opt/aldis-cpp/path
      cp ${cppEnv} opt/aldis-cpp/env
      cp ${closure}/store-paths opt/aldis-nix/store-paths
      cp ${gitconfig} etc/gitconfig
    '';
    config = {
      Entrypoint = [ "/usr/local/bin/aldis-verify" ];
      Env = [
        "PATH=/bin:/usr/bin:/usr/local/bin"
        "LANG=C.UTF-8"
        "SSL_CERT_FILE=/etc/ssl/certs/ca-bundle.crt"
        "GIT_SSL_CAINFO=/etc/ssl/certs/ca-bundle.crt"
      ];
      Labels."dev.aldis.verify.builder" = "nix";
    };
    maxLayers = 100;
  };
in
{
  inherit
    image
    env
    stable
    nightly
    qemu
    gcc14
    llvm
    cmake
    ninja
    armGnu
    riscvGnu
    cargoLlvmCov
    cargoDeny
    cbindgen
    ;
  default = image;
}

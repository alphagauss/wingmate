#!/usr/bin/env bash
# Build the Wingmate ARM64 Linux guest and PRoot binaries.
# Adapted from dsh-Go runtime/build-runtime.sh (see README.md).
set -euo pipefail
shopt -s inherit_errexit

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
CACHE="$ROOT/work/cache/android"
OUT="$ROOT/work/build/android"
LOCKS="$HERE/locks"
PY=(python3 -I "$HERE/lib/rtbuild.py")
RELEASE_SOURCES="${1:-}"
[[ $# -le 1 ]] || { echo "Usage: $0 [release-sources-dir]" >&2; exit 2; }
NODE_VERSION=24.22.1
PNPM_VERSION=11.28.5
UBUNTU_VERSION=24.04.5
PROOT_VERSION=5.1.107.102
TALLOC_VERSION=2.5.0
SHMEM_VERSION=0.7
TERMUX_RECIPE=b734bc49dc0eedb2c454e1c5d3c48d35d81d4bc9
PYTHON_VERSION=3.12.15
PBS_RELEASE=20261021
PBS_RECIPE=0c5e581b54a4736cdbbe68bdd6d7e571a1dca11e
SHARP_RECIPE=ebb95f8add54eee8bed840e3fb587e4cbec857d7
NPM_REGISTRY="${npm_config_registry:-https://registry.npmmirror.com}"

declare -A PIN=(
  [ubuntu-base-24.04.5-base-arm64.tar.gz]=a91d5a93010193712d346d761372b7c9db6dfcf093893161c64ca107f05914f2
  [node-v24.22.1-linux-arm64.tar.xz]=dbabf7a1cbf947e6059ffad2c27b80041db97dea1e553b62afdee7f13fca336e
  [node-v24.22.1-linux-x64.tar.xz]=6c172a9ab6718f7b801fb5a7bd486af2ee5e32758086a04b84fcce2aa77095e6
  [proot_5.1.107.102_aarch64.deb]=10a394b585c2fc2b2c8502544f4e5f1be581dc8bfff79e8ba62f16207423e030
  [libtalloc_2.5.0_aarch64.deb]=556591f43bb773ad8777e1a29522640866a55f95dab71914418b94a8c58ad5a7
  [libandroid-shmem_0.7_aarch64.deb]=0da3a24d558b93c92bcf8d611e0826a99ff96e396b148e6cdf33b47c47c57ff6
  [cpython-3.12.15+20261021-aarch64-unknown-linux-gnu-install_only_stripped.tar.gz]=b4567aa5a11f51b540c4dfd9e082ebf23e554f8ac17b85caa6cea23e83693394
  # GPL-2.0 source of the proot binary (same archive Termux's build recipe pins); used for COPYING.
  [proot-v5.1.107.102.zip]=30d12a6e2cc4147123756ff58edea262d3083bbc238c12d0ab9b39e3d1c40852
  [talloc-2.5.0.tar.gz]=912afa237510ae542a7733998eb18a12bcda35ab6729c8e2ddb43e8d0ebab007
  [libandroid-shmem-0.7.tar.gz]=1e5ff8459bc0a8c229dd8a94b27d119987e09ef3414331c2b5ebfff20b98e867
  [termux-packages-b734bc49dc0eedb2c454e1c5d3c48d35d81d4bc9.tar.gz]=71a87144a42f47c3e5951e1cd4cdf5364730746127b1fccf9066c1b12d69f511
  [python-build-standalone-0c5e581b54a4736cdbbe68bdd6d7e571a1dca11e.tar.gz]=11307e854a65298fbb986f5d758e1c60fc1f5da77f187b5329c4dd45a3b870f2
  [Python-3.12.15.tar.xz]=c2c4321961fab0fb999d66e0cecf521c2ab3994c7992873ea99e306c1094fd5a
  [sharp-libvips-ebb95f8add54eee8bed840e3fb587e4cbec857d7.tar.gz]=65fdb633c2d97626352a3771b7a0ce30b6a89af03d2a1238491c8b068e6b8dad
  [cairo-1.18.6.tar.xz]=1c767308174337a74694da0f3ec069c271452163a1ef4540964c50c301f157d4
)


log() { printf '\033[1;34m[runtime %s]\033[0m %s\n' "$(date +%H:%M:%S)" "$*" >&2; }
die() { printf '\033[1;31m[runtime] error:\033[0m %s\n' "$*" >&2; exit 1; }
sha256() { sha256sum "$1" | cut -d' ' -f1; }


for tool in curl flock sha256sum python3 tar xz; do
  command -v "$tool" >/dev/null || die "missing host tool: $tool"
done

fetch() {
  local dest="$1" want="$2" url
  shift 2
  mkdir -p "$(dirname "$dest")"
  if [[ -f "$dest" && "$(sha256 "$dest")" == "$want" ]]; then return 0; fi
  for url in "$@"; do
    log "download $url"
    if curl -fL --retry 1 --connect-timeout 10 --max-time 120 -sS -o "$dest.part" "$url"; then
      [[ "$(sha256 "$dest.part")" == "$want" ]] || die "sha256 mismatch for $url"
      mv "$dest.part" "$dest"
      return
    fi
  done
  die "download failed for $dest"
}

node_dist() {
  local name="node-v$NODE_VERSION-linux-$1.tar.xz"
  local dest="$CACHE/node/$name"
  fetch "$dest" "${PIN[$name]}" "https://npmmirror.com/mirrors/node/v$NODE_VERSION/$name" \
    "https://nodejs.org/dist/v$NODE_VERSION/$name"
  printf '%s\n' "$dest"
}

termux_deb() {
  local pkg="$1" name="${1}_${2}_aarch64.deb" pool="${1:0:1}"
  [[ "$pkg" != lib* ]] || pool="${pkg:0:4}"
  local path="pool/main/$pool/$pkg/$name"
  local dest="$CACHE/termux/$name"
  fetch "$dest" "${PIN[$name]}" "https://packages.termux.dev/apt/termux-main/$path" \
    "https://mirrors.tuna.tsinghua.edu.cn/termux/apt/termux-main/$path"
  printf '%s\n' "$dest"
}

ubuntu_extra_debs() {
  local name file sum dest
  python3 -I -c 'import json,sys; [print(p["filename"], p["sha256"]) for p in json.load(open(sys.argv[1]))["packages"]]' \
    "$LOCKS/ubuntu-noble-arm64.json" |
  while read -r file sum; do
    name="$(basename "$file")"
    dest="$CACHE/ubuntu/debs/$name"
    fetch "$dest" "$sum" "https://ports.ubuntu.com/ubuntu-ports/$file" \
      "https://mirrors.cloud.tencent.com/ubuntu-ports/$file" \
      "https://launchpad.net/ubuntu/+archive/primary/+files/$name"
    printf '%s\n' "$dest"
  done
}

install_npm_tree() {
  local lockdir="$1" prefix="$2"
  mkdir -p "$prefix"
  cp "$lockdir/package.json" "$lockdir/package-lock.json" "$prefix/"
  touch "$WORK/user.npmrc" "$WORK/global.npmrc"
  (
    cd "$prefix"
    npm ci --omit=dev --cpu=arm64 --os=linux --libc=glibc --ignore-scripts \
      --userconfig="$WORK/user.npmrc" --globalconfig="$WORK/global.npmrc" --no-audit --no-fund \
      --cache="$CACHE/npm" --registry="$NPM_REGISTRY" --replace-registry-host=always
  )
}

build_file_viewer() {
  local ws="$WORK/workspace" pnpm="$WORK/pnpm-prefix/node_modules/pnpm/bin/pnpm.mjs"
  mkdir -p "$ws/plugins/wingmate-file-viewer"
  cp "$ROOT/package.json" "$ROOT/pnpm-lock.yaml" "$ROOT/pnpm-workspace.yaml" "$ws/"
  tar -C "$ROOT/plugins/wingmate-file-viewer" --exclude=node_modules --exclude=dist -cf - . |
    tar -C "$ws/plugins/wingmate-file-viewer" -xf -
  (
    cd "$ws"
    npm_config_registry="$NPM_REGISTRY" node "$pnpm" --filter dsh-wingmate-file-viewer install \
      --frozen-lockfile --store-dir "$CACHE/pnpm"
    cd plugins/wingmate-file-viewer
    npm run build
    npm pack --ignore-scripts --pack-destination "$WORK" --cache "$CACHE/npm"
  )
  mkdir "$WORK/file-viewer"
  tar -xzf "$WORK"/dsh-wingmate-file-viewer-*.tgz --strip-components=1 -C "$WORK/file-viewer"
}

build_runtime() {
  local abi=arm64-v8a
  local work="$WORK" out="$WORK/output"
  local assets="$out/assets/runtime" jni="$out/jniLibs/$abi"
  mkdir -p "$assets/licenses" "$jni"

  local ubuntu_name="ubuntu-base-$UBUNTU_VERSION-base-arm64.tar.gz"
  local python_name="cpython-$PYTHON_VERSION+$PBS_RELEASE-aarch64-unknown-linux-gnu-install_only_stripped.tar.gz"
  local base="$CACHE/ubuntu/$ubuntu_name" python="$CACHE/python/$python_name"
  fetch "$base" "${PIN[$ubuntu_name]}" "https://cdimage.ubuntu.com/ubuntu-base/releases/24.04/release/$ubuntu_name" \
    "https://mirrors.tuna.tsinghua.edu.cn/ubuntu-cdimage/ubuntu-base/releases/24.04/release/$ubuntu_name"
  fetch "$python" "${PIN[$python_name]}" "https://registry.npmmirror.com/-/binary/python-build-standalone/$PBS_RELEASE/$python_name" \
    "https://github.com/astral-sh/python-build-standalone/releases/download/$PBS_RELEASE/$python_name"
  local node proot_deb talloc_deb shmem_deb proot_src
  node="$(node_dist arm64)"
  ubuntu_extra_debs > "$work/extra-debs.txt"
  local -a extra_debs=()
  mapfile -t extra_debs < "$work/extra-debs.txt"
  proot_deb="$(termux_deb proot "$PROOT_VERSION")"
  talloc_deb="$(termux_deb libtalloc "$TALLOC_VERSION")"
  shmem_deb="$(termux_deb libandroid-shmem "$SHMEM_VERSION")"
  local proot_name="proot-v$PROOT_VERSION.zip"
  proot_src="$CACHE/termux/$proot_name"
  fetch "$proot_src" "${PIN[$proot_name]}" "https://codeload.github.com/termux/proot/zip/refs/tags/v$PROOT_VERSION"
  local talloc_name="talloc-$TALLOC_VERSION.tar.gz" talloc_src="$CACHE/termux/talloc-$TALLOC_VERSION.tar.gz"
  fetch "$talloc_src" "${PIN[$talloc_name]}" \
    "https://ftp.osuosl.org/pub/blfs/conglomeration/talloc/$talloc_name" \
    "https://www.samba.org/ftp/talloc/$talloc_name"

  if [[ -n "$RELEASE_SOURCES" ]]; then
    local shmem_name="libandroid-shmem-$SHMEM_VERSION.tar.gz" recipes="termux-packages-$TERMUX_RECIPE.tar.gz"
    local shmem_src="$CACHE/termux/$shmem_name"
    fetch "$shmem_src" "${PIN[$shmem_name]}" \
      "https://codeload.github.com/termux/libandroid-shmem/tar.gz/refs/tags/v$SHMEM_VERSION"
    mkdir -p "$RELEASE_SOURCES/termux"
    cp "$proot_src" "$talloc_src" "$shmem_src" "$RELEASE_SOURCES/termux/"
    fetch "$CACHE/termux/$recipes" "${PIN[$recipes]}" \
      "https://codeload.github.com/termux/termux-packages/tar.gz/$TERMUX_RECIPE"
    cp "$CACHE/termux/$recipes" "$RELEASE_SOURCES/termux/"
    local pbs_recipes="python-build-standalone-$PBS_RECIPE.tar.gz" cpython="Python-$PYTHON_VERSION.tar.xz"
    fetch "$CACHE/python/$pbs_recipes" "${PIN[$pbs_recipes]}" \
      "https://codeload.github.com/astral-sh/python-build-standalone/tar.gz/$PBS_RECIPE"
    fetch "$CACHE/python/$cpython" "${PIN[$cpython]}" \
      "https://www.python.org/ftp/python/$PYTHON_VERSION/$cpython"
    mkdir -p "$RELEASE_SOURCES/python"
    cp "$CACHE/python/"{"$pbs_recipes","$cpython"} "$RELEASE_SOURCES/python/"

    local sharp_recipes="sharp-libvips-$SHARP_RECIPE.tar.gz" cairo="cairo-1.18.6.tar.xz"
    fetch "$CACHE/sharp/$sharp_recipes" "${PIN[$sharp_recipes]}" \
      "https://codeload.github.com/lovell/sharp-libvips/tar.gz/$SHARP_RECIPE"
    fetch "$CACHE/sharp/$cairo" "${PIN[$cairo]}" "https://cairographics.org/releases/$cairo"
    mkdir -p "$RELEASE_SOURCES/sharp-libvips"
    cp "$CACHE/sharp/"{"$sharp_recipes","$cairo"} "$RELEASE_SOURCES/sharp-libvips/"

    python3 -B "$HERE/release-sources.py" --base "$base" "${extra_debs[@]/#/--deb=}" \
      --cache "$CACHE/sources/ubuntu" --out "$RELEASE_SOURCES/ubuntu"
  fi

  # -- jniLibs: Termux binaries, renamed lib*.so so the package manager extracts them --
  local debx="$work/debs"
  local d
  for d in "$proot_deb" "$talloc_deb" "$shmem_deb"; do "${PY[@]}" deb-extract "$d" "$debx"; done
  local tprefix="$debx/data/data/com.termux/files/usr"
  cp "$tprefix/bin/proot"                  "$jni/libproot.so"
  cp "$tprefix/libexec/proot/loader"       "$jni/libproot-loader.so"
  cp "$tprefix/lib/libtalloc.so.$TALLOC_VERSION" "$jni/libtalloc.so"
  cp "$tprefix/lib/libandroid-shmem.so"    "$jni/libandroid-shmem.so"
  local orig_proot orig_talloc
  orig_proot="$(sha256 "$jni/libproot.so")"; orig_talloc="$(sha256 "$jni/libtalloc.so")"
  # APKs can only carry lib*.so, so the DT_NEEDED/SONAME "libtalloc.so.2" becomes "libtalloc.so".
  "${PY[@]}" patch-dynstr "$jni/libproot.so" libtalloc.so.2 libtalloc.so >&2
  "${PY[@]}" patch-dynstr "$jni/libtalloc.so" libtalloc.so.2 libtalloc.so >&2
  chmod 0755 "$jni"/*.so
  "${PY[@]}" elf-info "$jni"/*.so > "$work/jni-elf.json"

  python3 -I - "$jni" "$work/jni-elf.json" "$work/jnilibs.json" <<PYEOF
import hashlib, json, os, sys
jni, elf, out = sys.argv[1:4]
info = json.load(open(elf))
src = {
  "libproot.so": ("proot", "$PROOT_VERSION", "$(basename "$proot_deb")", "usr/bin/proot",
                  "DT_NEEDED libtalloc.so.2 -> libtalloc.so", "$orig_proot"),
  "libproot-loader.so": ("proot", "$PROOT_VERSION", "$(basename "$proot_deb")", "usr/libexec/proot/loader", None, None),
  "libtalloc.so": ("libtalloc", "$TALLOC_VERSION", "$(basename "$talloc_deb")", "usr/lib/libtalloc.so.$TALLOC_VERSION",
                   "DT_SONAME libtalloc.so.2 -> libtalloc.so", "$orig_talloc"),
  "libandroid-shmem.so": ("libandroid-shmem", "$SHMEM_VERSION", "$(basename "$shmem_deb")", "usr/lib/libandroid-shmem.so", None, None),
}
res = {}
for name, (pkg, ver, deb, inner, patch, orig) in src.items():
    path = os.path.join(jni, name)
    e = info[path]
    res[name] = {
        "sha256": hashlib.sha256(open(path, "rb").read()).hexdigest(),
        "bytes": os.path.getsize(path),
        "source": "termux-main %s %s (%s:/data/data/com.termux/files/%s)" % (pkg, ver, deb, inner),
        "patch": patch, "originalSha256": orig,
        "machine": e["machine"], "type": e["type"], "needed": e.get("needed"),
        "soname": e.get("soname"), "minLoadAlign": e["minLoadAlign"],
    }
json.dump(res, open(out, "w"), indent=2)
PYEOF
  # licenses for the native bits
  python3 -I - "$proot_src" "$assets/licenses/proot-COPYING.txt" <<'PYEOF'
import sys, zipfile
z = zipfile.ZipFile(sys.argv[1])
name = next(n for n in z.namelist() if n.endswith("/COPYING") and n.count("/") == 1)
open(sys.argv[2], "wb").write(z.read(name))
PYEOF
  cp "$debx/data/data/com.termux/files/usr/share/doc/libandroid-shmem/copyright" "$assets/licenses/libandroid-shmem-copyright.txt"
  sed -n '1,/\*\//p' "$tprefix/include/talloc.h" > "$assets/licenses/libtalloc-notice.txt"
  tar -xOzf "$talloc_src" "talloc-$TALLOC_VERSION/LICENSE" > "$assets/licenses/libtalloc-LICENSE.txt"
  tar -xOzf "$base" usr/share/common-licenses/GPL-3 > "$assets/licenses/gpl-3.0.txt"
  tar -xOzf "$base" usr/share/common-licenses/LGPL-3 > "$assets/licenses/lgpl-3.0.txt"
  tar -xOzf "$base" usr/share/common-licenses/MPL-1.1 > "$assets/licenses/mpl-1.1.txt"
  cp "$HERE/SOURCES.md" "$assets/licenses/runtime-sources.md"


  cp "$HERE/LICENSE.dsh-go" "$assets/licenses/dsh-go-AGPL-3.0.txt"

  local prefix="$work/dsh-prefix" pnpm_prefix="$work/pnpm-prefix" host_arch host_node
  case "$(uname -m)" in
    x86_64) host_arch=x64 ;;
    aarch64) host_arch=arm64 ;;
    *) die "unsupported Linux build host architecture" ;;
  esac
  host_node="$CACHE/host-node-$host_arch"
  if [[ ! -f "$host_node/.complete" ]]; then
    local host_archive
    host_archive="$(node_dist "$host_arch")"
    mkdir -p "$host_node"
    tar -xJf "$host_archive" --strip-components=1 -C "$host_node"
    touch "$host_node/.complete"
  fi
  export PATH="$host_node/bin:$PATH"
  install_npm_tree "$ROOT/runtime" "$prefix"
  install_npm_tree "$LOCKS/pnpm-$PNPM_VERSION" "$pnpm_prefix"
  build_file_viewer
  node "$HERE/prepare-dsh.mjs" "$prefix" "$work/dsh-excluded.json"

  local dsh_version patch="$work/dsh-web.patch.yml"
  dsh_version="$(node -p 'require(process.argv[1]).version' "$prefix/node_modules/@deepseek-ai/dsh/package.json")"
  {
    sed -n '/^- id: office-to-pdf$/,+1p' "$ROOT/plugins/cordis.patch.yml"
    cat <<'YAML'
- id: workspace-controller
  config:
    documentsDirectory: /root/Documents
- insert:
    - id: wingmate-file-viewer
      name: /opt/dsh/plugins/wingmate-file-viewer/host.mjs
YAML
  } > "$patch"

  local input_hash runtime_version versions
  input_hash="$(
    {
      sha256sum "$base" "$node" "$python" "${extra_debs[@]}" "$proot_deb" "$talloc_deb" "$shmem_deb" |
        awk '{print $1}'
      sha256sum "$HERE/build-runtime.sh" "$HERE/lib/rtbuild.py" "$HERE/prepare-dsh.mjs" "$HERE/guest/"*.mjs \
        "$HERE/patches/dsh-$dsh_version.json" "$HERE/webview-compat.js" \
        "$HERE/LICENSE.dsh-go" "$HERE/SOURCES.md" "$ROOT/runtime/package.json" "$ROOT/runtime/package-lock.json" \
        "$LOCKS/pnpm-$PNPM_VERSION/package.json" "$LOCKS/pnpm-$PNPM_VERSION/package-lock.json" \
        "$ROOT/scripts/runtime-files.mjs" "$ROOT/scripts/runtime-patches.mjs" \
        "$ROOT/runtime/patches/dsh-$dsh_version.json" "$patch" | awk '{print $1}'
      sha256sum "$WORK"/dsh-wingmate-file-viewer-*.tgz | awk '{print $1}'
    } | sha256sum | cut -d' ' -f1
  )"
  runtime_version="dsh-$dsh_version-$input_hash"
  versions="$(printf '{"runtimeVersion":"%s","abi":"%s","dshVersion":"%s","nodeVersion":"%s","pnpmVersion":"%s","ubuntuVersion":"%s","pythonVersion":"%s","prootVersion":"%s","tallocVersion":"%s","androidShmemVersion":"%s"}' \
    "$runtime_version" "$abi" "$dsh_version" "$NODE_VERSION" "$PNPM_VERSION" "$UBUNTU_VERSION" \
    "$PYTHON_VERSION" "$PROOT_VERSION" "$TALLOC_VERSION" "$SHMEM_VERSION")"

  "${PY[@]}" assemble --abi "$abi" --base "$base" --node "$node" --python "$python" \
    --dsh-prefix "$prefix" --dsh-excluded "$work/dsh-excluded.json" --pnpm-prefix "$pnpm_prefix" \
    --file-viewer "$work/file-viewer" --web-patch "$patch" --out "$assets/rootfs-$abi.tar.xz" \
    --report "$out/build-report.json" --runtime-info "$versions" "${extra_debs[@]/#/--deb=}"
  "${PY[@]}" manifest --abi "$abi" --report "$out/build-report.json" --jnilibs "$work/jnilibs.json" \
    --versions "$versions" --out "$assets/manifest.json"

  # Publish one atomic build input for Gradle. XZ and native ELF stay byte-identical.
  python3 -I - "$out" "$OUT/runtime-$abi.zip" <<'PYEOF'
import os
import pathlib
import shutil
import sys
import zipfile

out, dest = map(pathlib.Path, sys.argv[1:])
temporary = dest.with_suffix(".part")
with zipfile.ZipFile(temporary, "w") as bundle:
    for file in sorted(out.rglob("*")):
        if file.is_file():
            entry = zipfile.ZipInfo(file.relative_to(out).as_posix(), (2026, 1, 1, 0, 0, 0))
            with file.open("rb") as source, bundle.open(entry, "w") as target:
                shutil.copyfileobj(source, target)
os.replace(temporary, dest)
print(dest)
PYEOF
}

mkdir -p "$CACHE" "$OUT"
exec 9>"$CACHE/build.lock"
flock 9
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
build_runtime

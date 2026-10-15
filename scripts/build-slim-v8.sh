#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

OUT=${1:-${CARGO_TARGET_DIR:-target}/slim-v8/librusty_v8.a}
JOBS=${JOBS:-$(nproc 2>/dev/null || echo 4)}
CACHE=${KAPLAY_CACHE_DIR:-$HOME/.cache/kaplay-native}
LLVM_VERSION=23.1.3

if [ "$(uname -s)" != Linux ]; then
    echo "build-slim-v8.sh currently supports Linux hosts only" >&2
    exit 1
fi

case "$(uname -m)" in
    x86_64) GN_PKG=linux-amd64; LLVM_ASSET=Linux-X64 ;;
    aarch64) GN_PKG=linux-arm64; LLVM_ASSET=Linux-ARM64 ;;
    *)
        echo "unsupported architecture: $(uname -m)" >&2
        exit 1
        ;;
esac

mkdir -p "$CACHE"

if [ -z "${GN:-}" ] && ! command -v gn >/dev/null 2>&1; then
    GN_DIR=$CACHE/gn-$GN_PKG
    if [ ! -x "$GN_DIR/gn" ]; then
        echo "downloading gn"
        curl -fsSL -o "$CACHE/gn-$GN_PKG.zip" \
            "https://chrome-infra-packages.appspot.com/dl/gn/gn/$GN_PKG/+/latest"
        mkdir -p "$GN_DIR"
        unzip -oq "$CACHE/gn-$GN_PKG.zip" -d "$GN_DIR"
        chmod +x "$GN_DIR/gn"
        rm -f "$CACHE/gn-$GN_PKG.zip"
    fi
    export GN=$GN_DIR/gn
fi

if [ -z "${LIBCLANG_PATH:-}" ] && [ -z "${CLANG_BASE_PATH:-}" ]; then
    for base in /usr/lib/llvm-2* /usr/lib/llvm-3*; do
        [ -x "$base/bin/clang" ] || continue
        if ls "$base"/lib/libclang.so.2[1-9]* "$base"/lib/libclang.so.3* >/dev/null 2>&1; then
            export LIBCLANG_PATH=$base/lib
            export CLANG_BASE_PATH=$base
            break
        fi
    done
fi

if [ -z "${LIBCLANG_PATH:-}" ] || [ -z "${CLANG_BASE_PATH:-}" ]; then
    LLVM_DIR=$CACHE/llvm-$LLVM_VERSION
    if [ ! -e "$LLVM_DIR/lib/libclang.so" ] || [ ! -x "$LLVM_DIR/bin/clang" ]; then
        echo "downloading LLVM $LLVM_VERSION (libclang 21+ and a native clang; Chromium's download is x64-only)"
        mkdir -p "$LLVM_DIR"
        LLVM_MAJOR=${LLVM_VERSION%%.*}
        if command -v zstd >/dev/null 2>&1; then
            curl -fsSL -o "$CACHE/llvm.tar.zst" \
                "https://github.com/llvm/llvm-project/releases/download/llvmorg-$LLVM_VERSION/LLVM-$LLVM_VERSION-$LLVM_ASSET.tar.zst"
            zstd -d --long=30 -c "$CACHE/llvm.tar.zst" |
                tar -x -C "$LLVM_DIR" --strip-components=1 --wildcards \
                    '*/lib/*' '*/bin/clang' '*/bin/clang++' "*/bin/clang-$LLVM_MAJOR" \
                    '*/bin/llvm-*' '*/bin/lld' '*/bin/ld.lld'
            rm -f "$CACHE/llvm.tar.zst"
        else
            curl -fsSL -o "$CACHE/llvm.tar.xz" \
                "https://github.com/llvm/llvm-project/releases/download/llvmorg-$LLVM_VERSION/LLVM-$LLVM_VERSION-$LLVM_ASSET.tar.xz"
            tar -xJf "$CACHE/llvm.tar.xz" -C "$LLVM_DIR" --strip-components=1 --wildcards \
                '*/lib/*' '*/bin/clang' '*/bin/clang++' "*/bin/clang-$LLVM_MAJOR" \
                '*/bin/llvm-*' '*/bin/lld' '*/bin/ld.lld'
            rm -f "$CACHE/llvm.tar.xz"
        fi
    fi
    export LIBCLANG_PATH=${LIBCLANG_PATH:-$LLVM_DIR/lib}
    export CLANG_BASE_PATH=${CLANG_BASE_PATH:-$LLVM_DIR}
fi

export NUM_JOBS=$JOBS
export V8_FROM_SOURCE=1
export GN_ARGS="${GN_ARGS:+$GN_ARGS }v8_enable_i18n_support=false v8_enable_webassembly=false v8_enable_temporal_support=false use_glib=false"

if [ -z "${RUSTFLAGS:-}" ]; then
    for dir in "$HOME"/.rustup/toolchains/*/lib/rustlib/*/bin/gcc-ld; do
        if [ -x "$dir/ld.lld" ]; then
            export PATH=$dir:$PATH
            break
        fi
    done
    if command -v ld.lld >/dev/null 2>&1; then
        export RUSTFLAGS="-C force-unwind-tables=no -C link-arg=-fuse-ld=lld -C link-arg=-Wl,--icf=all"
    fi
fi

echo "building slim V8 without i18n, WebAssembly and Temporal (jobs=$JOBS)"
cargo build --release -p kaplay-native-host --no-default-features --features v8

TARGET_DIR=${CARGO_TARGET_DIR:-target}
ARCHIVE=$TARGET_DIR/release/gn_out/obj/librusty_v8.a
mkdir -p "$(dirname "$OUT")"
cp "$ARCHIVE" "$OUT"
echo "slim V8 archive: $OUT"
echo "rebuild any host against it with RUSTY_V8_ARCHIVE=$OUT"

#!/usr/bin/env bash
# Build one distribution package, then install it in a fresh container.
set -euo pipefail
format=${1:?usage: native.sh deb|rpm|arch|tumbleweed|void INPUT OUTPUT}
input=$(realpath "${2:?}")
output=${3:?}
root=$(git rev-parse --show-toplevel)
cd "$root"
source scripts/packaging/environment
case "$format" in
    deb) build_image=$DEBIAN_IMAGE; install_image=$DEBIAN_IMAGE ;;
    rpm) build_image=$FEDORA_IMAGE; install_image=$FEDORA_IMAGE ;;
    arch) build_image=$ARCH_IMAGE; install_image=$ARCH_IMAGE ;;
    tumbleweed) build_image=$TUMBLEWEED_IMAGE; install_image=$TUMBLEWEED_IMAGE ;;
    void) build_image=$VOID_BUILD_IMAGE; install_image=$VOID_IMAGE ;;
    *) echo 'Unknown package format' >&2; exit 1 ;;
esac
mkdir -p "$output"
output=$(realpath "$output")
[[ "$output" != "$root" && "$output" != "$root/"* ]]
[[ -z $(ls -A "$output") ]] || { echo 'Output directory must be empty' >&2; exit 1; }
docker run --rm --platform linux/amd64 \
    -v "$root:/source:ro" -v "$input:/input:ro" -v "$output:/output" \
    -e CI_JOBS="${CI_JOBS:-2}" "$build_image" bash /source/scripts/packaging/native-build.sh "$format"
docker run --rm --platform linux/amd64 \
    -v "$root:/source:ro" -v "$output:/output:ro" \
    "$install_image" bash /source/scripts/packaging/native-install.sh "$format"
# This receipt is only written after the fresh-container check succeeds.
python3 - "$input/source.json" "$output" "$format" <<'PY'
import hashlib, json, pathlib, sys
identity = json.load(open(sys.argv[1]))
out = pathlib.Path(sys.argv[2])
receipt = {'source': identity, 'format': sys.argv[3], 'clean_install': True,
           'packages': {p.name: hashlib.file_digest(p.open('rb'), 'sha256').hexdigest()
                        for p in sorted((out / 'packages').iterdir()) if p.is_file()}}
(out / 'verified.json').write_text(json.dumps(receipt, indent=2) + '\n')
PY

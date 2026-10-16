#!/usr/bin/env bash
# Copyright (C) 2026 Red Hat, Inc.
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
# http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.
#
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 LOCAL_IMAGE_PREFIX" >&2
  exit 2
fi

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
work_dir=$(mktemp -d "${TMPDIR:-/tmp}/osbi-addon-fixtures.XXXXXX")
trap 'rm -rf -- "$work_dir"' EXIT
local_prefix=$1
libc_annotation=ai.openkaiden.openshell-build-image.libc

variant_tag() {
  local fixture=$1 architecture=$2 libc=$3
  local variant="linux-$architecture"
  if [[ -n $libc ]]; then variant="${variant}-${libc}"; fi
  printf '%s-%s-%s:fixture' "$local_prefix" "$fixture" "$variant"
}

build_variant() {
  local fixture=$1 architecture=$2 libc=$3
  local variant="linux-$architecture"
  if [[ -n $libc ]]; then variant="${variant}-${libc}"; fi
  local context="$work_dir/$fixture-$variant"
  local marker_root="$context/usr/local/share/openshell-addon-test"
  local fixture_env
  fixture_env=$(printf '%s' "$fixture" | tr '[:lower:]-' '[:upper:]_')
  mkdir -p "$marker_root/$fixture/variants"
  cp "$script_dir/Containerfile" "$context/Containerfile"
  printf 'fixture=%s\nos=linux\narchitecture=%s\nlibc=%s\nvariant=%s\n' \
    "$fixture" "$architecture" "${libc:-none}" "$variant" \
    > "$marker_root/$fixture/selected"
  cp "$marker_root/$fixture/selected" "$marker_root/$fixture/variants/$variant"
  printf '%s:%s\n' "$fixture" "$variant" > "$marker_root/last-applied"
  podman build --format oci --pull=never --platform "linux/$architecture" \
    --label 'org.opencontainers.image.source=https://github.com/openkaiden/openshell-build-image' \
    --env "OSBI_TEST_${fixture_env}=$variant" \
    --env "OSBI_TEST_VARIANT=$variant" \
    --tag "$(variant_tag "$fixture" "$architecture" "$libc")" "$context"
}

for architecture in amd64 arm64; do
  build_variant single "$architecture" ''
done

platforms_index="$local_prefix-platforms:fixture"
podman manifest create "$platforms_index"
for architecture in amd64 arm64; do
  build_variant platforms "$architecture" ''
  podman manifest add "$platforms_index" \
    "$(variant_tag platforms "$architecture" '')"
done

platforms_libc_index="$local_prefix-platforms-libc:fixture"
podman manifest create "$platforms_libc_index"
for architecture in amd64 arm64; do
  for libc in glibc musl; do
    build_variant platforms-libc "$architecture" "$libc"
    podman manifest add --annotation "$libc_annotation=$libc" \
      "$platforms_libc_index" \
      "$(variant_tag platforms-libc "$architecture" "$libc")"
  done
done

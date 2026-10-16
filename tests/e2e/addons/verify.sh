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

local_prefix=$1
libc_annotation=ai.openkaiden.openshell-build-image.libc

for architecture in amd64 arm64; do
  image="$local_prefix-single-linux-$architecture:fixture"
  actual_platform=$(podman image inspect --format '{{.Os}}/{{.Architecture}}' "$image")
  if [[ $actual_platform != "linux/$architecture" ]]; then
    echo "unexpected platform for $image: $actual_platform" >&2
    exit 1
  fi
done

podman manifest inspect "$local_prefix-platforms:fixture" \
  | jq -e '
      (.manifests | length) == 2 and
      (all(.manifests[]; (.annotations // {}) == {})) and
      ([.manifests[] | [.platform.os, .platform.architecture]] | sort) ==
        [["linux", "amd64"], ["linux", "arm64"]]
    ' > /dev/null

podman manifest inspect "$local_prefix-platforms-libc:fixture" \
  | jq -e --arg key "$libc_annotation" '
      (.manifests | length) == 4 and
      ([.manifests[] |
        [.platform.os, .platform.architecture, .annotations[$key]]] | sort) ==
        [["linux", "amd64", "glibc"],
         ["linux", "amd64", "musl"],
         ["linux", "arm64", "glibc"],
         ["linux", "arm64", "musl"]]
    ' > /dev/null

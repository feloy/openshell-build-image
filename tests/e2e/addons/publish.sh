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

if [[ $# -ne 2 || ! $1 =~ ^[0-9a-f]{40}$ ]]; then
  echo "usage: $0 MAIN_COMMIT_SHA LOCAL_IMAGE_PREFIX" >&2
  exit 2
fi
: "${GHCR_TOKEN:?GHCR_TOKEN is required}"
: "${GITHUB_OUTPUT:?GITHUB_OUTPUT is required}"
: "${GITHUB_STEP_SUMMARY:?GITHUB_STEP_SUMMARY is required}"

source_sha=$1
local_prefix=$2
work_dir=$(mktemp -d "${RUNNER_TEMP:-/tmp}/osbi-addon-publish.XXXXXX")
trap 'rm -rf -- "$work_dir"' EXIT
authfile="$work_dir/ghcr-auth.json"

printf '%s' "$GHCR_TOKEN" | podman login --authfile "$authfile" \
  ghcr.io --username "$GITHUB_ACTOR" --password-stdin

printf '## Addon fixture images\n\n' >> "$GITHUB_STEP_SUMMARY"
for fixture in single-amd64 single-arm64 platforms platforms-libc; do
  case "$fixture" in
    single-*)
      architecture=${fixture#single-}
      source_image="$local_prefix-single-linux-$architecture:fixture"
      image_name=ghcr.io/openkaiden/tests/addon-single
      tag="$architecture-$source_sha"
      ;;
    platforms)
      source_image="$local_prefix-platforms:fixture"
      podman manifest inspect "$source_image" \
        | jq -e '
            (.manifests | length) == 2 and
            (all(.manifests[]; (.annotations // {}) == {})) and
            ([.manifests[] | [.platform.os, .platform.architecture]] | sort) ==
              [["linux", "amd64"], ["linux", "arm64"]]
          ' > /dev/null
      image_name=ghcr.io/openkaiden/tests/addon-platforms
      tag=$source_sha
      ;;
    platforms-libc)
      source_image="$local_prefix-platforms-libc:fixture"
      podman manifest inspect "$source_image" \
        | jq -e --arg key ai.openkaiden.openshell-build-image.libc '
            (.manifests | length) == 4 and
            ([.manifests[] |
              [.platform.os, .platform.architecture, .annotations[$key]]] | sort) ==
              [["linux", "amd64", "glibc"],
               ["linux", "amd64", "musl"],
               ["linux", "arm64", "glibc"],
               ["linux", "arm64", "musl"]]
          ' > /dev/null
      image_name=ghcr.io/openkaiden/tests/addon-platforms-libc
      tag=$source_sha
      ;;
  esac

  destination_ref="$image_name:$tag"
  digestfile="$work_dir/$fixture.digest"
  if [[ $fixture == single-* ]]; then
    podman push --authfile "$authfile" --format oci \
      --digestfile "$digestfile" "$source_image" "docker://$destination_ref"
  else
    podman manifest push --all --authfile "$authfile" --format oci \
      --digestfile "$digestfile" "$source_image" "docker://$destination_ref"
  fi
  digest=$(cat "$digestfile")
  [[ "$digest" =~ ^sha256:[0-9a-f]{64}$ ]]
  published_ref="docker://$image_name@$digest"
  skopeo inspect --authfile "$authfile" --raw "$published_ref" \
    > "$work_dir/$fixture.published.json"

  case "$fixture" in
    single-*)
      jq -e '
        .mediaType == "application/vnd.oci.image.manifest.v1+json" and
        .config.mediaType == "application/vnd.oci.image.config.v1+json"
      ' "$work_dir/$fixture.published.json" > /dev/null
      skopeo inspect --authfile "$authfile" --config "$published_ref" \
        | jq -e --arg architecture "$architecture" \
            '.os == "linux" and .architecture == $architecture' > /dev/null
      ;;
    platforms)
      jq -e '
        .mediaType == "application/vnd.oci.image.index.v1+json" and
        (.manifests | length) == 2 and
        (all(.manifests[];
          .mediaType == "application/vnd.oci.image.manifest.v1+json" and
          (.annotations // {}) == {})) and
        ([.manifests[] | [.platform.os, .platform.architecture]] | sort) ==
          [["linux", "amd64"], ["linux", "arm64"]]
      ' "$work_dir/$fixture.published.json" > /dev/null
      ;;
    platforms-libc)
      jq -e --arg key ai.openkaiden.openshell-build-image.libc '
        .mediaType == "application/vnd.oci.image.index.v1+json" and
        (.manifests | length) == 4 and
        (all(.manifests[];
          .mediaType == "application/vnd.oci.image.manifest.v1+json")) and
        ([.manifests[] |
          [.platform.os, .platform.architecture, .annotations[$key]]] | sort) ==
          [["linux", "amd64", "glibc"],
           ["linux", "amd64", "musl"],
           ["linux", "arm64", "glibc"],
           ["linux", "arm64", "musl"]]
      ' "$work_dir/$fixture.published.json" > /dev/null
      ;;
  esac

  printf '%s_digest=%s\n' "${fixture//-/_}" "$digest" >> "$GITHUB_OUTPUT"
  printf '%s: `%s@%s`\n\n' "$fixture" "$image_name" "$digest" \
    >> "$GITHUB_STEP_SUMMARY"
done

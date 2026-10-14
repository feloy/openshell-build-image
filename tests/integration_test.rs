// Copyright (C) 2026 Red Hat, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

use std::process::{Command, Output};
use std::sync::OnceLock;

#[test]
fn removed_options_are_rejected() {
    let binary = env!("CARGO_BIN_EXE_openshell-build-image");
    for args in [
        vec!["--agent", "claude"],
        vec!["--with-agent-settings"],
        vec!["--inference", "anthropic"],
        vec!["--endpoint", "https://example.com"],
        vec!["--model", "example-model"],
        vec!["--config", "/unused"],
        vec!["--with-workspace-config"],
    ] {
        let output = Command::new(binary)
            .args([
                "--runtime",
                "podman",
                "--from",
                "alpine:3.24",
                "should-not-be-built:test",
            ])
            .args(&args)
            .output()
            .expect("binary should run");
        assert_eq!(output.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("unexpected argument") && stderr.contains(args[0]),
            "expected rejection of {}, got: {stderr}",
            args[0]
        );
    }
}

// Exercise CLI staging with an isolated host HOME and a fake runtime.
#[cfg(unix)]
mod copy_containerfile_cli {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn stages_exact_containerfile_without_modifying_host_home() {
        for enabled in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            // Old config files and their environment variable must not affect --from.
            std::fs::write(dir.path().join("config.toml"), "invalid old config").unwrap();
            std::fs::create_dir(dir.path().join(".kaiden")).unwrap();
            std::fs::write(
                dir.path().join(".kaiden/workspace.json"),
                "invalid workspace json",
            )
            .unwrap();
            let runtime = dir.path().join("podman");
            std::fs::write(
                &runtime,
                r#"#!/bin/sh
/bin/cat "$3" > "$TEST_CAPTURE_FILE"
for context do :; done
if [ -f "$context/build-containerfile" ]; then
    /bin/cp "$context/build-containerfile" "$TEST_STAGED_FILE"
fi
"#,
            )
            .unwrap();
            std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o755)).unwrap();
            let host_copy = dir.path().join("Containerfile");
            std::fs::write(&host_copy, "keep host file").unwrap();
            let used = dir.path().join("used-containerfile");
            let staged = dir.path().join("staged-containerfile");
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_openshell-build-image"));
            cmd.args(["--runtime", "podman", "--from", FEDORA, "test:copy"])
                .current_dir(dir.path())
                .env("HOME", dir.path())
                .env("PATH", dir.path())
                .env("OPENSHELL_BUILD_IMAGE_CONFIG", dir.path())
                .env("TEST_CAPTURE_FILE", &used)
                .env("TEST_STAGED_FILE", &staged);
            if enabled {
                cmd.arg("--copy-containerfile");
            }
            let output = cmd.output().unwrap();
            assert!(output.status.success(), "{:?}", output);
            assert_eq!(
                std::fs::read_to_string(host_copy).unwrap(),
                "keep host file"
            );
            assert!(
                std::fs::read_to_string(&used)
                    .unwrap()
                    .contains(&format!("FROM {FEDORA} AS system"))
            );
            assert_eq!(staged.exists(), enabled);
            if enabled {
                assert_eq!(
                    std::fs::read(&staged).unwrap(),
                    std::fs::read(&used).unwrap()
                );
                assert!(
                    std::fs::read_to_string(used)
                        .unwrap()
                        .contains("COPY build-containerfile /tmp/build-containerfile")
                );
            }
        }
    }
}

mod copy_containerfile {
    use super::*;

    pub(super) static IMAGE: OnceLock<String> = OnceLock::new();

    fn image() -> &'static str {
        IMAGE.get_or_init(|| {
            build_image(
                "openshell-test-copy-containerfile:integration",
                ALPINE,
                &["--copy-containerfile"],
            )
        })
    }

    #[test]
    #[ignore]
    fn containerfile_is_in_image_home() {
        let output = run_in_image(image(), "cat \"$HOME/Containerfile\"");
        assert!(output.status.success(), "{:?}", output);
        let content = String::from_utf8_lossy(&output.stdout);
        assert!(content.contains("FROM docker.io/library/alpine:3.24 AS system"));
        assert!(content.contains("COPY build-containerfile /tmp/build-containerfile"));
    }

    #[test]
    #[ignore]
    fn containerfile_is_owned_by_image_user() {
        let output = run_in_image(
            image(),
            "test \"$(stat -c '%u:%g' \"$HOME/Containerfile\")\" = \"$(id -u):$(id -g)\"",
        );
        assert!(output.status.success(), "{:?}", output);
    }

    #[test]
    #[ignore]
    fn containerfile_is_absent_without_flag() {
        let output = run_in_image(ubuntu_image(), "test ! -e \"$HOME/Containerfile\"");
        assert!(output.status.success(), "{:?}", output);
    }
}

// ---------------------------------------------------------------------------
// Image build helpers
// ---------------------------------------------------------------------------

const UBUNTU: &str = "docker.io/library/ubuntu:24.04";
const FEDORA: &str = "registry.fedoraproject.org/fedora:latest";
const ALPINE: &str = "docker.io/library/alpine:3.24";
const UBI: &str = "registry.access.redhat.com/ubi10/ubi:latest";
const HUMMINGBIRD: &str = "registry.access.redhat.com/hi/core-runtime:latest-builder";

fn build_image(tag: &str, from: &str, extra_args: &[&str]) -> String {
    let binary = env!("CARGO_BIN_EXE_openshell-build-image");
    let status = Command::new(binary)
        .args(["--runtime", "podman", "--from", from])
        .args(extra_args)
        .arg(tag)
        .status()
        .expect("binary should run");
    assert!(status.success(), "image build failed for tag {tag}");
    tag.to_string()
}

fn run_in_image(image: &str, cmd: &str) -> Output {
    Command::new("podman")
        .args(["run", "--rm", "--entrypoint", "/bin/sh", image, "-c", cmd])
        .output()
        .expect("podman run should execute")
}

// ---------------------------------------------------------------------------
// One OnceLock per image variant — each image is built at most once
// ---------------------------------------------------------------------------

static UBUNTU_IMAGE: OnceLock<String> = OnceLock::new();
static ALPINE_IMAGE: OnceLock<String> = OnceLock::new();
static FEDORA_IMAGE: OnceLock<String> = OnceLock::new();
static UBI_IMAGE: OnceLock<String> = OnceLock::new();
static HUMMINGBIRD_IMAGE: OnceLock<String> = OnceLock::new();

fn ubuntu_image() -> &'static str {
    UBUNTU_IMAGE.get_or_init(|| build_image("openshell-test-ubuntu:integration", UBUNTU, &[]))
}

fn alpine_image() -> &'static str {
    ALPINE_IMAGE.get_or_init(|| build_image("openshell-test-alpine:integration", ALPINE, &[]))
}

fn fedora_image() -> &'static str {
    FEDORA_IMAGE.get_or_init(|| build_image("openshell-test-fedora:integration", FEDORA, &[]))
}

fn ubi_image() -> &'static str {
    UBI_IMAGE.get_or_init(|| build_image("openshell-test-ubi:integration", UBI, &[]))
}

fn hummingbird_image() -> &'static str {
    HUMMINGBIRD_IMAGE
        .get_or_init(|| build_image("openshell-test-hummingbird:integration", HUMMINGBIRD, &[]))
}

// ---------------------------------------------------------------------------
// Shared assertion helpers
// ---------------------------------------------------------------------------

fn image_layers(image: &str) -> serde_json::Value {
    let out = Command::new("podman")
        .args([
            "image",
            "inspect",
            "--format",
            "{{json .RootFS.Layers}}",
            image,
        ])
        .output()
        .expect("podman image inspect should execute");
    assert!(out.status.success(), "image inspection failed: {out:?}");
    let layers: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(layers.is_array(), "expected image layers, got: {layers}");
    layers
}

fn check_claude_in_path(image: &str, expected: bool) {
    let out = run_in_image(image, "command -v claude");
    if expected {
        assert!(out.status.success(), "claude not found in PATH");
    } else {
        assert!(!out.status.success(), "claude should not be in PATH");
    }
}

fn check_opencode_in_path(image: &str, expected: bool) {
    let out = run_in_image(image, "command -v opencode");
    if expected {
        assert!(out.status.success(), "opencode not found in PATH");
    } else {
        assert!(!out.status.success(), "opencode should not be in PATH");
    }
}

// ---------------------------------------------------------------------------
// Base images — one test module per variant
// ---------------------------------------------------------------------------

macro_rules! image_tests {
    ($mod_name:ident, $image_fn:ident, $base_image:expr) => {
        mod $mod_name {
            use super::*;

            #[test]
            #[ignore]
            fn base_image_layers_are_unchanged() {
                assert_eq!(image_layers($image_fn()), image_layers($base_image));
            }

            #[test]
            #[ignore]
            fn claude_is_not_installed() {
                check_claude_in_path($image_fn(), false);
            }

            #[test]
            #[ignore]
            fn opencode_is_not_installed() {
                check_opencode_in_path($image_fn(), false);
            }
        }
    };
}

image_tests!(ubuntu, ubuntu_image, UBUNTU);
image_tests!(alpine, alpine_image, ALPINE);
image_tests!(
    fedora,
    fedora_image,
    "registry.fedoraproject.org/fedora:latest"
);
image_tests!(
    ubi,
    ubi_image,
    "registry.access.redhat.com/ubi10/ubi:latest"
);
image_tests!(
    hummingbird,
    hummingbird_image,
    "registry.access.redhat.com/hi/core-runtime:latest-builder"
);

// ---------------------------------------------------------------------------
// Host certificates are managed by OpenShell
// ---------------------------------------------------------------------------

mod host_certificates {
    use super::*;

    #[test]
    #[ignore]
    fn ubuntu_host_cert_absent() {
        let out = run_in_image(
            ubuntu_image(),
            "test ! -e /usr/local/share/ca-certificates/system-ca.crt",
        );
        assert!(
            out.status.success(),
            "host CA bundle should not be in the image"
        );
    }

    #[test]
    #[ignore]
    fn fedora_host_cert_absent() {
        let out = run_in_image(
            fedora_image(),
            "test ! -e /etc/pki/ca-trust/source/anchors/system-ca.crt",
        );
        assert!(
            out.status.success(),
            "host CA bundle should not be in the image"
        );
    }
}

// ---------------------------------------------------------------------------
// Cleanup — runs when the test process exits, after all tests complete
// ---------------------------------------------------------------------------

#[ctor::dtor]
fn cleanup_images() {
    // Only remove images built by this test process.
    for image in [
        &copy_containerfile::IMAGE,
        &UBUNTU_IMAGE,
        &FEDORA_IMAGE,
        &ALPINE_IMAGE,
        &UBI_IMAGE,
        &HUMMINGBIRD_IMAGE,
    ] {
        if let Some(tag) = image.get() {
            Command::new("podman")
                .args(["rmi", "--force", tag])
                .status()
                .ok();
        }
    }
}

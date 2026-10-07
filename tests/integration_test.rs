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
fn removed_agent_options_are_rejected() {
    let binary = env!("CARGO_BIN_EXE_openshell-build-image");
    for args in [
        vec!["--agent", "claude"],
        vec!["--with-agent-settings"],
        vec!["--inference", "anthropic"],
        vec!["--endpoint", "https://example.com"],
        vec!["--model", "example-model"],
    ] {
        let output = Command::new(binary)
            .args(["--runtime", "podman", "should-not-be-built:test"])
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
            let config = fedora_config_dir();
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
            cmd.args(["--runtime", "podman", "test:copy"])
                .arg("--config")
                .arg(config.path())
                .current_dir(dir.path())
                .env("HOME", dir.path())
                .env("PATH", dir.path())
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

    static IMAGE: OnceLock<String> = OnceLock::new();

    fn image() -> &'static str {
        IMAGE.get_or_init(|| {
            build_image(
                "openshell-test-copy-containerfile:integration",
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
        assert!(content.contains("FROM docker.io/library/ubuntu:24.04 AS system"));
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

fn fedora_config_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config.toml"),
        "[openshell_build_image.base_image]\nimage = \"fedora\"\ntag = \"latest\"\n",
    )
    .unwrap();
    dir
}

fn ubi_config_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config.toml"),
        "[openshell_build_image.base_image]\nimage = \"ubi\"\ntag = \"latest\"\n",
    )
    .unwrap();
    dir
}

fn hummingbird_config_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config.toml"),
        "[openshell_build_image.base_image]\nimage = \"hummingbird\"\ntag = \"latest-builder\"\n",
    )
    .unwrap();
    dir
}

fn build_image(tag: &str, extra_args: &[&str]) -> String {
    let binary = env!("CARGO_BIN_EXE_openshell-build-image");
    let status = Command::new(binary)
        .args(["--runtime", "podman"])
        .args(extra_args)
        .arg(tag)
        .status()
        .expect("binary should run");
    assert!(status.success(), "image build failed for tag {tag}");
    tag.to_string()
}

fn run_in_image(image: &str, cmd: &str) -> Output {
    Command::new("podman")
        .args(["run", "--rm", "--entrypoint", "/bin/bash", image, "-c", cmd])
        .output()
        .expect("podman run should execute")
}

// ---------------------------------------------------------------------------
// One OnceLock per image variant — each image is built at most once
// ---------------------------------------------------------------------------

static UBUNTU_IMAGE: OnceLock<String> = OnceLock::new();
static FEDORA_IMAGE: OnceLock<String> = OnceLock::new();
static UBI_IMAGE: OnceLock<String> = OnceLock::new();
static HUMMINGBIRD_IMAGE: OnceLock<String> = OnceLock::new();
static NO_WORKSPACE_CONFIG_OCI_FEATURE_UBUNTU_IMAGE: OnceLock<String> = OnceLock::new();
static NO_WORKSPACE_CONFIG_LOCAL_FEATURE_UBUNTU_IMAGE: OnceLock<String> = OnceLock::new();
static NO_WORKSPACE_CONFIG_NETWORK_HOSTS_UBUNTU_IMAGE: OnceLock<String> = OnceLock::new();
static UBUNTU_NO_POLICY_IMAGE: OnceLock<String> = OnceLock::new();

fn ubuntu_image() -> &'static str {
    UBUNTU_IMAGE
        .get_or_init(|| build_image("openshell-test-ubuntu:integration", &["--with-policy"]))
}

fn fedora_image() -> &'static str {
    FEDORA_IMAGE.get_or_init(|| {
        let config = fedora_config_dir();
        build_image(
            "openshell-test-fedora:integration",
            &["--config", config.path().to_str().unwrap(), "--with-policy"],
        )
    })
}

fn ubi_image() -> &'static str {
    UBI_IMAGE.get_or_init(|| {
        let config = ubi_config_dir();
        build_image(
            "openshell-test-ubi:integration",
            &["--config", config.path().to_str().unwrap(), "--with-policy"],
        )
    })
}

fn hummingbird_image() -> &'static str {
    HUMMINGBIRD_IMAGE.get_or_init(|| {
        let config = hummingbird_config_dir();
        build_image(
            "openshell-test-hummingbird:integration",
            &["--config", config.path().to_str().unwrap(), "--with-policy"],
        )
    })
}

// ---------------------------------------------------------------------------
// Shared assertion helpers
// ---------------------------------------------------------------------------

fn check_packages(image: &str) {
    for pkg in ["curl", "ip", "tar"] {
        let out = run_in_image(image, &format!("which {pkg}"));
        assert!(out.status.success(), "{pkg} not found in image");
    }
}

fn check_policy_yaml(image: &str) {
    let out = run_in_image(image, "test -f /etc/openshell/policy.yaml");
    assert!(
        out.status.success(),
        "policy.yaml not found in /etc/openshell/"
    );
}

fn check_claude_in_path(image: &str, expected: bool) {
    let out = run_in_image(image, "which claude");
    if expected {
        assert!(out.status.success(), "claude not found in PATH");
    } else {
        assert!(!out.status.success(), "claude should not be in PATH");
    }
}

fn check_opencode_in_path(image: &str, expected: bool) {
    let out = run_in_image(image, "which opencode");
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
    ($mod_name:ident, $image_fn:ident) => {
        mod $mod_name {
            use super::*;

            #[test]
            #[ignore]
            fn packages_installed() {
                check_packages($image_fn());
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

            #[test]
            #[ignore]
            fn policy_yaml_present() {
                check_policy_yaml($image_fn());
            }
        }
    };
}

image_tests!(ubuntu, ubuntu_image);
image_tests!(fedora, fedora_image);
image_tests!(ubi, ubi_image);
image_tests!(hummingbird, hummingbird_image);

// ---------------------------------------------------------------------------
// Workspace helpers for feature-based builds
// ---------------------------------------------------------------------------

fn workspace_dir(workspace_json: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let kaiden = dir.path().join(".kaiden");
    std::fs::create_dir_all(&kaiden).unwrap();
    std::fs::write(kaiden.join("workspace.json"), workspace_json).unwrap();
    dir
}

fn build_image_with_workspace(tag: &str, workspace_json: &str, extra_args: &[&str]) -> String {
    let dir = workspace_dir(workspace_json);
    let binary = env!("CARGO_BIN_EXE_openshell-build-image");
    let status = Command::new(binary)
        .current_dir(dir.path())
        .args(["--runtime", "podman"])
        .arg("--with-workspace-config")
        .args(extra_args)
        .arg(tag)
        .status()
        .expect("binary should run");
    assert!(status.success(), "image build failed for tag {tag}");
    tag.to_string()
}

// ---------------------------------------------------------------------------
// Feature image singletons — ubuntu (default) and fedora variants
// ---------------------------------------------------------------------------

static FEATURE_COMMON_UTILS_UBUNTU_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_NODE_UBUNTU_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_PYTHON_UBUNTU_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_COMMON_UTILS_FEDORA_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_NODE_FEDORA_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_PYTHON_FEDORA_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_COMMON_UTILS_UBI_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_NODE_UBI_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_PYTHON_UBI_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_LOCAL_UBUNTU_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_LOCAL_FEDORA_IMAGE: OnceLock<String> = OnceLock::new();
static FEATURE_LOCAL_UBI_IMAGE: OnceLock<String> = OnceLock::new();

const COMMON_UTILS_WORKSPACE: &str = r#"{
    "features": {
        "ghcr.io/devcontainers/features/common-utils:2": {
            "installZsh": true
        }
    }
}"#;

const NODE_WORKSPACE: &str = r#"{
    "features": {
        "ghcr.io/devcontainers/features/node:1": {
            "version": "22"
        }
    }
}"#;

const PYTHON_WORKSPACE: &str = r#"{
    "features": {
        "ghcr.io/devcontainers/features/python:1": {
            "version": "os-provided",
            "installTools": true
        }
    }
}"#;

const NETWORK_HOSTS_WORKSPACE: &str = r#"{
    "network": {
        "hosts": ["example.com"]
    }
}"#;

fn feature_common_utils_ubuntu_image() -> &'static str {
    FEATURE_COMMON_UTILS_UBUNTU_IMAGE.get_or_init(|| {
        build_image_with_workspace(
            "openshell-test-feature-common-utils-ubuntu:integration",
            COMMON_UTILS_WORKSPACE,
            &[],
        )
    })
}

fn feature_node_ubuntu_image() -> &'static str {
    FEATURE_NODE_UBUNTU_IMAGE.get_or_init(|| {
        build_image_with_workspace(
            "openshell-test-feature-node-ubuntu:integration",
            NODE_WORKSPACE,
            &[],
        )
    })
}

fn feature_python_ubuntu_image() -> &'static str {
    FEATURE_PYTHON_UBUNTU_IMAGE.get_or_init(|| {
        build_image_with_workspace(
            "openshell-test-feature-python-ubuntu:integration",
            PYTHON_WORKSPACE,
            &[],
        )
    })
}

fn feature_common_utils_fedora_image() -> &'static str {
    FEATURE_COMMON_UTILS_FEDORA_IMAGE.get_or_init(|| {
        let config = fedora_config_dir();
        build_image_with_workspace(
            "openshell-test-feature-common-utils-fedora:integration",
            COMMON_UTILS_WORKSPACE,
            &["--config", config.path().to_str().unwrap()],
        )
    })
}

fn feature_node_fedora_image() -> &'static str {
    FEATURE_NODE_FEDORA_IMAGE.get_or_init(|| {
        let config = fedora_config_dir();
        build_image_with_workspace(
            "openshell-test-feature-node-fedora:integration",
            NODE_WORKSPACE,
            &["--config", config.path().to_str().unwrap()],
        )
    })
}

fn feature_python_fedora_image() -> &'static str {
    FEATURE_PYTHON_FEDORA_IMAGE.get_or_init(|| {
        let config = fedora_config_dir();
        build_image_with_workspace(
            "openshell-test-feature-python-fedora:integration",
            PYTHON_WORKSPACE,
            &["--config", config.path().to_str().unwrap()],
        )
    })
}

fn feature_common_utils_ubi_image() -> &'static str {
    FEATURE_COMMON_UTILS_UBI_IMAGE.get_or_init(|| {
        let config = ubi_config_dir();
        build_image_with_workspace(
            "openshell-test-feature-common-utils-ubi:integration",
            COMMON_UTILS_WORKSPACE,
            &["--config", config.path().to_str().unwrap()],
        )
    })
}

fn feature_node_ubi_image() -> &'static str {
    FEATURE_NODE_UBI_IMAGE.get_or_init(|| {
        let config = ubi_config_dir();
        build_image_with_workspace(
            "openshell-test-feature-node-ubi:integration",
            NODE_WORKSPACE,
            &["--config", config.path().to_str().unwrap()],
        )
    })
}

fn feature_python_ubi_image() -> &'static str {
    FEATURE_PYTHON_UBI_IMAGE.get_or_init(|| {
        let config = ubi_config_dir();
        build_image_with_workspace(
            "openshell-test-feature-python-ubi:integration",
            PYTHON_WORKSPACE,
            &["--config", config.path().to_str().unwrap()],
        )
    })
}

fn local_feature_workspace_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let kaiden = dir.path().join(".kaiden");
    let feature_dir = kaiden.join("my-feature");
    std::fs::create_dir_all(&feature_dir).unwrap();
    std::fs::write(
        kaiden.join("workspace.json"),
        r#"{"features": {"./my-feature": {"filename": "hello-from-feature"}}}"#,
    )
    .unwrap();
    std::fs::write(
        feature_dir.join("devcontainer-feature.json"),
        r#"{"id": "my-feature", "version": "1.0.0", "name": "My Test Feature", "options": {"filename": {"type": "string", "default": "default-filename"}}}"#,
    )
    .unwrap();
    std::fs::write(
        feature_dir.join("install.sh"),
        "#!/bin/sh\nsh \"$(dirname \"$0\")/main.sh\"\n",
    )
    .unwrap();
    std::fs::write(
        feature_dir.join("main.sh"),
        "#!/bin/sh\ntouch \"$_REMOTE_USER_HOME/$FILENAME\"\n",
    )
    .unwrap();
    dir
}

fn build_image_with_local_feature(tag: &str, extra_args: &[&str]) -> String {
    let dir = local_feature_workspace_dir();
    let binary = env!("CARGO_BIN_EXE_openshell-build-image");
    let status = Command::new(binary)
        .current_dir(dir.path())
        .args(["--runtime", "podman"])
        .arg("--with-workspace-config")
        .args(extra_args)
        .arg(tag)
        .status()
        .expect("binary should run");
    assert!(status.success(), "image build failed for tag {tag}");
    tag.to_string()
}

fn feature_local_ubuntu_image() -> &'static str {
    FEATURE_LOCAL_UBUNTU_IMAGE.get_or_init(|| {
        build_image_with_local_feature("openshell-test-feature-local-ubuntu:integration", &[])
    })
}

fn feature_local_fedora_image() -> &'static str {
    FEATURE_LOCAL_FEDORA_IMAGE.get_or_init(|| {
        let config = fedora_config_dir();
        build_image_with_local_feature(
            "openshell-test-feature-local-fedora:integration",
            &["--config", config.path().to_str().unwrap()],
        )
    })
}

fn feature_local_ubi_image() -> &'static str {
    FEATURE_LOCAL_UBI_IMAGE.get_or_init(|| {
        let config = ubi_config_dir();
        build_image_with_local_feature(
            "openshell-test-feature-local-ubi:integration",
            &["--config", config.path().to_str().unwrap()],
        )
    })
}

// ---------------------------------------------------------------------------
// Helpers and singletons for --with-workspace-config absence tests
// ---------------------------------------------------------------------------

/// Like build_image_with_workspace but WITHOUT --with-workspace-config, so the
/// workspace file is present on disk but deliberately ignored by the tool.
fn build_image_in_workspace_dir(tag: &str, workspace_json: &str, extra_args: &[&str]) -> String {
    let dir = workspace_dir(workspace_json);
    let binary = env!("CARGO_BIN_EXE_openshell-build-image");
    let status = Command::new(binary)
        .current_dir(dir.path())
        .args(["--runtime", "podman"])
        .args(extra_args)
        .arg(tag)
        .status()
        .expect("binary should run");
    assert!(status.success(), "image build failed for tag {tag}");
    tag.to_string()
}

fn no_workspace_config_oci_feature_ubuntu_image() -> &'static str {
    NO_WORKSPACE_CONFIG_OCI_FEATURE_UBUNTU_IMAGE.get_or_init(|| {
        build_image_in_workspace_dir(
            "openshell-test-no-workspace-config-oci-feature-ubuntu:integration",
            COMMON_UTILS_WORKSPACE,
            &[],
        )
    })
}

fn no_workspace_config_local_feature_ubuntu_image() -> &'static str {
    NO_WORKSPACE_CONFIG_LOCAL_FEATURE_UBUNTU_IMAGE.get_or_init(|| {
        let dir = local_feature_workspace_dir();
        let binary = env!("CARGO_BIN_EXE_openshell-build-image");
        let status = Command::new(binary)
            .current_dir(dir.path())
            .args(["--runtime", "podman"])
            .arg("openshell-test-no-workspace-config-local-feature-ubuntu:integration")
            .status()
            .expect("binary should run");
        assert!(status.success(), "image build failed");
        "openshell-test-no-workspace-config-local-feature-ubuntu:integration".to_string()
    })
}

fn no_workspace_config_network_hosts_ubuntu_image() -> &'static str {
    NO_WORKSPACE_CONFIG_NETWORK_HOSTS_UBUNTU_IMAGE.get_or_init(|| {
        build_image_in_workspace_dir(
            "openshell-test-no-workspace-config-network-hosts-ubuntu:integration",
            NETWORK_HOSTS_WORKSPACE,
            &["--with-policy"],
        )
    })
}

fn ubuntu_no_policy_image() -> &'static str {
    UBUNTU_NO_POLICY_IMAGE
        .get_or_init(|| build_image("openshell-test-ubuntu-no-policy:integration", &[]))
}

// ---------------------------------------------------------------------------
// Feature integration tests — one macro per feature, instantiated per base image
// ---------------------------------------------------------------------------

macro_rules! feature_common_utils_tests {
    ($mod_name:ident, $image_fn:ident, $base_image_fn:ident) => {
        mod $mod_name {
            use super::*;

            #[test]
            #[ignore]
            fn zsh_installed() {
                let out = run_in_image($image_fn(), "which zsh");
                assert!(out.status.success(), "zsh not found in image");
            }

            #[test]
            #[ignore]
            fn zsh_not_in_base_image() {
                let out = run_in_image($base_image_fn(), "which zsh");
                assert!(!out.status.success(), "zsh should not be in base image");
            }
        }
    };
}

macro_rules! feature_node_tests {
    ($mod_name:ident, $image_fn:ident, $base_image_fn:ident) => {
        mod $mod_name {
            use super::*;

            #[test]
            #[ignore]
            fn node_in_path() {
                let out = run_in_image($image_fn(), "node --version");
                assert!(out.status.success(), "node not found in PATH");
            }

            #[test]
            #[ignore]
            fn node_not_in_base_image() {
                let out = run_in_image($base_image_fn(), "which node");
                assert!(!out.status.success(), "node should not be in base image");
            }

            #[test]
            #[ignore]
            fn npm_in_path() {
                let out = run_in_image($image_fn(), "npm --version");
                assert!(out.status.success(), "npm not found in PATH");
            }

            #[test]
            #[ignore]
            fn npm_not_in_base_image() {
                let out = run_in_image($base_image_fn(), "which npm");
                assert!(!out.status.success(), "npm should not be in base image");
            }
        }
    };
}

macro_rules! feature_python_tests {
    ($mod_name:ident, $image_fn:ident, $base_image_fn:ident) => {
        mod $mod_name {
            use super::*;

            #[test]
            #[ignore]
            fn python3_available() {
                let out = run_in_image($image_fn(), "python3 --version");
                assert!(out.status.success(), "python3 not found in image");
            }

            #[test]
            #[ignore]
            fn flake8_installed() {
                let out = run_in_image($image_fn(), "which flake8");
                assert!(out.status.success(), "flake8 not found in PATH");
            }

            #[test]
            #[ignore]
            fn flake8_not_in_base_image() {
                let out = run_in_image($base_image_fn(), "which flake8");
                assert!(!out.status.success(), "flake8 should not be in base image");
            }

            #[test]
            #[ignore]
            fn pylint_installed() {
                let out = run_in_image($image_fn(), "which pylint");
                assert!(out.status.success(), "pylint not found in PATH");
            }

            #[test]
            #[ignore]
            fn pylint_not_in_base_image() {
                let out = run_in_image($base_image_fn(), "which pylint");
                assert!(!out.status.success(), "pylint should not be in base image");
            }
        }
    };
}

macro_rules! feature_local_tests {
    ($mod_name:ident, $image_fn:ident, $base_image_fn:ident) => {
        mod $mod_name {
            use super::*;

            #[test]
            #[ignore]
            fn file_created_by_feature() {
                let out = run_in_image($image_fn(), "test -f /sandbox/hello-from-feature");
                assert!(out.status.success(), "file not created by local feature");
            }

            #[test]
            #[ignore]
            fn file_not_in_base_image() {
                let out = run_in_image($base_image_fn(), "test -f /sandbox/hello-from-feature");
                assert!(!out.status.success(), "file should not exist in base image");
            }
        }
    };
}

feature_local_tests!(
    feature_local_ubuntu,
    feature_local_ubuntu_image,
    ubuntu_image
);
feature_local_tests!(
    feature_local_fedora,
    feature_local_fedora_image,
    fedora_image
);
feature_local_tests!(feature_local_ubi, feature_local_ubi_image, ubi_image);

feature_common_utils_tests!(
    feature_common_utils_ubuntu,
    feature_common_utils_ubuntu_image,
    ubuntu_image
);
feature_common_utils_tests!(
    feature_common_utils_fedora,
    feature_common_utils_fedora_image,
    fedora_image
);
feature_common_utils_tests!(
    feature_common_utils_ubi,
    feature_common_utils_ubi_image,
    ubi_image
);
feature_node_tests!(feature_node_ubuntu, feature_node_ubuntu_image, ubuntu_image);
feature_node_tests!(feature_node_fedora, feature_node_fedora_image, fedora_image);
feature_node_tests!(feature_node_ubi, feature_node_ubi_image, ubi_image);
feature_python_tests!(
    feature_python_ubuntu,
    feature_python_ubuntu_image,
    ubuntu_image
);
feature_python_tests!(
    feature_python_fedora,
    feature_python_fedora_image,
    fedora_image
);
feature_python_tests!(feature_python_ubi, feature_python_ubi_image, ubi_image);

// ---------------------------------------------------------------------------
// Tests: workspace content excluded when --with-workspace-config is absent
// ---------------------------------------------------------------------------

mod without_workspace_config {
    use super::*;

    #[test]
    #[ignore]
    fn oci_feature_not_installed() {
        // COMMON_UTILS_WORKSPACE declares common-utils which installs zsh.
        // Without --with-workspace-config the workspace file must be ignored.
        let out = run_in_image(no_workspace_config_oci_feature_ubuntu_image(), "which zsh");
        assert!(
            !out.status.success(),
            "zsh should not be installed when --with-workspace-config is absent"
        );
    }

    #[test]
    #[ignore]
    fn local_feature_not_applied() {
        // The local feature workspace creates /sandbox/hello-from-feature.
        // Without --with-workspace-config that file must not exist.
        let out = run_in_image(
            no_workspace_config_local_feature_ubuntu_image(),
            "test -f /sandbox/hello-from-feature",
        );
        assert!(
            !out.status.success(),
            "local feature file should not exist when --with-workspace-config is absent"
        );
    }

    #[test]
    #[ignore]
    fn network_hosts_not_in_policy() {
        let out = run_in_image(
            no_workspace_config_network_hosts_ubuntu_image(),
            "cat /etc/openshell/policy.yaml",
        );
        assert!(out.status.success(), "failed to read policy.yaml");
        let policy = String::from_utf8_lossy(&out.stdout);
        assert!(
            !policy.contains("name: workspace"),
            "workspace network rule should not be present when --with-workspace-config is absent"
        );
    }
}

// ---------------------------------------------------------------------------
// --with-policy flag tests
// ---------------------------------------------------------------------------

mod with_policy {
    use super::*;

    #[test]
    #[ignore]
    fn policy_yaml_present_when_flag_set() {
        let out = run_in_image(ubuntu_image(), "test -f /etc/openshell/policy.yaml");
        assert!(
            out.status.success(),
            "policy.yaml not found in /etc/openshell/ when --with-policy was passed"
        );
    }

    #[test]
    #[ignore]
    fn policy_yaml_absent_without_flag() {
        let out = run_in_image(
            ubuntu_no_policy_image(),
            "test ! -f /etc/openshell/policy.yaml",
        );
        assert!(
            out.status.success(),
            "policy.yaml found in /etc/openshell/ even though --with-policy was not passed"
        );
    }
}

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
    for tag in [
        "openshell-test-copy-containerfile:integration",
        "openshell-test-ubuntu:integration",
        "openshell-test-fedora:integration",
        "openshell-test-ubi:integration",
        "openshell-test-feature-common-utils-ubuntu:integration",
        "openshell-test-feature-node-ubuntu:integration",
        "openshell-test-feature-python-ubuntu:integration",
        "openshell-test-feature-common-utils-fedora:integration",
        "openshell-test-feature-node-fedora:integration",
        "openshell-test-feature-python-fedora:integration",
        "openshell-test-feature-common-utils-ubi:integration",
        "openshell-test-feature-node-ubi:integration",
        "openshell-test-feature-python-ubi:integration",
        "openshell-test-feature-local-ubuntu:integration",
        "openshell-test-feature-local-fedora:integration",
        "openshell-test-feature-local-ubi:integration",
        "openshell-test-no-workspace-config-oci-feature-ubuntu:integration",
        "openshell-test-no-workspace-config-local-feature-ubuntu:integration",
        "openshell-test-no-workspace-config-network-hosts-ubuntu:integration",
        "openshell-test-ubuntu-no-policy:integration",
    ] {
        Command::new("podman")
            .args(["rmi", "--force", tag])
            .status()
            .ok();
    }
}

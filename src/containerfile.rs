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

use crate::config::Config;
use crate::feature::StagedFeature;

#[derive(Debug, PartialEq)]
pub enum ContainerfileError {
    NotSupported { image: String },
}

impl std::fmt::Display for ContainerfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerfileError::NotSupported { image } => {
                write!(f, "base image '{image}' is not supported")
            }
        }
    }
}

impl std::error::Error for ContainerfileError {}

pub fn generate(
    config: &Config,
    features: &[StagedFeature],
    with_policy: bool,
    copy_containerfile: bool,
) -> Result<String, ContainerfileError> {
    let tag = &config.base_image.tag;
    let system_stage = match config.base_image.image.as_str() {
        "fedora" => dnf_system_stage(
            "registry.fedoraproject.org/fedora",
            tag,
            &[
                "bind-utils",
                "ca-certificates",
                "curl",
                "iproute",
                "iptables",
                "iputils",
                "net-tools",
                "nftables",
                "nmap-ncat",
                "openssh-server",
                "procps-ng",
                "traceroute",
                "which",
            ],
        ),
        "ubi" => dnf_system_stage(
            "registry.access.redhat.com/ubi10/ubi",
            tag,
            &[
                "bind-utils",
                "ca-certificates",
                "iputils",
                "net-tools",
                "nftables",
                "nmap-ncat",
                "openssh-server",
                "procps-ng",
                "which",
            ],
        ),
        "hummingbird" => dnf_system_stage(
            "registry.access.redhat.com/hi/core-runtime",
            tag,
            &[
                "bind-utils",
                "iproute",
                "openssh-server",
                "procps-ng",
                "which",
                "tar",
            ],
        ),
        "ubuntu" => ubuntu_system_stage(tag),
        image => {
            return Err(ContainerfileError::NotSupported {
                image: image.to_string(),
            });
        }
    };
    Ok(format!(
        "{system_stage}\n{}",
        final_stage(features, with_policy, copy_containerfile)
    ))
}

/// Renders the feature installation section for the `final` stage.
///
/// Each feature's files are COPYed from the build context into the image, then
/// install.sh is run with options passed as env var assignments. `_REMOTE_USER`
/// and `_REMOTE_USER_HOME` are set before the block so scripts can resolve the
/// target user. Each feature's `containerEnv` is set immediately after its
/// install so subsequent features can reference it.
fn features_section(features: &[StagedFeature]) -> String {
    if features.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str("ENV _REMOTE_USER=\"root\"\n");
    out.push_str("ENV _REMOTE_USER_HOME=\"/sandbox\"\n");
    out.push_str("RUN mkdir -p \"$_REMOTE_USER_HOME\"\n");

    for feature in features {
        out.push('\n');
        out.push_str(&format!("# Feature: {}\n", feature.id));

        let install_dir = format!("/tmp/feature-install/{}", feature.dir_name);
        out.push_str(&format!(
            "COPY features/{}/ {install_dir}/\n",
            feature.dir_name
        ));

        // Build sorted option assignments: VAR="value" (embedded " escaped).
        let mut opt_pairs: Vec<(&String, &String)> = feature.merged_options.iter().collect();
        opt_pairs.sort_by_key(|(k, _)| k.as_str());
        let opts_prefix = if opt_pairs.is_empty() {
            String::new()
        } else {
            let opts = opt_pairs
                .iter()
                .map(|(k, v)| format!("{}=\"{}\"", k, v.replace('"', "\\\"")))
                .collect::<Vec<_>>()
                .join(" ");
            format!("{opts} ")
        };

        out.push_str(&format!(
            "RUN chmod +x {install_dir}/install.sh && \\\n    {opts_prefix}{install_dir}/install.sh\n"
        ));

        // containerEnv: one ENV per variable, sorted, double-quoted.
        if !feature.container_env.is_empty() {
            let mut env_pairs: Vec<(&String, &String)> = feature.container_env.iter().collect();
            env_pairs.sort_by_key(|(k, _)| k.as_str());
            for (k, v) in env_pairs {
                let escaped = v.replace('"', "\\\"");
                out.push_str(&format!("ENV {k}=\"{escaped}\"\n"));
            }
        }
    }
    out.push_str("RUN rm -rf /tmp/feature-install\n");
    out.push('\n');
    out
}

fn ubuntu_system_stage(tag: &str) -> String {
    format!(
        r#"# System base
FROM docker.io/library/ubuntu:{tag} AS system

# Core system dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        dnsutils \
        iproute2 \
        iptables \
        nftables \
        iputils-ping \
        net-tools \
        netcat-openbsd \
        openssh-sftp-server \
        procps \
        traceroute \
    && rm -rf /var/lib/apt/lists/*

"#
    )
}

fn dnf_system_stage(base_image: &str, tag: &str, packages: &[&str]) -> String {
    let pkg_lines = packages
        .iter()
        .map(|p| format!("        {p} \\"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"# System base
FROM {base_image}:{tag} AS system

# Core system dependencies
USER 0
RUN dnf install -y --setopt=install_weak_deps=False \
{pkg_lines}
    && dnf clean all

"#
    )
}

fn final_stage(features: &[StagedFeature], with_policy: bool, copy_containerfile: bool) -> String {
    let containerfile_section = if copy_containerfile {
        "COPY build-containerfile /tmp/build-containerfile\n\
         RUN cp /tmp/build-containerfile \"$HOME/Containerfile\" && rm /tmp/build-containerfile\n\n"
    } else {
        ""
    };
    let features_section = features_section(features);
    let policy_section = if with_policy {
        "COPY policy.yaml /etc/openshell/policy.yaml\n\n"
    } else {
        ""
    };
    format!(
        r#"# Final base image
FROM system AS final

{features_section}{policy_section}{containerfile_section}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BaseImageConfig, Config};

    fn build_cf(
        config: &Config,
        features: &[StagedFeature],
        with_policy: bool,
    ) -> Result<String, ContainerfileError> {
        generate(config, features, with_policy, false)
    }

    fn ubuntu_config(tag: &str) -> Config {
        Config {
            version: 1,
            base_image: BaseImageConfig {
                image: "ubuntu".to_string(),
                tag: tag.to_string(),
            },
        }
    }

    fn fedora_config() -> Config {
        Config {
            version: 1,
            base_image: BaseImageConfig {
                image: "fedora".to_string(),
                tag: "latest".to_string(),
            },
        }
    }

    fn ubi_config() -> Config {
        Config {
            version: 1,
            base_image: BaseImageConfig {
                image: "ubi".to_string(),
                tag: "latest".to_string(),
            },
        }
    }

    fn hummingbird_config() -> Config {
        Config {
            version: 1,
            base_image: BaseImageConfig {
                image: "hummingbird".to_string(),
                tag: "latest-builder".to_string(),
            },
        }
    }

    fn mock_feature(id: &str, dir_name: &str) -> StagedFeature {
        StagedFeature {
            id: id.to_string(),
            dir_name: dir_name.to_string(),
            merged_options: std::collections::HashMap::new(),
            container_env: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn ubuntu_generates_successfully() {
        let config = ubuntu_config("noble-20251013");
        assert!(build_cf(&config, &[], false).is_ok());
    }

    #[test]
    fn ubuntu_containerfile_contains_tag() {
        let config = ubuntu_config("noble-20251013");
        let content = build_cf(&config, &[], false).unwrap();

        assert!(content.contains("FROM docker.io/library/ubuntu:noble-20251013 AS system"));
    }

    #[test]
    fn ubuntu_containerfile_tag_is_substituted() {
        let content = build_cf(&ubuntu_config("24.04"), &[], false).unwrap();

        assert!(content.contains("FROM docker.io/library/ubuntu:24.04 AS system"));
        assert!(!content.contains("{tag}"));
    }

    #[test]
    fn fedora_generates_successfully() {
        assert!(build_cf(&fedora_config(), &[], false).is_ok());
    }

    #[test]
    fn fedora_containerfile_contains_tag() {
        let content = build_cf(&fedora_config(), &[], false).unwrap();

        assert!(content.contains("FROM registry.fedoraproject.org/fedora:latest AS system"));
    }

    #[test]
    fn fedora_containerfile_tag_is_substituted() {
        let content = build_cf(&fedora_config(), &[], false).unwrap();

        assert!(!content.contains("{tag}"));
    }

    #[test]
    fn ubi_generates_successfully() {
        assert!(build_cf(&ubi_config(), &[], false).is_ok());
    }

    #[test]
    fn ubi_containerfile_contains_tag() {
        let content = build_cf(&ubi_config(), &[], false).unwrap();

        assert!(content.contains("FROM registry.access.redhat.com/ubi10/ubi:latest AS system"));
    }

    #[test]
    fn ubi_containerfile_tag_is_substituted() {
        let content = build_cf(&ubi_config(), &[], false).unwrap();

        assert!(!content.contains("{tag}"));
    }

    #[test]
    fn ubi_copies_policy_yaml() {
        let content = build_cf(&ubi_config(), &[], true).unwrap();
        assert!(content.contains("COPY policy.yaml /etc/openshell/policy.yaml"));
    }

    #[test]
    fn hummingbird_generates_successfully() {
        assert!(build_cf(&hummingbird_config(), &[], false).is_ok());
    }

    #[test]
    fn hummingbird_containerfile_contains_tag() {
        let content = build_cf(&hummingbird_config(), &[], false).unwrap();

        assert!(
            content.contains(
                "FROM registry.access.redhat.com/hi/core-runtime:latest-builder AS system"
            )
        );
    }

    #[test]
    fn hummingbird_containerfile_tag_is_substituted() {
        let content = build_cf(&hummingbird_config(), &[], false).unwrap();

        assert!(!content.contains("{tag}"));
    }

    #[test]
    fn hummingbird_copies_policy_yaml() {
        let content = build_cf(&hummingbird_config(), &[], true).unwrap();
        assert!(content.contains("COPY policy.yaml /etc/openshell/policy.yaml"));
    }

    #[test]
    fn hummingbird_containerfile_includes_iproute() {
        let content = build_cf(&hummingbird_config(), &[], false).unwrap();

        assert!(
            content.contains("iproute"),
            "hummingbird image must install iproute for network namespace support"
        );
    }

    #[test]
    fn not_supported_error_message() {
        let err = ContainerfileError::NotSupported {
            image: "centos".to_string(),
        };
        assert_eq!(err.to_string(), "base image 'centos' is not supported");
    }

    #[test]
    fn unknown_image_returns_not_supported() {
        let config = Config {
            version: 1,
            base_image: BaseImageConfig {
                image: "centos".to_string(),
                tag: "latest".to_string(),
            },
        };
        let err = build_cf(&config, &[], false).unwrap_err();

        assert_eq!(
            err,
            ContainerfileError::NotSupported {
                image: "centos".to_string()
            }
        );
    }
    #[test]
    fn feature_copy_instruction_present() {
        let feature = mock_feature("./tools/my-feature", "feature-0");
        let content = build_cf(&ubuntu_config("24.04"), &[feature], false).unwrap();

        assert!(content.contains("COPY features/feature-0/"));
        assert!(content.contains("/tmp/feature-install/feature-0/install.sh"));
    }

    #[test]
    fn feature_options_in_run_command() {
        let mut feature = mock_feature("./tools/my-feature", "feature-0");
        feature
            .merged_options
            .insert("VERSION".to_string(), "1.0".to_string());
        let content = build_cf(&ubuntu_config("24.04"), &[feature], false).unwrap();

        assert!(content.contains("VERSION=\"1.0\""));
    }

    #[test]
    fn feature_container_env_emitted_as_env_instruction() {
        let mut feature = mock_feature("./tools/my-feature", "feature-0");
        feature
            .container_env
            .insert("CARGO_HOME".to_string(), "/home/sandbox/.cargo".to_string());
        let content = build_cf(&ubuntu_config("24.04"), &[feature], false).unwrap();

        assert!(content.contains("ENV CARGO_HOME=\"/home/sandbox/.cargo\""));
    }

    #[test]
    fn feature_install_dir_cleaned_up() {
        let feature = mock_feature("./tools/my-feature", "feature-0");
        let content = build_cf(&ubuntu_config("24.04"), &[feature], false).unwrap();

        assert!(content.contains("RUN rm -rf /tmp/feature-install\n"));
    }

    #[test]
    fn no_features_produces_same_output_as_before() {
        let with_empty = build_cf(&ubuntu_config("24.04"), &[], false).unwrap();

        assert!(!with_empty.contains("# Feature:"));
        assert!(!with_empty.contains("_REMOTE_USER"));
        assert!(!with_empty.contains("rm -rf /tmp/feature-install"));
    }

    #[test]
    fn ubuntu_copies_policy_yaml() {
        let content = build_cf(&ubuntu_config("24.04"), &[], true).unwrap();
        assert!(content.contains("COPY policy.yaml /etc/openshell/policy.yaml"));
    }

    #[test]
    fn ubuntu_omits_policy_yaml_without_flag() {
        let content = build_cf(&ubuntu_config("24.04"), &[], false).unwrap();
        assert!(!content.contains("COPY policy.yaml /etc/openshell/policy.yaml"));
    }

    #[test]
    fn fedora_copies_policy_yaml() {
        let content = build_cf(&fedora_config(), &[], true).unwrap();
        assert!(content.contains("COPY policy.yaml /etc/openshell/policy.yaml"));
    }

    #[test]
    fn copy_containerfile_is_supported_on_all_base_images() {
        for config in [
            ubuntu_config("24.04"),
            fedora_config(),
            ubi_config(),
            hummingbird_config(),
        ] {
            let content = generate(&config, &[], false, true).unwrap();
            assert!(content.contains("COPY build-containerfile /tmp/build-containerfile"));
            assert!(content.contains("RUN cp /tmp/build-containerfile \"$HOME/Containerfile\""));
            assert!(!content.contains("--chown=sandbox"));
            let without_copy = build_cf(&config, &[], false).unwrap();
            assert!(!without_copy.contains("build-containerfile"));
        }
    }

    // CA cert tests

    #[test]
    fn host_ca_certificates_are_not_copied() {
        for content in [
            build_cf(&ubuntu_config("24.04"), &[], false).unwrap(),
            build_cf(&fedora_config(), &[], false).unwrap(),
            build_cf(&ubi_config(), &[], false).unwrap(),
            build_cf(&hummingbird_config(), &[], false).unwrap(),
        ] {
            assert!(
                !content.contains("COPY certs/"),
                "unexpected cert COPY: {content}"
            );
            assert!(!content.contains("system-ca.crt"));
            assert!(!content.contains("RUN update-ca-certificates"));
            assert!(!content.contains("RUN update-ca-trust"));
        }
    }
}

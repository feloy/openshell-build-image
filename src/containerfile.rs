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

use crate::feature::StagedFeature;

#[derive(Debug, PartialEq)]
pub enum ContainerfileError {
    InvalidImageReference { image: String },
}

impl std::fmt::Display for ContainerfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerfileError::InvalidImageReference { image } => write!(
                f,
                "invalid image reference {image:?}: expected a nonempty reference without whitespace or control characters"
            ),
        }
    }
}

impl std::error::Error for ContainerfileError {}

/// Validate the reference before inserting it into a FROM instruction.
/// The build engine validates the registry, repository, tag, and digest syntax.
pub fn parse_image_reference(image: &str) -> Result<String, ContainerfileError> {
    if image.is_empty() || image.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(ContainerfileError::InvalidImageReference {
            image: image.to_string(),
        });
    }
    Ok(image.to_string())
}

pub fn generate(
    from: &str,
    features: &[StagedFeature],
    copy_containerfile: bool,
) -> Result<String, ContainerfileError> {
    let image = parse_image_reference(from)?;
    Ok(format!(
        "# System base\nFROM {image} AS system\n\nUSER 0\n\n{}",
        final_stage(features, copy_containerfile)
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

fn final_stage(features: &[StagedFeature], copy_containerfile: bool) -> String {
    let containerfile_section = if copy_containerfile {
        "COPY build-containerfile /tmp/build-containerfile\n\
         RUN cp /tmp/build-containerfile \"$HOME/Containerfile\" && rm /tmp/build-containerfile\n\n"
    } else {
        ""
    };
    let features_section = features_section(features);
    format!(
        r#"# Final base image
FROM system AS final

{features_section}{containerfile_section}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn build_cf(from: &str, features: &[StagedFeature]) -> Result<String, ContainerfileError> {
        generate(from, features, false)
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
    fn arbitrary_image_references_are_preserved() {
        for image in [
            "alpine",
            "docker.io/library/alpine:3.24",
            "registry.fedoraproject.org/fedora:latest",
            "localhost:5000/project/tools:dev",
            "ghcr.io/example/project@sha256:0123456789abcdef",
            "example/project:tag@sha256:0123456789abcdef",
        ] {
            let content = build_cf(image, &[]).unwrap();
            assert!(content.contains(&format!("FROM {image} AS system\n")));
            assert!(
                !content.contains("RUN "),
                "unexpected build command: {content}"
            );
        }
    }

    #[test]
    fn invalid_references_cannot_change_containerfile_instructions() {
        for image in [
            "",
            " ",
            "alpine AS injected",
            "alpine\nRUN touch /injected",
            "alpine\r",
            "alpine\t",
            "alpine\0",
        ] {
            let err = build_cf(image, &[]).unwrap_err();
            assert!(matches!(
                err,
                ContainerfileError::InvalidImageReference { .. }
            ));
            assert!(err.to_string().contains("invalid image reference"));
        }
    }

    #[test]
    fn feature_copy_instruction_present() {
        let feature = mock_feature("./tools/my-feature", "feature-0");
        let content = build_cf("docker.io/library/ubuntu:24.04", &[feature]).unwrap();

        assert!(content.contains("COPY features/feature-0/"));
        assert!(content.contains("/tmp/feature-install/feature-0/install.sh"));
    }

    #[test]
    fn feature_options_in_run_command() {
        let mut feature = mock_feature("./tools/my-feature", "feature-0");
        feature
            .merged_options
            .insert("VERSION".to_string(), "1.0".to_string());
        let content = build_cf("docker.io/library/ubuntu:24.04", &[feature]).unwrap();

        assert!(content.contains("VERSION=\"1.0\""));
    }

    #[test]
    fn feature_container_env_emitted_as_env_instruction() {
        let mut feature = mock_feature("./tools/my-feature", "feature-0");
        feature
            .container_env
            .insert("CARGO_HOME".to_string(), "/home/sandbox/.cargo".to_string());
        let content = build_cf("docker.io/library/ubuntu:24.04", &[feature]).unwrap();

        assert!(content.contains("ENV CARGO_HOME=\"/home/sandbox/.cargo\""));
    }

    #[test]
    fn feature_install_dir_cleaned_up() {
        let feature = mock_feature("./tools/my-feature", "feature-0");
        let content = build_cf("docker.io/library/ubuntu:24.04", &[feature]).unwrap();

        assert!(content.contains("RUN rm -rf /tmp/feature-install\n"));
    }

    #[test]
    fn no_features_produces_same_output_as_before() {
        let with_empty = build_cf("docker.io/library/ubuntu:24.04", &[]).unwrap();

        assert!(!with_empty.contains("# Feature:"));
        assert!(!with_empty.contains("_REMOTE_USER"));
        assert!(!with_empty.contains("rm -rf /tmp/feature-install"));
    }

    #[test]
    fn copy_containerfile_is_supported_on_all_base_images() {
        for config in [
            "docker.io/library/ubuntu:24.04",
            "registry.fedoraproject.org/fedora:latest",
            "docker.io/library/alpine:3.24",
            "ghcr.io/example/project:dev",
        ] {
            let content = generate(config, &[], true).unwrap();
            assert!(content.contains("COPY build-containerfile /tmp/build-containerfile"));
            assert!(content.contains("RUN cp /tmp/build-containerfile \"$HOME/Containerfile\""));
            assert!(!content.contains("--chown=sandbox"));
            let without_copy = build_cf(config, &[]).unwrap();
            assert!(!without_copy.contains("build-containerfile"));
        }
    }

    // CA cert tests

    #[test]
    fn host_ca_certificates_are_not_copied() {
        for content in [
            build_cf("docker.io/library/ubuntu:24.04", &[]).unwrap(),
            build_cf("registry.fedoraproject.org/fedora:latest", &[]).unwrap(),
            build_cf("registry.access.redhat.com/ubi10/ubi:latest", &[]).unwrap(),
            build_cf("docker.io/library/alpine:3.24", &[]).unwrap(),
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

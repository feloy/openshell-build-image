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

pub fn generate(from: &str, copy_containerfile: bool) -> Result<String, ContainerfileError> {
    let image = parse_image_reference(from)?;
    Ok(format!(
        "# System base\nFROM {image} AS system\n\nUSER 0\n\n{}",
        final_stage(copy_containerfile)
    ))
}

fn final_stage(copy_containerfile: bool) -> String {
    let containerfile_section = if copy_containerfile {
        "COPY build-containerfile /tmp/build-containerfile\n\
         RUN cp /tmp/build-containerfile \"$HOME/Containerfile\" && rm /tmp/build-containerfile\n\n"
    } else {
        ""
    };
    format!(
        r#"# Final base image
FROM system AS final

{containerfile_section}
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn build_cf(from: &str) -> Result<String, ContainerfileError> {
        generate(from, false)
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
            let content = build_cf(image).unwrap();
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
            let err = build_cf(image).unwrap_err();
            assert!(matches!(
                err,
                ContainerfileError::InvalidImageReference { .. }
            ));
            assert!(err.to_string().contains("invalid image reference"));
        }
    }

    #[test]
    fn copy_containerfile_is_supported_on_all_base_images() {
        for config in [
            "docker.io/library/ubuntu:24.04",
            "registry.fedoraproject.org/fedora:latest",
            "docker.io/library/alpine:3.24",
            "ghcr.io/example/project:dev",
        ] {
            let content = generate(config, true).unwrap();
            assert!(content.contains("COPY build-containerfile /tmp/build-containerfile"));
            assert!(content.contains("RUN cp /tmp/build-containerfile \"$HOME/Containerfile\""));
            assert!(!content.contains("--chown=sandbox"));
            let without_copy = build_cf(config).unwrap();
            assert!(!without_copy.contains("build-containerfile"));
        }
    }

    // CA cert tests

    #[test]
    fn host_ca_certificates_are_not_copied() {
        for content in [
            build_cf("docker.io/library/ubuntu:24.04").unwrap(),
            build_cf("registry.fedoraproject.org/fedora:latest").unwrap(),
            build_cf("registry.access.redhat.com/ubi10/ubi:latest").unwrap(),
            build_cf("docker.io/library/alpine:3.24").unwrap(),
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

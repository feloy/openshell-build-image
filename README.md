# openshell-build-image

[OpenShell](https://github.com/NVIDIA/OpenShell-Community) is NVIDIA's runtime environment for autonomous AI agents. It provides isolated sandboxes where agents can safely run and iterate — without risk to the host system or your credentials.

OpenShell ships a set of [pre-built sandbox images](https://github.com/NVIDIA/OpenShell-Community), but they are general-purpose. `openshell-build-image` lets you build your own: lightweight, workspace-specific images that contain only what you need — without writing a Containerfile by hand.

The tool assembles the image from a base image and project-specific toolchains. Use `--runtime` to select what drives the build: a container CLI on the host (`podman`, `docker`, or the macOS `container` CLI), or a microVM (`vm`) that needs no container runtime installed at all — see [Building in a VM](#building-in-a-vm---runtime-vm).

1. **Base image** — chosen via a config file, defaults to Ubuntu 24.04. The builder uses the packages already present in the base image; it does not automatically install system tools.
2. **Project-specific toolchains** — toolchains and utilities declared as Dev Container Features in `.kaiden/workspace.json` are installed when `--with-workspace-config` is used.

Built-in agent installation and configuration have been removed. OCI addon support is tracked in [#178](https://github.com/openkaiden/openshell-build-image/issues/178).

### workspace.json fields

`.kaiden/workspace.json` is the per-workspace configuration file, read when `--with-workspace-config` is passed. The following fields are supported:

| Field | Description | Details |
| ----- | ----------- | ------- |
| `features` | Dev Container Features to install in the image | [Dev Container Features](#dev-container-features) |
| ~~`skills`~~ | ~~Skill directories~~ | ~~not used by the image builder~~ |
| ~~`environment`~~ | ~~Environment variables to inject into the workspace~~ | ~~not used by the image builder~~ |
| ~~`mcp`~~ | ~~MCP server configuration (command-based and URL-based servers)~~ | ~~not used by the image builder~~ |
| ~~`mounts`~~ | ~~Host directories to mount in the workspace~~ | ~~not used by the image builder~~ |
| ~~`ports`~~ | ~~TCP ports to expose from the workspace~~ | ~~not used by the image builder~~ |
| ~~`secrets`~~ | ~~Secret names to inject into the workspace~~ | ~~not used by the image builder~~ |

## Quick start

Build an image with a single command:

```sh
openshell-build-image --runtime podman myimage:latest
```

`<TAG>` and `--runtime` are the only required arguments — `--runtime` selects the build backend (`podman`, `docker`, `container`, or `vm`), and `<TAG>` sets the tag for the built image. By default, the tool uses Ubuntu 24.04 as the base image.

## Building in a VM (`--runtime vm`)

The three CLI runtimes hand the build to a container engine installed on your machine. `--runtime vm` instead boots a lightweight Linux microVM with [libkrun](https://github.com/containers/libkrun) and runs `buildah` inside it, so no container engine is needed on the host.

The VM runs its own kernel in its own process namespace and sees only three directories you share with it: the VM's root filesystem, the build context, and the output directory. Nothing else on the host is reachable from the build.

### What it produces

This is the one way `--runtime vm` differs from the others in its result. A container CLI leaves a tagged image in its local image store. The VM has no access to that store, so it writes a **flattened rootfs tarball** instead:

```sh
openshell-build-image --runtime vm myimage:latest
# -> ./myimage-latest.tar
```

The default filename comes from `<TAG>`, with `:` and `/` replaced by `-`. Override it with `--vm-output`. Load the result into an image store yourself, or unpack it:

```sh
tar -xf myimage-latest.tar -C /path/to/rootfs
```

### Requirements

`--runtime vm` works only on **macOS with Apple Silicon**, because libkrun builds on Apple's Hypervisor.framework. It also needs:

1. **libkrun**, which the binary links against rather than bundling:

   ```sh
   brew tap libkrun/krun && brew trust libkrun/krun
   brew install libkrun/krun/libkrun
   ```

   If a build fails with `Couldn't find or load libkrunfw`, add `export DYLD_LIBRARY_PATH="$(brew --prefix)/lib"`.

2. **A binary built with the `vm` feature and signed for the hypervisor.** The `aarch64-apple-darwin` download from a release has both. To build one yourself:

   ```sh
   make build-vm
   ```

   The feature is off by default because it links against libkrun, and macOS refuses the hypervisor to an unsigned binary. `make build-vm` does both, and re-signs on every build, since compiling clears the signature.

The VM's own root filesystem — a Linux tree with `buildah` in it — comes inside the binary and unpacks itself on first use, into `~/Library/Application Support/openshell-build-image/vm-rootfs`.

### Building the VM's root filesystem

Only needed to change what the build VM contains. It takes Podman on a `linux/arm64` machine:

```sh
make build-vm-rootfs                  # writes vm-rootfs/ and vm-rootfs.tar
make build-vm                         # embeds the tarball in the binary
make run-vm TAG=myimage:latest        # builds an image with it
```

Or point at the directory without embedding anything: `--vm-rootfs ./vm-rootfs`. `make help` lists the rest.

### Sizing the VM

`buildah` uses the `vfs` storage driver inside the VM — virtio-fs does not support overlayfs — which stores a full copy of every layer rather than a diff. Builds therefore need more memory than the same build under overlayfs. If one fails with an out-of-memory message, raise it:

```sh
openshell-build-image \
  --runtime vm \
  --vm-cpus 4 \
  --vm-memory 8192 \
  myimage:latest
```

### DNS inside the VM

The VM has no virtual network card: libkrun forwards its connections to the host, which makes them for real. Nothing supplies the guest a `resolv.conf`, and a lookup still travels to whatever nameserver the guest is told to use — the host's resolver settings do not apply to it, only the host's routing and firewall rules.

So the build reads the host's own nameservers on each run and hands them to the VM. That is what makes a build work behind a firewall that only allows DNS to the company resolver, and what lets a `FROM` line reach a registry mirror that only internal DNS knows about.

Override it when the host's resolvers are not the ones the build should use:

```sh
openshell-build-image --runtime vm --vm-dns 10.0.0.53 --vm-dns 10.0.0.54 myimage:latest
```

A loopback address is rejected: inside the VM, loopback is the VM. If the host resolves through one — systemd-resolved, a VPN client's local stub — pass the address it forwards to instead. When the host has no usable nameserver at all, the VM falls back to `1.1.1.1`.

## Configuring the base image

To use a different base image or tag, create a configuration file.

### File location

The tool looks for a `config.toml` file in this order, using the first directory found:

1. Directory given by the `--config` flag
2. Directory set in the `OPENSHELL_BUILD_IMAGE_CONFIG` environment variable
3. The platform config directory:
   - Linux: `$XDG_CONFIG_HOME/openshell-build-image/` (defaults to `~/.config/openshell-build-image/`)
   - macOS: `~/Library/Application Support/openshell-build-image/`
   - Windows: `%APPDATA%\openshell-build-image\`

If no `config.toml` is found in the resolved directory, or the file is empty, built-in defaults are used.

If a directory is given explicitly (via `--config` or the environment variable) but it does not exist, the command fails immediately.

### Base images

**Ubuntu** (default)

```toml
[openshell_build_image.base_image]
image = "ubuntu"
tag   = "24.04"
```

**Fedora**

```toml
[openshell_build_image.base_image]
image = "fedora"
tag   = "latest"
```

**Red Hat UBI**

```toml
[openshell_build_image.base_image]
image = "ubi"
tag   = "latest"
```

**Red Hat Hardened Images (Hummingbird)**

```toml
[openshell_build_image.base_image]
image = "hummingbird"
tag   = "latest-builder"
```

### Full schema reference

```toml
[openshell_build_image]
version = 1

[openshell_build_image.base_image]
image = "ubuntu"   # "ubuntu", "fedora", "ubi", or "hummingbird"
tag   = "24.04"
```

| Field                                      | Default  | Description                  |
| ------------------------------------------ | -------- | ---------------------------- |
| `openshell_build_image.version`          | `1`      | Configuration schema version |
| `openshell_build_image.base_image.image` | `ubuntu` | Base image name (`ubuntu`, `fedora`, `ubi`, or `hummingbird`) |
| `openshell_build_image.base_image.tag`   | `24.04`  | Base image tag — Ubuntu: `24.04`, `22.04`, …; Fedora: `latest`, `43`, `42`, …; UBI: `latest`, `10.2-1780377767`, …; Hummingbird: `latest-builder`, … |

### Loading from a specific config directory

Pass `--config` to point to a directory explicitly (the tool reads `config.toml` inside it):

```sh
openshell-build-image --runtime podman --config /path/to/config/dir myimage:latest
```

Or set the environment variable instead:

```sh
OPENSHELL_BUILD_IMAGE_CONFIG=/path/to/config/dir openshell-build-image myimage:latest
```

## Logging

Use `-v` (info) or `-vv` (debug) to increase log verbosity — useful for tracing which config file is loaded:

```sh
openshell-build-image --runtime podman -v myimage:latest
```

OpenShell manages CA certificates for the sandbox. The image builder does not discover or copy the host's CA bundle into images.

## Dev Container Features

The tool supports [Dev Container Features](https://containers.dev/implementors/features/) declared in `.kaiden/workspace.json`. Pass `--with-workspace-config` to enable this; without it the file is not read and no features are installed.

### workspace.json schema

```json
{
  "features": {
    "<feature-ref>": {
      "<option>": "<value>"
    }
  }
}
```

Each key in `features` is a feature reference; each value is a map of options passed to the feature's `install.sh`.

### Feature references

| Format                 | Example                                  | Resolves to              |
| ---------------------- | ---------------------------------------- | ------------------------ |
| OCI registry reference | `ghcr.io/devcontainers/features/rust:1`  | downloaded from registry |
| Local path             | `./my-feature`                           | `.kaiden/my-feature/`    |

Local paths are resolved relative to `.kaiden/`: `./my-feature` points to `.kaiden/my-feature/`.

OCI references without an explicit registry default to `ghcr.io`. Tags and digests (`@sha256:…`) are both supported. Direct `http://` / `https://` tarball URLs are not supported.

### Example

```json
{
  "features": {
    "ghcr.io/devcontainers/features/rust:1": {
      "version": "stable",
      "profile": "minimal"
    },
    "./my-feature": {}
  }
}
```

With the above, `./my-feature` refers to a local feature at `.kaiden/my-feature/`.

### Installation order

Features are installed in the order defined by each feature's `installsAfter` field in its `devcontainer-feature.json`. Within the same dependency level, features are processed in alphabetical order by reference.

### How it works

When `--with-workspace-config` is passed, the tool reads `.kaiden/workspace.json` and:

1. Downloads and extracts each OCI feature into a temporary build context directory (`/tmp/openshell-build-image…`).
2. Copies local feature directories into the same build context.
3. Passes the build context to `podman build`, where each feature is installed via:
   ```dockerfile
   COPY features/<dir>/ /tmp/feature-install/<dir>/
   RUN chmod +x /tmp/feature-install/<dir>/install.sh && \
       OPTION="value" /tmp/feature-install/<dir>/install.sh
   ```
4. Cleans up all feature files from the image with `RUN rm -rf /tmp/feature-install` after all features are installed.

Features run as root so install scripts can write to system paths.

## Saving the Containerfile

Pass `--copy-containerfile` to include the exact Containerfile used for the build at `$HOME/Containerfile` **inside the image**, using the home directory and user inherited from the base image.

```sh
openshell-build-image --runtime podman --copy-containerfile myimage:latest
podman run --rm --entrypoint /bin/sh myimage:latest -c 'cat "$HOME/Containerfile"'
```

## Full option reference

```
openshell-build-image [OPTIONS] <TAG>
```

| Argument / Option              | Description                                                        |
| ------------------------------ | ------------------------------------------------------------------ |
| `<TAG>`                        | Tag for the built image (e.g. `myimage:latest`)                    |
| `--runtime <RUNTIME>`          | Backend to build the image with (`podman`, `docker`, `container`, `vm` — see [Building in a VM](#building-in-a-vm---runtime-vm)) |
| `--config <CONFIG>`            | Path to config directory containing `config.toml` (env: `OPENSHELL_BUILD_IMAGE_CONFIG`) |
| `--with-workspace-config`      | Read `.kaiden/workspace.json` and apply its features |
| `--copy-containerfile`         | Copy the build Containerfile to `$HOME/Containerfile` inside the image |
| `--vm-rootfs <DIR>`            | Root filesystem the build VM boots from (`--runtime vm` only). Defaults to the one embedded in the binary. |
| `--vm-output <FILE>`           | Path for the rootfs tarball produced by `--runtime vm`. Defaults to a name derived from `<TAG>` in the current directory. |
| `--vm-cpus <N>`                | vCPUs given to the build VM (`--runtime vm` only). Default `2`.     |
| `--vm-memory <MIB>`            | RAM in MiB given to the build VM (`--runtime vm` only). Default `4096`. |
| `--vm-dns <ADDR>`              | Nameserver the build VM resolves through (`--runtime vm` only). Repeatable. Defaults to the host's own nameservers. |
| `-v` / `-vv`                   | Increase log verbosity (info / debug)                              |

The five `--vm-*` options are rejected with any other `--runtime`, rather than silently ignored.

## Example — project toolchains

With Dev Container Features declared in `.kaiden/workspace.json`, build the image:

```sh
openshell-build-image --runtime podman \
  --with-workspace-config \
  myproject:latest
```

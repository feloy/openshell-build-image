# Shared CI building blocks

`build-vm-rootfs.yml` is a reusable job: it builds the image builder's rootfs and
uploads `vm-rootfs.tar`. E2E and VM Runtime call it independently, and each caller
receives an artifact in its own workflow run.

The smaller OpenShell operations are composite actions in `.github/actions/`.
Composite actions run as steps on the caller's existing runner, so certificates,
images, sockets and background processes remain available to subsequent steps.
Separate reusable workflow jobs would run on separate runners and could not
share that live state.

The caller owns runner selection, approval environments, dependencies, release
assets, checksum tool, driver signing, image builds, driver configuration,
runtime diagnostics and resource cleanup. The shared actions have no VM/Podman
branches. `.github/workflows/e2e.yml` shows the complete Podman and VM callers.
The self-hosted VM job retains `environment: integration`; configure its required
reviewers in the caller repository before enabling it.

## Alpine E2E image selection

Each driver matrix lists a short `name` for job and artifact labels and a
digest-pinned `image` reference passed directly to the builder through
`E2E_BASE_IMAGE`. The Alpine entry selects `docker.io/alpine/curl:8.22.0`. It includes
Linux AMD64 and ARM64 variants, Alpine, `/bin/sh`, `/bin/sleep`, `/usr/bin/curl`,
and CA certificates under `/etc`.
These prerequisites require no Dev Container Features.
Each driver job uses an Ubuntu/Alpine matrix with `fail-fast: false`, so both
variants run even if one fails. Each variant installs OpenShell, builds the image
builder and gets a fresh gateway, sandbox, policy state and temporary directory.
Diagnostics are stored separately
under `target/e2e/<driver>/<image>` and uploaded as `e2e-<driver>-<image>`.
Cleanup runs after each variant even on failure, including its OpenShell
installation. The VM matrix uses `max-parallel: 1` to limit resource use on the
self-hosted runner, without guaranteeing Ubuntu/Alpine execution order. Each VM
variant also removes its downloaded build rootfs and, after upload, its diagnostic
directory. The build-rootfs job runs once and supplies the same artifact to both
VM variants.

Two small result jobs preserve the required-check names `Podman on Linux` and
`VM on macOS`. They run with `if: always()` and require the corresponding matrix
result to be `success`; failed, cancelled or skipped variants cannot pass the
required check.

The builder selects root for its build steps, and the copied Containerfile is at
`/root/Containerfile` in the resulting image. Sandbox runtime identity must still
be explicitly set to UID/GID 10001 by the existing policy and driver configuration.
The explicit sandbox `/bin/sh` command overrides the inherited `/entrypoint.sh`.
`/usr/bin/curl` matches the existing policy and denial-log assertions.

The curl project's `curlimages/curl:8.22.0` was also evaluated. Its ARM64 image
contains a root-only `/etc/resolv.conf` (mode 0700). OpenShell v0.1.2's VM init
rewrites the resolver contents while preserving the existing file mode, so UID
10001 cannot read it and DNS fails before any network-policy assertion. The
selected `alpine/curl` image avoids that incompatibility.

## Use the installer from another repository

The installer is independent of this project's Makefile and E2E tests. Its bundled
[release manifest](../actions/install-openshell/release.json) is the single source
for the tested OpenShell version and each platform's asset names and SHA-256
checksums. Choose the platform and checksum tool in the caller, and pin the action
reference to a full commit SHA. Manifest mode requires jq on the caller's runner.

```yaml
jobs:
  setup:
    runs-on: ubuntu-26.04
    steps:
      - name: Install OpenShell
        id: openshell
        uses: openkaiden/openshell-build-image/.github/actions/install-openshell@<full-commit-sha>
        with:
          platform: linux-x86_64
          checksum-command: sha256sum --check
      - name: Check CLI
        env:
          OPENSHELL_BIN_DIR: ${{ steps.openshell.outputs.directory }}
        run: '"$OPENSHELL_BIN_DIR/openshell" --version'
```

Replace `<full-commit-sha>` with a published commit containing these actions.
For macOS, select `platform: macos-arm64` and `shasum -a 256 --check`. The caller
signs the VM driver with the required entitlement after installation. The installer
returns both `directory` and `version`; E2E exports that version as `E2E_VERSION`
for its driver image tags and Rust assertions.

To test a different release, supply `release-file` with a caller-owned manifest
using the same `version` and `platforms` schema. Alternatively, provide both
`version` and explicit `assets` lines (asset name, binary name, SHA-256 digest),
omitting `platform` and `release-file`. This explicit mode does not require jq.

| Action | Inputs and contract |
| --- | --- |
| `initialize-openshell` | Required `driver`, `artifacts`; optional `library-path` for macOS. Creates a private directory and exports `E2E_DIR`, `E2E_NAME`, `E2E_IMAGE`, `E2E_DRIVER`, `E2E_ARTIFACTS`, `E2E_LIBRARY_PATH` through `GITHUB_ENV`. |
| `install-openshell` | Required `platform` for manifest mode; optional `release-file` overrides the bundled release. Alternatively provide both `version` and `assets`. Optional `directory` (`runner.temp/openshell-bin`), `checksum-command` (`sha256sum --check`). Outputs installed `directory` and `version`. |
| `start-openshell-gateway` | After initialization and installation into `E2E_DIR/bin`, consumes required `driver-settings`, a TOML fragment file prepared by the caller. Replaces `__GATEWAY_PORT__` with the chosen gateway port. Requires OpenSSL and jq. |
| `create-openshell-sandbox` | After gateway startup, consumes required `image` and optional `policy` (defaults to `tests/e2e/policy.yaml`). Starts a retained sandbox and verifies readiness. |
| `collect-openshell-diagnostics` | After initialization, collects sandbox logs and metadata when creation was attempted. Call with `if: always()` before stopping the gateway. |
| `stop-openshell` | Deletes the sandbox and stops owned process groups; call with `if: always()`. Leaves `E2E_DIR` for the caller's driver cleanup, after which the caller removes it. |

Use the lifecycle actions in that order within a single job. Runtime-specific
setup and teardown belong between those shared steps, as shown by the E2E
workflow. A caller can run its own assertions instead of this project's Rust
tests. Each initialized job receives its own private state directory. Give
simultaneous jobs distinct diagnostic directories and uploaded artifact names.

## Use the rootfs workflow from another repository

The default source checkout is the caller repository at the caller commit.
When the caller is a different project, override **both** source inputs to an
openshell-build-image checkout containing its Makefile and rootfs build script.
Pin the workflow and source references to full commit SHAs, normally the same SHA.

```yaml
permissions:
  contents: read

jobs:
  rootfs:
    uses: openkaiden/openshell-build-image/.github/workflows/build-vm-rootfs.yml@<full-commit-sha>
    with:
      source-repository: openkaiden/openshell-build-image
      source-ref: <full-commit-sha>
      runner-labels: '["ubuntu-26.04-arm"]'
      artifact-name: openshell-builder-rootfs
      retention-days: 1
```

| Input | Default / purpose |
| --- | --- |
| `source-repository`, `source-ref` | Caller repository and commit; override together for another source repository |
| `runner-labels` | JSON array `["ubuntu-26.04-arm"]`; requires native Linux ARM64 with apt and sudo |
| `artifact-name` | `vm-rootfs`; unique within the caller workflow run |
| `retention-days` | `0`, using the repository default |

The workflow repository must allow calls from the consuming repository. For a
different private source repository, pass its read token as the optional
`source-token` secret; the caller's `GITHUB_TOKEN` normally cannot read it.
Consumers download the named artifact after a `needs` dependency on the rootfs
job. Preserve the source commit and artifact name when connecting a producer
to a consumer. Sharing the definition removes duplicated YAML; each workflow
run still performs its own build.

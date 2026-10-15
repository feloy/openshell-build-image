# End-to-end runtime coverage

The [OpenShell E2E workflow](../.github/workflows/e2e.yml) tests these runtime
and host platform combinations. Each cell names the OpenShell compute driver
used to run the built image:

| Runtime (`--runtime`) | macOS | Linux | Windows |
| --- | --- | --- | --- |
| Podman (`podman`) | — | `podman` | — |
| Docker (`docker`) | — | `docker` | — |
| Apple container (`container`) | `vm` | — | — |
| VM (`vm`) | `vm` | — | — |

`—` means no end-to-end test runs for that combination. The Apple container
tests build an image with `container`, export its filesystem, and run it through
OpenShell's VM driver with the same network-policy assertions as the other E2E
jobs. Both macOS jobs run on a self-hosted Apple Silicon Mac. GitHub-hosted
macOS runners cannot boot the required nested VMs.

# End-to-end runtime coverage

The [OpenShell E2E workflow](../.github/workflows/e2e.yml) tests these runtime
and host platform combinations:

| Runtime (`--runtime`) | macOS | Linux | Windows |
| --- | --- | --- | --- |
| Podman (`podman`) | — | Yes | — |
| Docker (`docker`) | — | Yes | — |
| Apple container (`container`) | — | — | — |
| VM (`vm`) | Yes | — | — |

`—` means no end-to-end test runs for that combination. The VM tests run on a
self-hosted Apple Silicon Mac.

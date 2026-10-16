# OCI addon E2E fixtures

`build.sh` builds three addon formats in the local Podman image store. Supply
a unique local image prefix:

```bash
bash tests/e2e/addons/build.sh localhost/osbi-addon-fixture-local
```

The `main` branch workflow runs `build.sh` and then `publish.sh` in the same
job, passing the same local image prefix to both. `publish.sh` pushes the
resulting Podman images and indexes directly to
`ghcr.io/openkaiden/tests/addon-single`, `addon-platforms`, and
`addon-platforms-libc`. It runs only when the fixture Containerfile, scripts,
or workflow changes on `main`; pull requests do not publish images. The
workflow summary contains digest-pinned references for E2E tests.

| Fixture | Registry object | Variants |
| --- | --- | --- |
| `single-amd64`, `single-arm64` | Separate single manifests | One Linux architecture each |
| `platforms` | OCI index | `linux/amd64`, `linux/arm64`; no libc annotations |
| `platforms-libc` | OCI index | Both architectures with `glibc` and `musl` descriptors |

Each variant contains its own
`/usr/local/share/openshell-addon-test/<fixture>/selected` and
`variants/<variant>` markers, plus a shared `last-applied` marker. It sets
`OSBI_TEST_<FIXTURE>` and `OSBI_TEST_VARIANT` in the image configuration, so
tests can check both fixture selection and addon order.

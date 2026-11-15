# Specialist installation and build efficiency review

Reviewed on 2026-11-15 against the installation paths based on commit
`c40250453c66fe320d64e40acda782cbff2a5c85`. The host checks used Python 3.12.14,
Node 24.22.3 and npm 11.19.1. There was no Docker daemon available: the results
below establish installer behavior and source-level build changes, not an image
build, container smoke result, image size, or Docker build-time improvement.

## Findings and changes

| Area | Observed behavior | Result |
| --- | --- | --- |
| Shared specialist tools | `select_tools` already returns a sorted set, and the suite installs that set once. | Kept the existing deduplication and independent tool environments. |
| Repeated Python installation | `venv` reused an existing environment. pip accepted a modified installed package as already satisfied, and unrelated old files survived. | Recreate the tool environment with `venv --clear`, then install the hashed lock and run `pip check`. A real offline fixture reproduced the failure before the change and passes afterward. |
| Python downloads | Every tool forced `--no-cache-dir`, including wheel-only installs with shared dependencies. | Wheel-only installs use an explicit cache below `--cache`. Locks still require every package hash and prohibit source packages. Source-build opt-ins continue with `--no-cache-dir`; their wheel-only build prerequisites may use the download cache. |
| Image download caches | The family helper deleted npm and verified binary downloads after each target build. | Image targets explicitly opt into a BuildKit cache mount; ordinary helper calls still remove the temporary default cache. |
| Node layer invalidation | All agent runtime files and locks were copied before installing Node. | Copy only the installer and pinned Node inputs before that layer. Runtime/catalog edits no longer invalidate it when the workspace base and those inputs remain unchanged. |
| Host interpreter files | Local test runs had produced 23 Python bytecode files under the copied agents/finance directories. | A specialist-Dockerfile-specific ignore file excludes `__pycache__`, `.pyc`, and `.pyo` files. Other image contexts are unchanged. |
| Interrupted release downloads | The resolver wrote directly to its reusable URL-keyed file. An interrupted transfer left a partial entry that blocked a later retry. | Download into a temporary file, check upstream size/digest when supplied, then publish atomically. The failed temporary file is removed. |
| Release hashing | The resolver loaded each complete artifact into a Python bytes object. | Hash in 1 MiB chunks. Existing content-addressed entries are checked before reuse, including rejecting symlinks. |

The implementation is in [install.py](../deploy/agents/install.py),
[resolve-locks.py](../deploy/agents/resolve-locks.py),
[build-family.sh](../deploy/agents/build-family.sh), and the
[specialist Dockerfile](../deploy/specialists.Dockerfile).

## Cache behavior and boundaries

The image uses the cache ID
`boxoro-specialists-linux-amd64-py312-node24-v1` at
`/var/cache/boxoro-specialists`, with `sharing=locked`. The ID separates the
reviewed platform, Python/Node versions, and cache policy; locked sharing makes
concurrent family builds wait for the current writer. No additional installer
parallelism was introduced. The cache is for a trusted builder and is never
mounted into an agent workspace. Artifact hashes, lock-file hashes, npm
integrity locks, approved npm lifecycle scripts, and safe archive extraction
remain enforced.

BuildKit cache mounts persist on the builder outside committed image layers.
They are independent of normal layer-cache reuse and do not automatically move
between fresh CI builders through `--cache-to`/`--cache-from`. Consequently,
repeated builds on the same builder can reuse package downloads; a new builder
still needs to acquire them. These are the documented BuildKit semantics, not
Docker measurements from this review. See the [Docker cache-mount
reference](https://docs.docker.com/reference/dockerfile/#run---mounttypecache)
and [cache optimization guide](https://docs.docker.com/build/cache/optimize/).

The helper's default remains `/tmp/boxoro-agent-downloads`, which it deletes
before the layer completes. `BOXORO_AGENT_CACHE` explicitly selects a reusable
location; custom images should set it only with a trusted external cache mount.
The container smoke test now rejects a final image containing download files
at either the default path or the BuildKit mount path.

pip's HTTP cache is an online-download optimization, not an offline package
index. The installer's existing `--offline` mode still requires a prepared
`CACHE/wheels` directory for Python, npm's populated cache for JavaScript, and
verified content-addressed binaries for releases. Source-built Python wheels
are intentionally not reused: their output hashes are not the downloaded
artifact hashes recorded in the requirements lock. See pip's [cache
behavior](https://pip.pypa.io/en/stable/topics/caching/) and [hash-checking
documentation](https://pip.pypa.io/en/stable/topics/secure-installs/). npm
documents integrity checks when cache entries are written and read in its
[cache reference](https://docs.npmjs.com/cli/v11/commands/npm-cache/).

The named ignore file follows Docker's [Dockerfile-specific ignore
convention](https://docs.docker.com/build/concepts/context/#filename-and-location).
It prevents host bytecode from becoming installation input; it does not change
the reviewed source or lock contents.

## Measured host results

### Fresh offline installation from existing caches

The installer created a new prefix for three locked tools using `--offline`
and the existing npm/binary cache. Installation completed with exit code 0 in
**2.389 seconds** on this host. This is a functional cache-reuse measurement,
not a cold-versus-warm speedup claim. All real executable probes exited 0:

| Tool | Observed version | Installation path exercised |
| --- | --- | --- |
| Prettier | 3.10.1 | npm integrity lock with scripts disabled |
| Buf | 1.75.0 | npm integrity lock with its versioned lifecycle-script approval |
| Hadolint | 2.15.1 | Content-addressed release artifact, rehashed before extraction |

The receipt retained installation lock SHA-256
`4543b98d111f673c5bf075eb77028c47cfe0b57cad2277f575bb0496022ca0bf`.

### Hashing allocation

For the same 33,554,432-byte fixture, `tracemalloc` measured peak Python
allocation of **33,559,866 bytes before** and **2,103,198 bytes after** streaming
the hash. Both returned SHA-256
`05f052c8f6da8ee5228ec291820b559c4be183773b9e97a6b82e30dacff85dd3`.
This is about a 16-fold reduction in peak traced allocation for that operation.
It is not total process RSS or a full-suite memory measurement. Single-run
elapsed times are retained in the [measurement record](validation/specialist-installation-review.json)
but are sensitive to host and filesystem-cache conditions.

## Verification

Run from the repository root:

```bash
python3 -m unittest qa.test_agents_install qa.test_agents_install_review -v
bash -n deploy/agents/build-family.sh scripts/build-specialist-image.sh scripts/build-full-agent-image.sh
hadolint --failure-threshold error deploy/specialists.Dockerfile
```

The **14 installer tests passed**, including real Python wheel installation,
restoration of modified installed files, removal of an unlocked dependency,
rejection of a changed wheel, rechecking a changed binary cache entry, safe
archive/path checks, npm script approval rules, and successful retry after an
interrupted release transfer. Shell syntax passed. Hadolint reported no errors;
the existing base-stage `USER root` warning (`DL3002`) and nonnumeric-user
information (`DL3066`) also appeared on the unchanged baseline Dockerfile.

The new final-image cache assertion and the existing nonroot, offline,
read-only-root, browser and executable probes still need a real image build and
`qa/specialist-image-smoke.sh`. Existing security/specialist composition and its
two smoke suites remain the validation path for the full worker image.

## Remaining duplication to assess separately

The security and specialist locks point to the same release artifacts for
Gitleaks, Trivy, Syft and Hadolint. This review keeps their established paths:
safe physical reuse would need to verify the installed bytes and preserve both
components' provenance and smoke checks. Ruff and shellcheck use different
versions or installation sources across those locks, so an executable-name
match alone is not enough to reuse them. No image-size reduction is claimed.

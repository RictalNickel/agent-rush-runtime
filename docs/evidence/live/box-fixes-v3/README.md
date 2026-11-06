# Box fixes v3: executable tmpfs, data packages, self-killing commands (2026-11-06)

Fixes for the box bugs found in the live demos ([demo-everyday-tasks](../demo-everyday-tasks/README.md) bug 1, [demo-build-kanban](../demo-build-kanban/README.md) bug 1). Verified on the live worker VM with scratch resources only:
- the image was built under the scratch tag `agent-rush-workspace:test`
- boxes ran on a scratch box network, `ar-box-test` (172.31.250.0/24), with its own rules tagged `ar-box-test-egress` and its own marker
- the binary was the debug build of this branch

The live `ar-box` network, the live boxes, the worker service, `/opt/agent-rush` and `/etc/agent-rush` were not touched. The scratch network, its rules and marker, and every scratch container and volume were removed afterwards. The worker's public address is redacted.

Image: `agent-rush-workspace@sha256:434b60c2f82492e94b1fd00b88b54ecf78472670167f231e0352fbcfc10a9bbd`

## What changed
1. **`/tmp` and `/home/agent` are mounted `exec`.** They keep `nosuid,nodev` and the 1 GiB limits. Docker mounts tmpfs `noexec` unless told otherwise, so no compiled extension under `$HOME` could load. The gVisor box is the sandbox, and `/workspace` was already executable.
2. **User-level installs go to `/workspace`, so they survive parking:**
   - `PYTHONUSERBASE=/workspace/.local`
   - `NPM_CONFIG_PREFIX=/workspace/.npm-global`
   - both `bin` directories are on `PATH`
   - `PIP_NO_CACHE_DIR=1` and `MPLBACKEND=Agg` are set

   `HOME` stays `/home/agent`, a tmpfs for caches and dotfiles. `PIP_USER` is not set because it would break `pip install` inside a venv, and plain `pip install` already falls back to a user install. See `deploy/box-tools/README.md`.
3. **The image adds `python3-numpy`, `python3-pandas`, `python3-matplotlib`, `python3-pil` and `librsvg2-bin` from apt.** `convert x.svg y.png|pdf` renders through `rsvg-convert`, and other image targets go through a PNG. ImageMagick's own `convert a.svg b.jpg` now works too, through the rsvg delegate.
4. **Exec always returns a result.**
   - The command reaches the in-box wrapper on stdin, ahead of the caller's stdin, and runs from a file with `eval`. It keeps `bash -lc` semantics: `$0` is `bash`, and errors read `bash: line N: …`.
   - The command never appears in any process's arguments, so `pkill -f '<part of the command>'` no longer matches the wrapper shells.
   - If a command kills its shell anyway, `docker exec` exits 128+N. The worker then:
     1. collects the output the command wrote from `/tmp/.arexec.<run id>`
     2. returns exit code 128+N with a stderr note, for example `[the shell was terminated by signal 15 (SIGTERM)]`

     Only other failures with no result header are errors.
   - Commands up to the 256 KiB limit no longer pass through argv, which caps one argument at 128 KiB.

## Results
| Check | Result |
| --- | --- |
| Nine box checks (`qualify-box`) on `ar-box-test` | **all 9 pass**: `box-qualification-scratch.json`. `non_root_readonly`: `rootfs_writable: []` |
| `live_box_round_trip` through the HTTP box client and node router | **pass**: `live_box_round_trip.log` |
| numpy, pandas and matplotlib bar chart in a runsc box | `1.26.4 2.1.4`, `chart.png: PNG image data, 640 x 480` |
| `pip install --user` of a compiled package (default user base) | `/workspace/.local/lib/python3.12/site-packages/ujson.cpython-312-x86_64-linux-gnu.so` loaded |
| `pip install --user numpy` into `$HOME` (the demo's failure) | `/home/agent/.local/lib/python3.12/site-packages/numpy/__init__.py 3.0` loaded |
| `convert a.svg a.png`, `/usr/bin/convert a.svg b.jpg`, `convert a.svg c.pdf` | PNG 120 x 80, JPEG 120x80, PDF 1.7 |
| `pkill -f 'python3 -m http.server 8000'; … nohup python3 -m http.server 8000 &` (the kanban restart) | exit 0, the server answers 200. It was HTTP 500 before |
| A command that kills its own shell (`pkill -TERM -f arexec`, `pkill -KILL -f arexec`) | exit **143** / **137**, `stdout: "partial\n"`, `stderr: "oops\n[the shell was terminated by signal 15 (SIGTERM)]"` |
| Executables in `/tmp` and `$HOME`; the root filesystem | `exec-ok`; `touch /usr/x /etc/x /opt/x` all refused |
| `/proc/mounts` in the box | `none /tmp tmpfs rw,nosuid,size=1048576k,mode=1777`. gVisor does not list `nodev`, and the box has no `CAP_MKNOD` |

`before-after.log` shows the same box flags with and without `exec`, run with raw `docker run` on the scratch network:
- **Before:** numpy's C extension under `$HOME` fails with `failed to map segment from shared object`.
- **After:** it loads.
- **The old exec wrapper:** with the command in argv, `pkill -f 'python3 app.py'` ends with `docker exec` exit 143 and no output. That missing output is what the worker answered with HTTP 500.

Unit tests (`tests/boxes.rs`):
- `a_shell_killed_by_a_signal_is_an_exit_code_not_an_error`: the exit-status mapping
- `exec_wrapper_keeps_the_command_out_of_process_arguments`: runs the real wrapper on the host and checks:
  - `bash -lc` semantics
  - stdin after the command
  - `pkill -f` on the command's text matches nothing
- `box_container_is_locked_down`: now requires `rw,exec,nosuid,nodev` and forbids `noexec`

Gate: `cargo fmt --all --check`, `cargo clippy --locked --all-targets -D warnings` and `cargo test --locked` all pass, with 198 passed, 0 failed and 9 ignored (the live tests).

## Image size
| | before (`34192dbf…`, live) | after (`434b60c2…`) |
| --- | --- | --- |
| `docker image inspect .Size` | 1,558,695,376 B | 1,679,966,808 B (+121 MB, +7.8%) |
| `docker images` | 5.91 GB | 6.45 GB |
| apt layer | 948 MB | 1.36 GB |

The 42 new packages take 257 MiB installed. Ubuntu's matplotlib and pandas pull in `python3-scipy` (60 MiB) and `python3-sympy` (24 MiB) as hard dependencies. Most of the rest of the layer growth is precompiled `.pyc`: 120 MiB under `/usr/lib/python3`, up from 10 MiB. The box needs it precompiled because the root is read-only, so Python cannot write it at import time.

One caveat: an agent that runs `pip install --user --upgrade numpy` (numpy 2.x) shadows the system numpy that apt's pandas and scipy were built against. Ordinary installs leave it alone, because the system numpy satisfies their requirements.

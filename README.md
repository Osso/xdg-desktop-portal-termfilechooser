# xdg-desktop-portal-termfilechooser

`xdg-desktop-portal-termfilechooser` is a D-Bus implementation backend for the XDG Desktop Portal FileChooser interface. It opens a configured terminal and terminal file chooser instead of a graphical file picker.

The installed executable is `/usr/bin/xdg-desktop-portal-termfilechooser`. The portal descriptor is installed at `/usr/share/xdg-desktop-portal/portals/termfilechooser.portal`, and its D-Bus activation file at `/usr/share/dbus-1/services/org.freedesktop.impl.portal.desktop.termfilechooser.service`.

## Runtime flow

1. `xdg-desktop-portal` selects this backend when its portal policy maps `org.freedesktop.impl.portal.FileChooser` to `termfilechooser`.
2. D-Bus activates `/usr/bin/xdg-desktop-portal-termfilechooser`, which owns `org.freedesktop.impl.portal.desktop.termfilechooser` and exports `org.freedesktop.impl.portal.FileChooser` at `/org/freedesktop/portal/desktop`.
3. For `OpenFile`, `SaveFile`, or `SaveFiles`, the backend registers a request-specific `org.freedesktop.impl.portal.Request` object at the caller-provided request handle.
4. The backend creates a temporary chooser-output file, builds the configured chooser command with that file, and launches it through the configured terminal.
5. The terminal command receives the portal title, `--`, then the chooser command. The chooser command receives its final configured argument rewritten as `argument=/temporary/output/file`, followed by the starting directory.
6. After the terminal exits successfully, the backend reads nonempty newline-delimited paths from the temporary file. These paths are the only acceptance signal. A successful terminal exit without output is cancellation, not a selected path.
7. Absolute selected paths are converted to `file:` URIs and returned in the `uris` result key. The request object is removed after completion.

`Request.Close` marks the request cancelled. Each terminal starts in its own process group; cancellation kills the terminal and chooser descendants, reaps the terminal process, and returns cancellation rather than manufacturing a path from `current_file` or another UI hint.

## Supported portal methods

| Method | Behavior |
| --- | --- |
| `OpenFile` | Uses `current_folder` when supplied; otherwise uses `default_dir`. `multiple=true` permits multiple output paths; otherwise exactly one absolute path is required. With `directory=true`, every explicit chooser output must be an existing directory; file paths are rejected. |
| `SaveFile` | Uses `current_folder`, or the parent directory of `current_file`, as the start directory. The explicitly selected path is returned. |
| `SaveFiles` | Requires the `files` byte-array option. The chooser must explicitly return one existing directory. Each safe plain filename is appended to that directory and returned. |

Response codes are `0` for accepted output, `1` for cancellation (including no chooser output or `Request.Close`), and `2` for errors such as invalid options, invalid selections, configuration errors, or terminal failure. Errors return no result fields.

### URI normalization

Chooser output is handled as Unix path bytes, not UTF-8 text. A returned path must be absolute. URI creation percent-encodes raw path bytes except RFC 3986 unreserved characters and `/`; literal `%` becomes `%25`, and non-UTF-8 bytes are encoded (for example `0xff` becomes `%FF`). This prevents lossy Unicode conversion and avoids treating an existing percent sequence as already encoded.

## Configuration

Without `--config`, the backend reads:

```text
$XDG_CONFIG_HOME/xdg-desktop-portal-termfilechooser/config.toml
```

When `XDG_CONFIG_HOME` is unset, the platform configuration directory is normally `~/.config`, so the usual path is:

```text
~/.config/xdg-desktop-portal-termfilechooser/config.toml
```

A missing default-path config uses built-in defaults. A path passed with `--config` must exist and parse successfully. Terminal and chooser command strings must be nonempty and valid shell-style quoted argument lists; `default_dir` must be absolute.

Default configuration:

```toml
[filechooser]
terminal = "kitty --class file-chooser --title"
chooser = "yazi --chooser-file"
default_dir = "/home/your-user"
```

Custom configuration, including quoted executable paths and arguments:

```toml
[filechooser]
terminal = "'/opt/Terminal Apps/my terminal' --title"
chooser = "'/opt/File Managers/my chooser' '--mode=save files' --chooser-file"
default_dir = "/srv/files"
```

The TOML strings are parsed with shell-style quoting, not executed through a shell. In particular, quote paths or arguments containing spaces inside the TOML string. The configured chooser command must end with the chooser's output-file option name (for the default Yazi integration, `--chooser-file`); the backend appends `=/temporary/output/file` to that final argument and then adds the initial directory.

## Command-line behavior

- `--config PATH`: use exactly `PATH` instead of the default config location.
- `--loglevel error|warn|info|debug|trace`: fallback log filter when `RUST_LOG` is unavailable or invalid.
- `--replace`: allows D-Bus name replacement and asks the session bus to replace an existing backend owner. It does not alter portal policy.
- `--configure-portal`: updates the active desktop's portal-policy preference, prints the updated policy path, and exits without starting the D-Bus service.

`--configure-portal` follows xdg-desktop-portal's policy lookup order. If `XDG_DESKTOP_PORTAL_DIR` is set, only that override directory is searched. Otherwise it searches these locations in order:

1. `$XDG_CONFIG_HOME/xdg-desktop-portal`
2. each `$XDG_CONFIG_DIRS/xdg-desktop-portal` directory
3. `/etc/xdg-desktop-portal`
4. `$XDG_DATA_HOME/xdg-desktop-portal`
5. each `$XDG_DATA_DIRS/xdg-desktop-portal` directory
6. `/usr/share/xdg-desktop-portal`

Within each directory, every nonempty colon-separated `XDG_CURRENT_DESKTOP` token is tried in order, lowercased, as `DESKTOP-portals.conf`; then `portals.conf` is tried. A policy under `$XDG_CONFIG_HOME` is edited in place. A lower-priority policy is copied to `$XDG_CONFIG_HOME/xdg-desktop-portal/FIRST-DESKTOP-portals.conf` and edited there. The operation changes or adds only this line under `[preferred]`:

```ini
org.freedesktop.impl.portal.FileChooser=termfilechooser
```

Other policy sections, keys, values, and ordering are retained. The command fails rather than creating a policy from nothing when no existing policy can be found.

## Install and deploy

The root `PKGBUILD` is designed for the local `arch` CLI sandbox, which bind-mounts the clean repository at `/src` without network access. It builds and tests that exact checkout with Cargo `--locked --offline`, then packages the executable, portal descriptor, D-Bus service file, and this README. Runtime dependencies are `gcc-libs` and `xdg-desktop-portal>=1.17.1`; the minimum portal version provides modern `portals.conf` selection, so the deprecated `UseIn` fallback is intentionally absent. `kitty` and `yazi` are optional because they are only the default configured commands.

Run `./deploy.sh` from this repository to:

1. refuse a dirty checkout, then install the root `PKGBUILD` through `authsudo arch install`;
2. run `/usr/bin/xdg-desktop-portal-termfilechooser --configure-portal`;
3. stop a running installed backend; and
4. restart `xdg-desktop-portal.service` for the user.

## Validation commands

`./run-tests.sh all` runs, in order:

```text
cargo fmt --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --all-features --locked
```

`./run-tests.sh unit [test-filter]` runs the Cargo test command with an optional test filter. This document describes the commands only; it does not claim a test run passed.

## Dependency rationale

| Dependency | Reason |
| --- | --- |
| `zbus` | Session D-Bus connection, portal interface export, request objects, and D-Bus value types. |
| `serde` and `toml` | Deserialize the user TOML configuration. |
| `clap` | Parse `--replace`, `--loglevel`, `--config`, and `--configure-portal`. |
| `tracing` and `tracing-subscriber` | Structured runtime logging and `RUST_LOG`/CLI filter setup. |
| `dirs` | Locate the XDG config directory and home-directory default. |
| `percent-encoding` | Encode raw Unix path bytes safely in `file:` URIs. |
| `tempfile` | Secure temporary chooser-output files and atomic portal-policy replacement. |
| `shlex` | Parse quoted terminal and chooser command strings without invoking a shell. |
| `async-channel` | Deliver terminal worker completion or cancellation back to the async portal handler. |
| `rustix` | Create and signal an isolated process group so cancellation terminates the terminal and chooser descendants. |

## Current limitations

- File filters and choice widgets from portal options are not implemented.
- `current_name` is not used as a prefilled save name or UI hint.
- `SaveFile` delegates naming entirely to explicit chooser output; it does not append `current_name` or the basename of `current_file`.
- `SaveFiles` accepts only plain, single-component filenames and requires the chooser result to be an existing directory.
- Only the FileChooser portal interface is advertised. This backend is not a general portal implementation.
- The bundled descriptor makes the backend discoverable; portal policy must explicitly select `termfilechooser` for FileChooser.

## Repository map

- `src/main.rs`: chooser process execution, result validation, URI encoding, CLI parsing.
- `src/runtime.rs`: D-Bus interfaces, request lifecycle, cancellation, config loading, service registration.
- `src/config.rs`: TOML configuration defaults and command validation.
- `src/portal_config.rs`: portal-policy discovery and policy-preserving preference update.
- `src/tests.rs` and `src/tests/`: unit and integration-style behavior tests, including cancellation and portal-policy modules.
- `termfilechooser.portal`: portal descriptor.
- `org.freedesktop.impl.portal.desktop.termfilechooser.service`: D-Bus activation definition.
- `PKGBUILD`: local `arch` sandbox build and install recipe.
- `deploy.sh`: local package deployment and portal restart flow.
- `run-tests.sh`: repository validation runner.
- `docs/specs/file-chooser-backend.md`: feature contract.

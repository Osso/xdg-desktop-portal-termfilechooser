# File chooser backend

The FileChooser backend in `src/main.rs` and `src/runtime.rs` must provide terminal-backed XDG Desktop Portal file selection. Runtime details are documented in [the repository architecture and operations guide](../../README.md).

## What it must do

### Portal API

- [x] Export `org.freedesktop.impl.portal.FileChooser` with `OpenFile`, `SaveFile`, and `SaveFiles` at `/org/freedesktop/portal/desktop`.
- [x] Register a request-specific `org.freedesktop.impl.portal.Request` object for each active portal call and remove it when that call completes.
- [x] Honour `Request.Close` by cancelling the active interaction and returning response code `1` with no result fields.
- [x] Treat only nonempty paths written by the chooser to its explicit temporary output file as acceptance; terminal success without an output path must return response code `1`.
- [x] Return response code `0` and an `uris` string array after valid accepted selection; return `1` for cancellation; return `2` and no results for errors.

### Selection semantics

- [x] For `OpenFile`, accept one absolute path by default, permit multiple absolute paths only when `multiple=true`, and require existing directories when `directory=true`.
- [x] For `SaveFile`, start from `current_folder` when available, otherwise the parent of `current_file`, and return only the explicitly chosen output path.
- [x] For `SaveFiles`, require a valid `files` option, require a selected existing directory, and return that directory joined to each safe plain filename.
- [x] Reject empty, relative, and invalid-count chooser output instead of returning it to the portal caller.
- [x] Preserve raw Unix path bytes while forming a `file:` URI, percent-encoding bytes that are unsafe in a URI including literal percent signs and non-UTF-8 bytes.

### Configuration and lifecycle

- [x] Load the default TOML config from `$XDG_CONFIG_HOME/xdg-desktop-portal-termfilechooser/config.toml` when present, use defaults when that implicit file is absent, and reject invalid explicitly requested config files.
- [x] Parse quoted terminal and chooser command strings into argument vectors without shell execution, rejecting empty or malformed commands and a nonabsolute `default_dir`.
- [x] Support `--replace` for D-Bus name replacement without changing portal-policy preferences.
- [x] Support `--configure-portal` by matching xdg-desktop-portal's XDG directory and desktop-token precedence, changing only the FileChooser key in the selected existing policy, and preserving the other policy content.

## How it works

- [Runtime flow, configuration, packaging, and operational behavior](../../README.md)

## Implementation inventory

- `src/main.rs` — CLI arguments, chooser execution, selection validation, URI normalization, and response construction.
- `src/runtime.rs` — FileChooser and Request D-Bus interfaces, request registration, cancellation, configuration loading, and D-Bus ownership.
- `src/config.rs` — TOML config model, defaults, validation, and quoted command parsing.
- `src/portal_config.rs` — policy source selection and a policy-preserving FileChooser preference rewrite.
- `termfilechooser.portal` — portal descriptor advertising the FileChooser interface.
- `org.freedesktop.impl.portal.desktop.termfilechooser.service` — D-Bus activation for the installed backend.
- `packaging/PKGBUILD` — sandbox-compatible pushed-`master` build, check, package installation, and an `xdg-desktop-portal>=1.17.1` dependency for modern policy selection without deprecated `UseIn` fallback.
- `deploy.sh` — local package install, portal policy selection, backend stop, and user portal restart.
- `run-tests.sh` — formatting, lint, and test command runner.

## Tests asserting this spec

- `src/tests.rs`, `src/tests/cancellation_tests.rs`, and `src/tests/portal_config_tests.rs` — configuration defaults and validation; quoted commands; process-tree cancellation; D-Bus introspection; `Request.Close`; explicit-output acceptance; URI encoding; OpenFile, SaveFile, SaveFiles, and portal-policy behavior.
- `run-tests.sh` — executes formatter, Clippy with warnings denied, and the repository test suite under `all`; `unit` accepts a Cargo test filter.
- `.github/workflows/ci.yml` — runs formatting, Clippy, and tests in GitHub Actions.

Fresh verification on this cycle: `./run-tests.sh all` completed successfully with formatting clean, Clippy warnings denied, and 40 tests passed.

## Known gaps (current cycle)

No known gaps in the implemented contract above.

## Out of scope

- Portal file filters and choice widgets: their options are currently not implemented.
- `current_name` save-name prefill: the backend currently ignores this UI hint.
- General portal coverage beyond `org.freedesktop.impl.portal.FileChooser`: the descriptor intentionally advertises only FileChooser.
- Automatic policy creation without an existing desktop or generic policy: `--configure-portal` intentionally fails in that case to avoid replacing unknown desktop policy.

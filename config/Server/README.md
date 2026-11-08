# Server settings

Machine settings and client-facing values read by `asura-config`. The JSON files hold the
values and their meanings; evidence and acceptance belong in docs. This inventory was checked
against `crates/asura-config/src/lib.rs` and `src/directional.rs`, 2026-11-08.

| File | Concern | Reader / override |
|---|---|---|
| `../services.toml` | Processes, arguments, ports, health checks and logs | Supervisor; `--config FILE` |
| `defaults.json` | Placement inputs, client map kinds and creation-screen profession bitmap | `asura_config::defaults`; `ASURA_DEFAULTS` |
| `intervals.json` | Tick, ping, probe and world-freshness intervals | `asura_config::intervals`; `ASURA_INTERVALS` |
| `movement.json` | Client movement-report correction tolerance | `asura_config::movement`; `ASURA_MOVEMENT` |
| `directional_movement.json` | Directional-walker configuration | `asura_config::directional::current` |
| `spawns.json` | Scatter around an entry spawn | `asura_config::spawns`; `ASURA_SPAWNS` |
| `portals.json` | Half extent of point-doorway boundary boxes | `asura_config::portal_box_half_extent` |
| `account_settings.json` | Login account-settings payload | `asura_config::account_settings`; `ASURA_ACCOUNT_SETTINGS` |
| `file_check.json` | Pre-handshake file-check response | `asura_config::file_check`; `ASURA_FILE_CHECK` |

Most readers cache their result for the process lifetime; editing a file does not establish live
reload. File absence/error handling differs between readers. Defaults are required, while some
other readers still contain compiled fallbacks. That is an implementation gap against AGENTS.md
4a, not permission to add more fallbacks or a claim that all settings fail closed.

Values chosen by the server do not become immutable merely because they appear here. The
client-vocabulary meaning of `immutable` is defined in AGENTS.md; it is an editor note, not a
runtime authorization check.

World maps, entry lists, doorway geometry and behavior belong to the selected
[world pack](../World/README.md). Evidence for machine values stays in
[Client protocol](../../docs/reference/client-protocol.md) and the named research/recording;
[Project status](../../docs/work/project-status.md) owns acceptance.
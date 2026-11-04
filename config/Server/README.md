# Server settings

How this machine runs ASURA, as opposed to what the world contains.

Today the one file is `config/services.toml`, one level up: which services start, their arguments,
ports, health checks and log paths. It stays there until something needs a second server file.

What does not belong here: anything that describes the game. A map, a spawn point, a doorway or a
quest is world content, and lives in a package under `config/World/`
(`docs/38-layer-decisions.md`, D23, D24).

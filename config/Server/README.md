# Server settings

How this machine runs ASURA, as opposed to what the world contains.

Two files today:

- `config/services.toml`, one level up: which services start, their arguments, ports, health checks
  and log paths. It is the supervisor's own file.
- `config/Server/defaults.json`: the values a world falls back on when a row says nothing. One
  documented key today - `default_outpost`, the outpost a character enters when its row names neither a
  map nor a last outpost - and every key carries its own `doc` sentence, because JSON has no comments
  and a file of bare numbers cannot be reviewed. No key has a default in code: a missing file stops
  the socket layer at start-up (`docs/38-layer-decisions.md`, D34).
- `config/Server/intervals.json`: the intervals every program runs on - the world tick, the ping
  interval and value, the login path's probe timeout, and the freshness window the login path and
  the socket layer share. Read by `asura-config`, once per process; an environment variable still
  overrides a single value for an experiment (`docs/38-layer-decisions.md`, D33).

What does not belong here: anything that describes the game. A map, a spawn point, a doorway or a
quest is world content, and lives in a package under `config/World/`
(`docs/38-layer-decisions.md`, D23, D24).

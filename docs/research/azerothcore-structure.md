# The reference server's structure, read from its own tree

Read 2026-11-08 from the checkout at `C:\Users\Apo\Documents\GitHub\WowWarband`, which is
`github.com/mod-playerbots/azerothcore-wotlk` at `30a36ec4d` (2026-11-03, branch `main`). Every claim
below names the path it came from. This is a reading of that tree, not of our documents; where our
documents disagree with it, this file is the measurement and the disagreement is reported.

## What it is, in programs

Two applications, and nothing else listens to a client.

| program | entry point | what it starts, in its own order |
|---|---|---|
| `authserver` | `src/server/apps/authserver/Main.cpp` | config (`_ACORE_REALM_CONFIG`), logging, the auth database pool, `sSecretMgr`, `sRealmList` on a `RealmsStateUpdateDelay` (default 20 s), then `sAuthSocketMgr.StartNetwork(bind, 3724)` |
| `worldserver` | `src/server/apps/worldserver/Main.cpp` | config (`_ACORE_CORE_CONFIG`), logging, thread pool (`ThreadPool`, default 2), `sScriptMgr` with `AddScripts` and `AddModulesScripts`, metrics, `sSecretMgr`, `sWorld->SetInitialWorldSettings()`, then `sWorldSocketMgr.StartWorldNetwork(bind, worldPort, Network.Threads)`; on shutdown `sWorldSessionMgr->KickAll()`, `sBattlegroundMgr`, `sOutdoorPvPMgr`, `sMapMgr->UnloadAll()`, `sScriptMgr->OnAfterUnloadAllMaps()` |

The split is by conversation, not by database: auth answers login and hands out a realm entry; the
world owns the gameplay socket. One database service, several logical databases, one pool per
application.

## What it is, in libraries

`src/server/CMakeLists.txt` builds, in this order and only as needed:

| target | where | size |
|---|---|---|
| apps | `apps/` | the two `Main.cpp` above |
| `database` | `database/` | `DatabaseEnv`, `DatabaseLoader`, `AdhocStatement`, `Updater/` (`DBUpdater`, `UpdateFetcher`), `Logging/` |
| `shared` | `shared/` | `Network/` (`AsyncAcceptor`, `NetworkThread`, `Socket`, `SocketMgr`), `Packets/`, `Realms/` (`Realm`, `RealmList`), `Secrets/` (`SecretMgr`), `DataStores/`; 8 `.cpp` files |
| `game` | `game/` | a static library of 325 `.cpp` files, the whole simulation and its socket layer |
| `scripts` | `scripts/` | 598 `.cpp` files of content behaviour |

Plus `modules/` for out-of-tree code (`mod-playerbots`, `mod-ale`, `mod-aoe-loot`, and six more), and
`data/sql/` with `base`, `updates`, `archive`, `custom` - the content schema and its updater.

The important structural fact: **the socket layer is not an application.** Sockets and sessions live
inside the `game` library, at `game/Server/`: `WorldSocket`, `WorldSocketMgr`, `WorldSession`,
`WorldSessionMgr`, with `Packets/` and `Protocol/` beside them. The `Main.cpp` only wires the config
and starts the manager. That is why a world server can be one process with one listener and still have
the network as a subsystem with an owner.

## What the game library is made of

43 subsystem directories under `src/server/game/`, each a noun of the game rather than a technical
layer:

Accounts, Achievements, Addons, AI, ArenaSpectator, AuctionHouse, Autobroadcast, Battlefield,
Battlegrounds, Cache, Calendar, Chat, Combat, Conditions, DataStores, DungeonFinding, Entities, Events,
Globals, Grids, Groups, Guilds, Handlers, Instances, Loot, Mails, Maps, Misc, Miscellaneous, Modules,
Motd, Movement, OutdoorPvP, Petitions, Pools, Quests, Reputation, Scripting, Server, Skills, Spells,
Texts, Tickets, Time, Tools, Warden, Weather, World.

Two of those deserve naming because our tree has no equivalent:

- **`Entities/`** is the object model: `Object` -> `Unit` -> `Player`/`Creature`, plus `Item`,
  `GameObject`, `DynamicObject`, `Corpse`, `Pet`, `Totem`, `Transport`, `Vehicle`. Each entity is a
  file pair with its own update, its own state and its own persistence.
- **`Maps/` and `Grids/`** are the world's spatial model: `Map`, `MapInstanced`, `MapMgr`,
  `MapUpdater` (a producer/consumer queue, `PCQueue`, `activate(std::size_t num_threads)`), and
  `Grids/` with `MapGrid`, `MapGridManager`, `GridRefMgr`, a terrain loader and a separate
  collision-data file. `worldserver.conf.dist:1334` is `MapUpdate.Threads = 1`, and
  `Instance.UnloadDelay` (`:3375`) is how long an empty instance lives.

## What behaviour looks like

`Scripting/ScriptMgr.h` declares **345** `On...` hooks - `OnMapUpdate`, `OnCreatureUpdate`,
`OnCreatureAddWorld`, `OnPlayerLogin`, `OnSpellCheckCast`, `OnCreatureSelectLevel`,
`OnCreatureSaveToDB` and the rest. Content is written against those hooks in `scripts/` (598 files)
and in `modules/` (out of tree), and the engine never names a quest, a dungeon or a boss. That is the
script-manager position our plan's `asura-store`-style move and ADR 0014 already point at.

## What the configuration looks like

`env/dist/etc/worldserver.conf.dist` and `authserver.conf.dist` are the shipped files, one per
program, with sections (`[worldserver]`, `[worldserver]` keys from line 4 on). The keys that decide the
machine: `Network.Threads = 1` (`:365`), `ThreadPool = 2` (`:502`), `MapUpdate.Threads = 1`
(`:1334`), `Instance.UnloadDelay` (`:3375`). Each program reads only its own file, and a key it does
not know is a start-up error unless it is one of the documented exceptions.

## What this means for a Guild Wars clone

The skeleton to copy is: **two programs, a shared core, a database layer, a game library organised by
game noun, a script layer with hooks, a modules directory, one config file per program, and a SQL
content tree with an updater.** The parts that do not carry over, because Guild Wars has no such
thing, are `Battlegrounds`, `ArenaSpectator`, `Warden`, `AuctionHouse`, `Mails`, `Vehicle`, `Pet`,
`Totem`, `Corpse` and `Transport` (checked in the tree above: the first five exist in the reference and
the last five are inside `Entities/` or absent as a directory).

What has to be added, because Guild Wars needs it and the reference has no equivalent: the file check
and version exchange before login, a portal web service for identity, party and hero/henchman
management, and the map-content pack our project reads from `Gw.dat` rather than from SQL rows.

The consequence for our own plan is a re-cut, not a rewrite: the world we build is a **new program**
with this library shape - `shared` (sockets, packets, secrets), `database`, `game/` by noun,
`scripts`, `conf` - and the existing `asura-world` moves into it cut by cut and is then deleted.

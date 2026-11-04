# 39 - The map list

Every map the client names, kept here so it is never lost again: 278 outposts and 378 explorables,
keyed by map id. This is a **reference**, not configuration and not a decision: it places nothing,
and nothing in `crates/` reads it. What it answers is one question - what is map 148 called.

## Where it comes from

- The names are the client's own tables, read through the Py4GW link: `py4gw/enums_src/map_enums.py`
  in Py4GW Stealth (`outposts`, `explorables`, `outpost_name_to_id`, `explorable_name_to_id`,
  `name_to_map_id`), transcribed there from Reforged's `Py4GWCoreLib/enums_src/Map_enums.py`.
- They are **complete tables**, not a per-map lookup: every outpost and every explorable has a row, and
  the reverse maps exist, so an id names a map and a name names an id with no client running.
- Campaign and region are **not** in these tables. They are one `AreaInfo` record per map id
  (`campaign`, `region`, `type`, `file_id`; `py4gw/context/instance_info_context.py`), read from a
  client through `GetMapInfo(map_id)`. This list names maps; it does not place them in a campaign or a
  region (`docs/22-world-model.md`, `docs/38-layer-decisions.md` D36).
- The table is ASCII as written; no name was altered, shortened or reordered. Ids are sorted
  numerically inside each table, and an id that appears in both tables appears in both.

## How to read the `pack` column

`yes` means `config/World/Vanilla/maps/<id>/` exists - the map has a row in the instance table this
server sends. It says nothing about the name being right; only that the map is one this server has seen
a load for.

## Outposts (278)

| id | name | pack |
|---|---|---|
| 4 | Guild Hall - Warrior's Isle | yes |
| 5 | Guild Hall - Hunter's Isle | yes |
| 6 | Guild Hall - Wizard's Isle | yes |
| 10 | Bloodstone Fen outpost |  |
| 11 | The Wilds outpost |  |
| 12 | Aurora Glade outpost |  |
| 14 | Gates of Kryta outpost |  |
| 15 | D'Alessio Seaboard outpost |  |
| 16 | Divinity Coast outpost |  |
| 19 | Sanctum Cay outpost |  |
| 20 | Droknar's Forge |  |
| 21 | The Frost Gate outpost |  |
| 22 | Ice Caves of Sorrow outpost |  |
| 23 | Thunderhead Keep outpost |  |
| 24 | Iron Mines of Moladune outpost |  |
| 25 | Borlis Pass outpost |  |
| 28 | The Great Northern Wall outpost |  |
| 29 | Fort Ranik outpost |  |
| 30 | Ruins of Surmia outpost |  |
| 32 | Nolani Academy outpost |  |
| 35 | Ember Light Camp |  |
| 36 | Grendich Courthouse |  |
| 37 | Glint' Challenge |  |
| 38 | Augury Rock outpost |  |
| 39 | Sardelac Sanitarium |  |
| 40 | Piken Square |  |
| 49 | Henge of Denravi |  |
| 51 | Senjis Corner |  |
| 52 | Guild Hall - Burning Isle | yes |
| 55 | Lions Arch |  |
| 57 | Bergen Hot Springs |  |
| 65 | D'Alessio Arena | yes |
| 73 | Riverside Province outpost |  |
| 75 | The Hall of Heroes Arena | yes |
| 76 | Broken Tower Arena | yes |
| 77 | House zu Heltzer |  |
| 78 | The Courtyard Arena | yes |
| 79 | Unholy Temples Area | yes |
| 80 | Burial Mounds Arena | yes |
| 81 | Ascalon City |  |
| 82 | Tomb of the Primeval Kings |  |
| 83 | The Vault Arena | yes |
| 84 | The Underworld Arena | yes |
| 85 | Ascalon Arena outpost |  |
| 86 | Sacred Temples Arena | yes |
| 109 | The Amnoon Oasis |  |
| 116 | Dunes of Despair outpost |  |
| 117 | Thirsty River outpost |  |
| 118 | Elona Reach outpost |  |
| 120 | The Dragon's Lair outpost |  |
| 122 | Ring of Fire outpost |  |
| 123 | Abaddon's Mouth outpost |  |
| 124 | Hell's Precipice outpost |  |
| 126 | Golden Gates |  |
| 129 | Lutgardis Conservatory |  |
| 130 | Vasburg Armory |  |
| 131 | Serenity Temple |  |
| 132 | Ice Tooth Cave |  |
| 133 | Beacons Perch |  |
| 134 | Yaks Bend |  |
| 135 | Frontier Gate |  |
| 136 | Beetletun |  |
| 137 | Fishermens Haven |  |
| 138 | Temple of the Ages |  |
| 139 | Ventaris Refuge |  |
| 140 | Druids Overlook |  |
| 141 | Maguuma Stade |  |
| 142 | Quarrel Falls |  |
| 150 | Ascalon Academy PvP battle | yes |
| 152 | Heroes Audience |  |
| 153 | Seekers Passage |  |
| 154 | Destinys Gorge |  |
| 155 | Camp Rankor |  |
| 156 | The Granite Citadel |  |
| 157 | Marhans Grotto |  |
| 158 | Port Sledge |  |
| 159 | Copperhammer Mines |  |
| 163 | Pre-Searing: The Barradin Estate | yes |
| 164 | Pre-Searing: Ashford Abbey | yes |
| 165 | Pre-Searing: Foibles Fair | yes |
| 166 | Pre-Searing: Fort Ranik | yes |
| 176 | Guild Hall - Frozen Isle | yes |
| 177 | Guild Hall - Nomad's Isle | yes |
| 178 | Guild Hall - Druid's Isle | yes |
| 179 | Guild Hall - Isle of the Dead | yes |
| 180 | Fort Koga | yes |
| 181 | Shiverpeak Arena outpost |  |
| 182 | Amnoon Arena | yes |
| 183 | Deldrimor Arena | yes |
| 184 | The Crag | yes |
| 188 | Random Arenas outpost | yes |
| 189 | Team Arenas outpost |  |
| 193 | Cavalon |  |
| 194 | Kaineng Center |  |
| 204 | Unwaking Waters - Kurzick |  |
| 206 | Deldrimor War Camp |  |
| 208 | Heroes' Crypt | yes |
| 213 | Zen Daijun outpost |  |
| 214 | Minister Chos Estate outpost |  |
| 215 | Vizunah Square |  |
| 216 | Nahpui Quarter outpost |  |
| 217 | Tahnnakai Temple outpost |  |
| 218 | Arborstone outpost |  |
| 219 | Boreas Seabed outpost |  |
| 220 | Sunjiang District outpost |  |
| 221 | Fort Aspenwood |  |
| 222 | The Eternal Grove outpost |  |
| 223 | The Jade Quarry |  |
| 224 | Gyala Hatchery outpost |  |
| 225 | Raisu Palace outpost |  |
| 226 | Imperial Sanctum outpost |  |
| 228 | Grenz Frontier |  |
| 230 | Amatz Basin outpost |  |
| 234 | The Aurios Mines outpost |  |
| 242 | Shing Jea Monastery |  |
| 243 | Shing Jea Arena outpost |  |
| 248 | Great Temple of Balthazar |  |
| 249 | Tsumei Village |  |
| 250 | Seitung Harbor |  |
| 251 | Ran Musu Gardens |  |
| 253 | Dwayna Vs Grenth outpost |  |
| 266 | Urgoz's Warren outpost |  |
| 272 | Altrumm Ruins outpost |  |
| 273 | Zos Shivros Channel outpost |  |
| 274 | Dragons Throat outpost |  |
| 275 | Guild Hall - Isle of Weeping Stone | yes |
| 276 | Guild Hall - Isle of Jade | yes |
| 277 | Harvest Temple |  |
| 278 | Breaker Hollow |  |
| 279 | Leviathan Pits |  |
| 281 | Zaishen Challenge outpost | yes |
| 282 | Zaishen Elite outpost | yes |
| 283 | Maatu Keep |  |
| 284 | Zin Ku Corridor |  |
| 286 | Brauer Academy |  |
| 287 | Durheim Archives |  |
| 288 | Bai Paasu Reach |  |
| 289 | Seafarer's Rest |  |
| 291 | Vizunah Square Local Quarter |  |
| 292 | Vizunah Square Foreign Quarter |  |
| 293 | Fort Aspenwood - Luxon |  |
| 294 | Fort Aspenwood - Kurzick |  |
| 295 | The Jade Quarry - Luxon |  |
| 296 | The Jade Quarry - Kurzick |  |
| 297 | Unwaking Waters Luxon |  |
| 298 | Unwaking Waters Kurzick |  |
| 300 | Etnaran Keys | yes |
| 303 | The Marketplace |  |
| 307 | The Deep outpost |  |
| 308 | Ascalon Arena | yes |
| 309 | Annihilation |  |
| 310 | Kill Count Training |  |
| 311 | Annihilation |  |
| 312 | Obelisk Annihilation Training |  |
| 314 | Shiverpeak Arena | yes |
| 318 | D'Alessio Arena | yes |
| 319 | Amnoon Arena | yes |
| 320 | Fort Koga | yes |
| 321 | Heroes' Crypt | yes |
| 322 | Shiverpeak Arena | yes |
| 328 | Saltspray Beach - Luxon | yes |
| 329 | Saltspray Beach - Kurzick | yes |
| 330 | Heroes Ascent outpost | yes |
| 331 | Grenz Frontier - Luxon | yes |
| 332 | Grenz Frontier - Kurzick | yes |
| 333 | The Ancestral Lands - Luxon |  |
| 334 | The Ancestral Lands - Kurzick | yes |
| 335 | Etnaran Keys - Luxon | yes |
| 336 | Etnaran Keys - Kurzick | yes |
| 337 | Kaanai Canyon - Luxon | yes |
| 338 | Kaanai Canyon - Kurzick | yes |
| 339 | D'Alessio Arena | yes |
| 340 | Amnoon Arena | yes |
| 341 | Fort Koga | yes |
| 342 | Heroes' Crypt | yes |
| 343 | Shiverpeak Arena | yes |
| 348 | Tanglewood Copse |  |
| 349 | Saint Anjeka's Shrine |  |
| 350 | Eredon Terrace |  |
| 352 | Brawler's Pit | yes |
| 353 | Petrified Arena | yes |
| 354 | Seabed Arena | yes |
| 359 | Guild Hall - Imperial Isle | yes |
| 360 | Guild Hall - Isle of Meditation | yes |
| 368 | Dragon Arena outpost |  |
| 376 | Camp Hojanu |  |
| 378 | Wehhan Terraces |  |
| 381 | Yohlon Haven |  |
| 387 | Sunspear Sanctuary |  |
| 388 | Aspenwood Gate - Kurzick |  |
| 389 | Aspenwood Gate - Luxon |  |
| 390 | Jade Flats Kurzick |  |
| 391 | Jade Flats Luxon |  |
| 393 | Chantry of Secrets |  |
| 396 | Mihanu Township |  |
| 398 | Basalt Grotto |  |
| 403 | Honur Hill |  |
| 407 | Yahnur Market |  |
| 414 | The Kodash Bazaar |  |
| 421 | Venta Cemetery outpost |  |
| 424 | Kodonur Crossroads outpost |  |
| 425 | Rilohn Refuge outpost |  |
| 426 | Pogahn Passage outpost |  |
| 427 | Moddok Crevice outpost |  |
| 428 | Tihark Orchard outpost |  |
| 431 | Sunspear Great Hall |  |
| 433 | Dzagonur Bastion outpost |  |
| 434 | Dasha Vestibule outpost |  |
| 435 | Grand Court of Sebelkeh outpost |  |
| 438 | Bone Palace |  |
| 440 | The Mouth of Torment |  |
| 442 | Lair of the Forgotten |  |
| 445 | The Ebony Citadel of Mallyx |  |
| 449 | Kamadan Jewel of Istan |  |
| 450 | Gate of Torment |  |
| 457 | Beknur Harbor |  |
| 467 | Rollerbeetle Racing outpost |  |
| 469 | Gate of Fear |  |
| 473 | Gate of Secrets |  |
| 474 | Gate of Anguish |  |
| 476 | Jennurs Horde outpost |  |
| 477 | Nundu Bay outpost |  |
| 478 | Gate of Desolation outpost |  |
| 479 | Champions Dawn |  |
| 480 | Ruins of Morah outpost |  |
| 487 | Beknur Harbor |  |
| 489 | Kodlonu Hamlet |  |
| 491 | Jokanur Diggings outpost |  |
| 492 | Blacktide Den outpost |  |
| 493 | Consulate Docks outpost |  |
| 494 | Gate of Pain outpost |  |
| 495 | Gate of Madness outpost |  |
| 496 | Abaddons Gate outpost |  |
| 497 | Sunspear Arena outpost |  |
| 502 | The Astralarium |  |
| 504 | Churranu Island Arena |  |
| 529 | Guild Hall - Uncharted Isle | yes |
| 530 | Guild Hall - Isle of Wurms | yes |
| 536 | Sunspear Arena | yes |
| 537 | Guild Hall - Corrupted Isle | yes |
| 538 | Guild Hall - Isle of Solitude | yes |
| 544 | Chahbek Village outpost |  |
| 545 | Remains of Sahlahja outpost |  |
| 549 | Hero Battles outpost |  |
| 554 | Dajkah Inlet outpost |  |
| 555 | The Shadow Nexus outpost |  |
| 559 | Gate of the Nightfallen Lands |  |
| 624 | Vlox's Falls |  |
| 638 | Gadd's Encampment |  |
| 639 | Umbral Grotto |  |
| 640 | Rata Sum |  |
| 641 | Tarnished Haven |  |
| 642 | Eye of the North outpost |  |
| 643 | Sifhalla |  |
| 644 | Gunnar's Hold |  |
| 645 | Olafstead |  |
| 648 | Doomlore Shrine |  |
| 650 | Longeyes Ledge |  |
| 652 | Central Transfer Chamber |  |
| 675 | Boreal Station |  |
| 721 | Costume Brawl outpost |  |
| 795 | Zaishen Menagerie outpost |  |
| 796 | Codex Arena outpost | yes |
| 808 | Lions Arch - Halloween |  |
| 809 | Lions Arch - Wintersday |  |
| 810 | Lions Arch - Canthan New Year |  |
| 811 | Ascalon City - Wintersday |  |
| 812 | Droknars Forge - Halloween |  |
| 813 | Droknars Forge - Wintersday |  |
| 814 | Tomb of the Primeval Kings - Halloween |  |
| 815 | Shing Jea Monastery - Dragon Festival |  |
| 816 | Shing Jea Monastery - Canthan New Year |  |
| 817 | Kaineng Center |  |
| 818 | Kamadan Jewel of Istan - Halloween |  |
| 819 | Kamadan Jewel of Istan - Wintersday |  |
| 820 | Kamadan Jewel of Istan - Canthan New Year |  |
| 821 | Eye of the North outpost - Wintersday |  |
| 857 | Embark Beach |  |

## Explorables (378)

| id | name | pack |
|---|---|---|
| 7 | Warrior's Isle |  |
| 8 | Hunter's Isle |  |
| 9 | Wizard's Isle |  |
| 13 | Diessa Lowlands |  |
| 17 | Talmark Wilderness |  |
| 18 | The Black Curtain |  |
| 26 | Talus Chute |  |
| 27 | Griffon's Mouth |  |
| 31 | Xaquang Skyway |  |
| 33 | Old Ascalon |  |
| 34 | The Fissure of Woe |  |
| 41 | Sage Lands |  |
| 42 | Mamnoon Lagoon |  |
| 43 | Silverwood |  |
| 44 | Ettin's Back |  |
| 45 | Reed Bog |  |
| 46 | The Falls |  |
| 47 | Dry Top |  |
| 48 | Tangle Root |  |
| 53 | Tears of the Fallen |  |
| 54 | Scoundrel's Rise |  |
| 56 | Cursed Lands |  |
| 58 | North Kryta Province |  |
| 59 | Nebo Terrace |  |
| 60 | Majesty's Rest |  |
| 61 | Twin Serpent Lakes |  |
| 62 | Watchtower Coast |  |
| 63 | Stingray Strand |  |
| 64 | Kessex Peak |  |
| 67 | Burning Isle |  |
| 68 | Frozen Isle |  |
| 69 | Nomad's Isle |  |
| 70 | Druid's Isle |  |
| 71 | Isle of the Dead (guild hall) |  |
| 72 | The Underworld |  |
| 87 | Icedome |  |
| 88 | Iron Horse Mine |  |
| 89 | Anvil Rock |  |
| 90 | Lornar's Pass |  |
| 91 | Snake Dance |  |
| 92 | Tasca's Demise |  |
| 93 | Spearhead Peak |  |
| 94 | Ice Floe |  |
| 95 | Witman's Folly |  |
| 96 | Mineral Springs |  |
| 97 | Dreadnought's Drift |  |
| 98 | Frozen Forest |  |
| 99 | Traveler's Vale |  |
| 100 | Deldrimor Bowl |  |
| 101 | Regent Valley |  |
| 102 | The Breach |  |
| 103 | Ascalon Foothills |  |
| 104 | Pockmark Flats |  |
| 105 | Dragon's Gullet |  |
| 106 | Flame Temple Corridor |  |
| 107 | Eastern Frontier |  |
| 108 | The Scar |  |
| 110 | Diviner's Ascent |  |
| 111 | Vulture Drifts |  |
| 112 | The Arid Sea |  |
| 113 | Prophet's Path |  |
| 114 | Salt Flats |  |
| 115 | Skyward Reach |  |
| 119 | Augury Rock mission |  |
| 121 | Perdition Rock |  |
| 127 | Scarred Earth | yes |
| 128 | The Eternal Grove (explorable area) |  |
| 144 | Gyala Hatchery (explorable area) |  |
| 145 | The Catacombs | yes |
| 146 | Lakeside County | yes |
| 147 | The Northlands | yes |
| 148 | Ascalon City (pre-Searing) | yes |
| 149 | Ascalon Academy | yes |
| 151 | Ascalon Academy | yes |
| 160 | Green Hills County | yes |
| 161 | Wizard's Folly | yes |
| 162 | Regent Valley (pre-Searing) | yes |
| 167 | Burning Isle | yes |
| 168 | Druid's Isle | yes |
| 170 | Frozen Isle | yes |
| 171 | Warrior's Isle | yes |
| 172 | Hunter's Isle |  |
| 173 | Wizard's Isle | yes |
| 174 | Nomad's Isle | yes |
| 175 | Isle of the Dead | yes |
| 190 | Sorrow's Furnace |  |
| 191 | Grenth's Footprint |  |
| 195 | Drazach Thicket |  |
| 196 | Jaya Bluffs |  |
| 197 | Shenzun Tunnels |  |
| 198 | Archipelagos |  |
| 199 | Maishang Hills |  |
| 200 | Mount Qinkai |  |
| 201 | Melandru's Hope |  |
| 202 | Rhea's Crater |  |
| 203 | Silent Surf |  |
| 205 | Morostav Trail |  |
| 209 | Mourning Veil Falls |  |
| 210 | Ferndale |  |
| 211 | Pongmei Valley |  |
| 212 | Monastery Overlook |  |
| 227 | Unwaking Waters (explorable area) |  |
| 232 | Shadow's Passage |  |
| 233 | Raisu Palace (explorable area) |  |
| 235 | Panjiang Peninsula |  |
| 236 | Kinya Province |  |
| 237 | Haiju Lagoon |  |
| 238 | Sunqua Vale |  |
| 239 | Wajjun Bazaar |  |
| 240 | Bukdek Byway |  |
| 241 | The Undercity |  |
| 244 | Arborstone (explorable area) |  |
| 245 | Minister Chos Estate (explorable area) |  |
| 246 | Zen Daijun (explorable area) |  |
| 247 | Boreas Seabed (explorable area) |  |
| 252 | Linnok Courtyard |  |
| 256 | Sunjiang District (explorable area) |  |
| 265 | Nahpui Quarter (explorable area) |  |
| 269 | Tahnnakai Temple (explorable area) |  |
| 280 | Isle of the Nameless |  |
| 285 | Monastery Overlook |  |
| 290 | Bejunkan Pier |  |
| 301 | Raisu Pavilion |  |
| 302 | Kaineng Docks |  |
| 313 | Saoshang Trail |  |
| 344 | The Hall of Heroes |  |
| 345 | The Courtyard |  |
| 346 | Scarred Earth |  |
| 347 | The Underworld (explorable area) |  |
| 351 | Divine Path |  |
| 361 | Isle of Weeping Stone |  |
| 362 | Isle of Jade |  |
| 363 | Imperial Isle |  |
| 364 | Isle of Meditation |  |
| 369 | Jahai Bluffs |  |
| 371 | Marga Coast |  |
| 373 | Sunward Marches |  |
| 375 | Barbarous Shore |  |
| 377 | Bahdok Caverns |  |
| 379 | Dejarin Estate |  |
| 380 | Arkjok Ward |  |
| 382 | Gandara, the Moon Fortress |  |
| 384 | The Floodplain of Mahnkelon |  |
| 385 | Lion's Arch (Sunspears in Kryta) |  |
| 386 | Turai's Procession |  |
| 392 | Yatendi Canyons |  |
| 394 | Garden of Seborhin |  |
| 395 | Holdings of Chokhin |  |
| 397 | Vehjin Mines |  |
| 399 | Forum Highlands |  |
| 400 | Kaineng Center (Sunspears in Cantha) |  |
| 402 | Resplendent Makuun |  |
| 404 | Wilderness of Bahdza |  |
| 406 | Vehtendi Valley |  |
| 413 | The Hidden City of Ahdashim |  |
| 415 | Lion's Gate |  |
| 419 | The Mirror of Lyss |  |
| 420 | Secure the Refuge |  |
| 422 | Bad Tide Rising#NPCs - Kamadan, Jewel of Istan (explorable area) |  |
| 423 | The Tribunal |  |
| 429 | Consulate |  |
| 430 | Plains of Jarin |  |
| 432 | Cliffs of Dohjok |  |
| 436 | Command Post |  |
| 437 | Joko's Domain |  |
| 439 | The Ruptured Heart |  |
| 441 | The Shattered Ravines |  |
| 443 | Poisoned Outcrops |  |
| 444 | The Sulfurous Wastes |  |
| 446 | The Alkali Pan |  |
| 447 | A Land of Heroes |  |
| 448 | Crystal Overlook |  |
| 455 | Nightfallen Garden |  |
| 456 | Churrhir Fields |  |
| 461 | The Underworld |  |
| 462 | Heart of Abaddon |  |
| 463 | The Underworld |  |
| 465 | Nightfallen Jahai |  |
| 466 | Depths of Madness |  |
| 468 | Domain of Fear |  |
| 470 | Domain of Pain |  |
| 471 | Bloodstone Fen (explorable area) |  |
| 472 | Domain of Secrets |  |
| 474 | Domain of Anguish |  |
| 481 | Fahranur, The First City |  |
| 482 | Bjora Marches |  |
| 483 | Zehlon Reach |  |
| 484 | Lahtenda Bog |  |
| 485 | Arbor Bay |  |
| 486 | Issnur Isles |  |
| 488 | Mehtani Keys |  |
| 490 | Island of Shehkah |  |
| 499 | Ice Cliff Chasms |  |
| 500 | Bokka Amphitheatre |  |
| 501 | Riven Earth |  |
| 503 | Throne of Secrets |  |
| 505 | Shing Jea Monastery (mission) |  |
| 506 | Haiju Lagoon (mission) |  |
| 507 | Jaya Bluffs (mission) |  |
| 508 | Seitung Harbor (mission) |  |
| 509 | Tsumei Village (mission) |  |
| 510 | Seitung Harbor (mission 2) |  |
| 511 | Tsumei Village (mission 2) |  |
| 513 | Drakkar Lake |  |
| 531 | Uncharted Isle |  |
| 532 | Isle of Wurms |  |
| 539 | Corrupted Isle |  |
| 540 | Isle of Solitude |  |
| 543 | Sun Docks |  |
| 546 | Jaga Moraine |  |
| 547 | Bombardment | yes |
| 548 | Norrhart Domains |  |
| 550 | The Beachhead | yes |
| 551 | The Crossing | yes |
| 552 | Desert Sands | yes |
| 553 | Varajar Fells |  |
| 558 | Sparkfly Swamp |  |
| 560 | Cathedral of Flames (level 1) |  |
| 561 | The Troubled Keeper |  |
| 566 | Verdant Cascades |  |
| 567 | Cathedral of Flames (level 2) |  |
| 568 | Cathedral of Flames (level 3) |  |
| 569 | Magus Stones |  |
| 570 | Catacombs of Kathandrax (level 1) |  |
| 571 | Catacombs of Kathandrax (level 2) |  |
| 572 | Alcazia Tangle |  |
| 573 | Rragar's Menagerie (level 1) |  |
| 574 | Rragar's Menagerie (level 2) |  |
| 575 | Rragar's Menagerie (level 3) |  |
| 576 | Ooza Pit |  |
| 577 | Slaver's Exile |  |
| 578 | Oola's Lab (level 1) |  |
| 579 | Oola's Lab (level 2) |  |
| 580 | Oola's Lab (level 3) |  |
| 581 | Shards of Oor (level 1) |  |
| 582 | Shards of Oor (level 2) |  |
| 583 | Shards of Oor (level 3) |  |
| 584 | Arachni's Haunt |  |
| 585 | Arachni's Haunt |  |
| 593 | Fetid River | yes |
| 596 | Forgotten Shrines | yes |
| 598 | Antechamber | yes |
| 604 | Vloxen Excavations (level 1) |  |
| 605 | Vloxen Excavations (level 2) |  |
| 606 | Vloxen Excavations (level 3) |  |
| 607 | Heart of the Shiverpeaks (level 1) |  |
| 608 | Heart of the Shiverpeaks (level 2) |  |
| 609 | Heart of the Shiverpeaks (level 3) |  |
| 612 | Bloodstone Caves (level 1) |  |
| 613 | Bloodstone Caves (level 2) |  |
| 614 | Bloodstone Caves (level 3) |  |
| 615 | Bogroot Growths (level 1) |  |
| 616 | Bogroot Growths (level 2) |  |
| 617 | Raven's Point (level 1) |  |
| 618 | Raven's Point (level 2) |  |
| 619 | Raven's Point (level 3) |  |
| 620 | Slaver's Exile (level 1) |  |
| 621 | Slaver's Exile (level 2) |  |
| 622 | Slaver's Exile (level 3) |  |
| 623 | Slaver's Exile (level 4) |  |
| 625 | Battledepths |  |
| 628 | Sepulchre of Dragrimmar (level 1) |  |
| 629 | Sepulchre of Dragrimmar (level 2) |  |
| 630 | Frostmaw's Burrows (level 1) |  |
| 631 | Frostmaw's Burrows (level 2) |  |
| 632 | Frostmaw's Burrows (level 3) |  |
| 633 | Frostmaw's Burrows (level 4) |  |
| 634 | Frostmaw's Burrows (level 5) |  |
| 635 | Darkrime Delves (level 1) |  |
| 636 | Darkrime Delves (level 2) |  |
| 637 | Darkrime Delves (level 3) |  |
| 646 | Hall of Monuments |  |
| 647 | Dalada Uplands |  |
| 649 | Grothmar Wardowns |  |
| 651 | Sacnoth Valley |  |
| 653 | Curse of the Nornbear |  |
| 654 | Blood Washes Blood |  |
| 655 | A Gate Too Far (level 1) |  |
| 656 | A Gate Too Far (level 2) |  |
| 657 | A Gate Too Far (level 3) |  |
| 658 | The Elusive Golemancer (level 1) |  |
| 659 | The Elusive Golemancer (level 2) |  |
| 660 | The Elusive Golemancer (level 3) |  |
| 661 | Finding the Bloodstone (level 1) |  |
| 662 | Finding the Bloodstone (level 2) |  |
| 663 | Finding the Bloodstone (level 3) |  |
| 664 | Genius Operated Living Enchanted Manifestation |  |
| 665 | Against the Charr |  |
| 666 | Warband of Brothers (level 1) |  |
| 667 | Warband of Brothers (level 2) |  |
| 668 | Warband of Brothers (level 3) |  |
| 669 | Assault on the Stronghold |  |
| 670 | Destruction's Depths (level 1) |  |
| 671 | Destruction's Depths (level 2) |  |
| 672 | Destruction's Depths (level 3) |  |
| 673 | A Time for Heroes |  |
| 674 | Warband Training |  |
| 676 | Catacombs of Kathandrax |  |
| 678 | Attack of the Nornbear |  |
| 686 | Polymock Coliseum |  |
| 687 | Polymock Glacier |  |
| 688 | Polymock Crossing |  |
| 690 | Cold as Ice |  |
| 691 | Beneath Lion's Arch |  |
| 692 | Tunnels Below Cantha |  |
| 693 | Caverns Below Kamadan |  |
| 695 | Service: In Defense of the Eye |  |
| 696 | Mano a Norn-o |  |
| 697 | Service: Practice, Dummy |  |
| 698 | Hero Tutorial |  |
| 700 | The Norn Fighting Tournament |  |
| 701 | Secret Lair of the Snowmen |  |
| 702 | Norn Brawling Championship |  |
| 703 | Kilroy's Punchout Training |  |
| 704 | Fronis Irontoe's Lair |  |
| 705 | The Justiciar's End |  |
| 707 | The Great Norn Alemoot |  |
| 708 | Varajar Fells(unknow) |  |
| 710 | Epilogue |  |
| 711 | Insidious Remnants |  |
| 717 | Attack on Jalis's Camp |  |
| 722 | Whitefury Rapids | yes |
| 723 | Kysten Shore | yes |
| 724 | Deepway Ruins | yes |
| 725 | Plikkup Works | yes |
| 726 | Kilroy's Punchout Tournament |  |
| 727 | Special Ops: Flame Temple Corridor |  |
| 728 | Special Ops: Dragon's Gullet |  |
| 729 | Special Ops: Grendich Courthouse |  |
| 770 | The Tengu Accords |  |
| 771 | The Battle of Jahai |  |
| 772 | The Flight North |  |
| 773 | The Rise of the White Mantle |  |
| 781 | Secret Lair of the Snowmen |  |
| 782 | Secret Lair of the Snowmen |  |
| 783 | Droknar's Forge (explorable area) |  |
| 788 | Deactivating R.O.X.#NPCs |  |
| 789 | Deactivating P.O.X. |  |
| 790 | Deactivating N.O.X. |  |
| 791 | Secret Underground Lair |  |
| 792 | Golem Tutorial Simulation |  |
| 793 | Snowball Dominance |  |
| 794 | Zaishen Menagerie Grounds |  |
| 806 | The Underworld (Something Wicked This Way Comes) |  |
| 807 | The Underworld (Don't Fear the Reapers) |  |
| 837 | Talmark Wilderness (War in Kryta) |  |
| 838 | Trial of Zinn |  |
| 839 | Divinity Coast (explorable area) |  |
| 840 | Lion's Arch Keep |  |
| 841 | D'Alessio Seaboard (explorable area) |  |
| 842 | The Battle for Lion's Arch (explorable area) |  |
| 843 | Riverside Province (explorable area) |  |
| 844 | Lion's Arch (War in Kryta) |  |
| 845 | The Mausoleum |  |
| 846 | Rise |  |
| 847 | Shadows in the Jungle |  |
| 848 | A Vengeance of Blades |  |
| 849 | Auspicious Beginnings |  |
| 854 | Olafstead (explorable area) |  |
| 855 | The Great Snowball Fight of the Gods (Operation: Crush Spirits) |  |
| 856 | The Great Snowball Fight of the Gods (Fighting in a Winter Wonderland) |  |
| 860 | What Waits in Shadow#NPCs - Dragon's Throat (explorable area) |  |
| 861 | A Chance Encounter#NPCs - Kaineng Center (Winds of Change) |  |
| 862 | Tracking the Corruption#NPCs - The Marketplace (explorable area) |  |
| 863 | Cantha Courier Crisis#NPCs - Bukdek Byway (Winds of Change) |  |
| 864 | A Treaty's a Treaty#NPCs - Tsumei Village (Winds of Change) |  |
| 865 | Deadly Cargo#NPCs - Seitung Harbor (explorable area) |  |
| 866 | The Rescue Attempt#NPCs - Tahnnakai Temple (Winds of Change) |  |
| 867 | Violence in the Streets#NPCs - Wajjun Bazaar (Winds of Change) |  |
| 868 | Scarred Psyche |  |
| 869 | Calling All Thugs#NPCs - Shadow's Passage (Winds of Change) |  |
| 870 | Finding Jinnai#NPCs - Altrumm Ruins |  |
| 871 | Raid on Shing Jea Monastery#NPCs - Shing Jea Monastery |  |
| 872 | Raid on Kaineng Center#NPCs - Kaineng Center (Winds of Change) |  |
| 873 | Ministry of Oppression#NPCs - Wajjun Bazaar (Winds of Change) |  |
| 874 | The Final Confrontation#NPCs - The Final Confrontation |  |
| 875 | Lakeside County: 1070 AE |  |
| 876 | Ashford Catacombs: 1070 AE |  |

## The pack's ids these tables do not name (43)

These have a folder and an instance value in `config/World/Vanilla/maps/`, and no row in either table.
They are named by the client's `AreaInfo` record (`name_id`) or not at all; they are not invented here.

74, 185, 186, 229, 231, 299, 355, 356, 357, 358, 366, 533, 534, 535, 541, 542, 594, 595, 599, 709, 713, 714, 779, 787, 823, 824, 825, 826, 827, 828, 829, 831, 832, 833, 834, 835, 836, 877, 878, 879, 880, 881, 882

Id 474 appears in both tables. It is copied as it stands rather than deduplicated: the client's own
tables are the witness, and a choice between them would be ours.

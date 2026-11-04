# Design: the mode

Eloram is one mode: ARAM: Mayhem. Five against five on one lane, champions dealt at random,
augments picked as you level, about twenty minutes, first nexus to fall loses. Its rules are
League's ARAM with Mayhem on top, at League's numbers (the League Wiki and Riot's Mayhem posts,
listed at the end). Everything is in the game's own names: our map, our champions, our items, our
augments. `NETCODE.md` is how it is played over the network.

The numbers below are the rules the sim keeps (`el_sim::rules` and `content/`). A change to one is a
change to a golden hash.

## A match

- **Ten seats**, two teams of five: Dawn and Dusk. Empty seats take bots, which play through the
  same orders as people.
- **The draw**: ten different champions are dealt from the pool, one a seat, and a champion select
  of 20 s follows (`--select-secs`). A reroll sends yours to your team's bench and deals you one no
  one holds. Rerolls are banked across matches, as League's are: a signed-in wallet holds two at
  most (a new one starts with two), spends one on each reroll, and earns one back for each match
  it's still seated in when the match ends; a guest gets one each select (`el-server`'s
  `rerolls.rs`, kept in the records directory's `rerolls.json`). Anyone on
  the team can take a champion off its bench, leaving their own in its place. You see your own
  team's champions and bench only. The match starts when the time is up or everyone seated is
  ready, with the picks the select ended on; the server's bots keep what they were dealt.
- **Start**: level 3 with 1,400 gold. Passive gold from 1:00. The base gates hold champions in for
  the first 15 s.
- **No recall, and the fountain doesn't heal.** The shop is open only while you are dead or on your
  team's platform, and closes when you leave it.
- **Deaths** last 11 s at the start, growing with level to 40 s.
- **Healing** from any source is halved. Damage from an enemy champion more than 1,000 units away is
  reduced to between 85% and 70%, by distance.
- **Surrender** is offered from 8:00: a yes opens a vote for a minute, among the team's players
  (bots don't vote), and it passes with four of five of them, rounded up (alone with bots, one;
  two players, both). It fails once enough say no, or when its minute is up, and the team waits
  three minutes for the next.
- **The end**: a nexus falls.

## The map: the Causeway

One lane over a chasm, about 11,000 units from nexus to nexus and about 1,700 wide, widening into
each base. The sim's map is a rectangle of 15,360 × 3,840 units with the lane along +x; the camera
looks at it on a diagonal, which is a matter of drawing, not data.

Each side, from the middle out:
- an **outer turret** and an **inhibitor turret** in the lane;
- an **inhibitor**, which respawns, and while it is down the other side's waves bring super minions;
- two **nexus turrets** before the **nexus**;
- the **platform** with the shop, behind a fountain laser no enemy survives.

Turrets protect the one behind them: an inhibitor turret can't be hit while the outer turret stands,
the inhibitor while its turret stands, the nexus turrets while the inhibitor stands, the nexus while
either nexus turret stands.

**Brush** lines both sides of the lane in a few patches. A unit in brush is seen by the other team
only from inside the same brush, or when revealed (`NETCODE.md`, "Fog is the server's").

**Health relics**: four pads, two a side. The outer pair first appears at 1:45, the inner pair at
2:30, and each comes back 90 s after it is taken. A champion who takes one is healed 8% of their
missing health and mana at once. 2.5 s later a beam strikes the pad: every champion within 850
units, of either team, gets 16% of missing health and mana (Mayhem: 22%).

## Minions and turrets

- **Waves** leave both nexuses at 0:50, then every 25 s, the interval shrinking to 13 s as the match
  goes on. A wave is three melee minions and three casters; every second wave from the third adds
  a siege minion.
- **Minions** walk the lane and fight what they meet, in League's order of who to hit: an enemy
  champion who attacks an allied champion nearby, then the nearest enemies by kind. They push
  each other aside and give way to champions.
- **Turrets** shoot the nearest minion, until an enemy champion damages an allied champion in the
  turret's range. Then that champion is the target. A turret's shots against a champion grow
  stronger with each consecutive hit.
- **Mayhem**: the nexus and the nexus turrets have 3,000 health each.

## Champions

- **A champion** is a table: base stats and their growth a level (health, mana or energy, attack
  damage, ability power, armour, magic resist, attack speed, attack range, move speed), an auto
  attack, a passive and four abilities (Q, W, E and R). R unlocks at level 6 and ranks up at 11 and
  16; the other three have five ranks each. Levels run 3 to 18.
- **Abilities** are composed from a fixed set of effects: damage, heals, shields, buffs, crowd
  control (stun, root, slow, silence, airborne), knockbacks, dashes, missiles, areas, zones, reveals,
  cooldown changes. So is every augment and every item. A champion needs code of its own only for
  what the effects can't say.
- **Summoner spells**: two a champion, the same two for everyone. D is Blink: there at once, a
  short way. F is Tag: a thrown snowball that, on a hit, lets you dash to its target while the tag
  lasts.
- **The pool** is twenty, each dealt once a match across both teams. Marksmen: Vessa, the Long
  Watch (a piercing bolt, a hobbling shot, a roll, a shot across the map); Marret, the Gunwright (a
  ricochet, spent brass that slows, a kick back, a salvo). Mages: Orrin of the Ash Choir (an ember,
  a field of ash that lands late, a blink, a starfall); Seren, the Quiet Star (a lance of dusk, a
  gravity well, a drift, a skyfall from across the map that shows who it hit); Nimra, the
  Frostglass (a shard, a field of rime, a cold snap that roots, a whiteout that stuns around her);
  Torek, the Siege Engine (a shell that lands late, a snare charge, a brace, a bombardment from far
  off). Tanks: Brakka, the Quarry (a chain hook, a stone skin, a quake, an upheaval); Hollis, the
  Bell Warden (a toll that stuns, a bronze hide, a peal that pushes, a bell he holds around him);
  Gorum, the Old Ox (a horn toss, a stamp, a charge, unbroken). Fighters: Talmar, the Tidewarden (a
  backwash, a riptide around him, a breaker dash, a maelstrom that throws them up); Vire, the Ashen
  Blade (a sweep, a war cry, a close-in dash, a verdict that silences); Pell, the Hedge Knight (a
  shield bash, a rally for allies, a spur, a banner charge); Brun, the Smith (a hammerfall, a
  temper, a bellows blast, an anvil); Sefa, the Wind Dancer (a gale cut, an updraft, a glide, a
  tempest around her). Assassins: Kesh, the Quiet Knife (twin blades, a shadow step, a cripple, an
  execution); Sable, the Night Courier (a hex bolt, a shade step that hurts where she lands, a hush,
  a dark hour). Enchanters: Liora, the Lantern (a bind, a mend, an aegis, a sanctuary); Oda of the
  Long Road (a road stone, a tether that roots, a hearth for an ally, the long road for the team);
  Ulla, the Mender (a sap, a bloom that heals, thorns, a verdance over the team); Mira, the
  Lamplighter (a wick, a glow for allies, a snuff that silences, a bonfire).
  `el_sim::content::champions` is their numbers.

## Gold, experience and items

- **Gold**: passive income from 1:00; minion kills (last hits), champion kills and assists, turrets
  (shared by the team). Shared experience from deaths within range.
- **Items**: bought in the shop while it is open, six slots and a trinket. Four of the finished
  items have an active, used from their slot's key and then on a cooldown of their own (ability
  haste doesn't shorten it): Bulwark Charm a shield, Windstride a burst of speed, Thunder Censer a
  blast that slows the enemies near, Mending Chalice a heal. Not while stunned or in the air;
  silence doesn't stop one. `el_sim::content::items` is their numbers.
- **The trinket** holds treats (Hearthbread): one to start, one more for every champion of a team
  each time it destroys an enemy turret, six at most. Eating one heals a little (halved, as all
  healing is) and gives a moment's speed, then 10 s before the next.

## Mayhem: augments

- **Four augment screens**: at the start (level 3), and at levels 7, 11 and 15. One augment, Encore
  (Gold), earns a fifth: Prismatic, opened after the four (at once, if they're all done).
- **Three offers** a screen, and each offer can be rerolled once.
- **Tiers**: Silver, Gold and Prismatic. Everyone gets the same tier on a given screen, and the
  first two screens are never both Silver.
- **When**: only while the shop is open (dead, or on the platform). An unpicked screen waits.
- **What**: an augment is an effect program like an ability's, attached to a trigger: on an auto
  hit, an ability hit, a cast, taking damage, a kill, every tick, a level.
- **Combo Breaker**: a champion immobilised for 5 of the last 7 s is cleansed and immune to crowd
  control for 3 s.
- **A headliner**: one match-wide modifier a game, drawn from the seed and shown as the match
  starts: everyone faster, 40 haste for all, passive gold twice as fast, death timers halved,
  500 more health and 20 of each resist, or 20 attack damage and 40 ability power more
  (`el_sim::content::headliners`).
- **The pool of augments** is about forty, a third of them a tier: stat changes, and ops on a
  trigger (an auto that lands, an ability that hits, a cast, a takedown, low health, a pulse).
  `el_sim::content::augments` is their numbers.
- **The offers** are drawn from the match's seed, the seat, the screen and the offer's number, so
  they don't depend on the order anything else happened in. A seat's offers are sent to that seat
  only.

## Controls

League's defaults, because that is what players' hands know:
- **Right click** to move, or to attack what's under the cursor. **A** then click to attack-move.
  **S** stops, **H** holds.
- **Q, W, E, R** cast; **D, F** are the summoner spells. Smart cast by default (cast where the cursor
  is on the press). With indicators on (the menu's, or `?indicators=1`; League's "quick cast with
  indicator"), a held key shows the ability's range and its shape where it would land (a
  skillshot's lane, an area's circle kept in range, a dash's path, a blink's landing) and letting go
  casts; **Esc** or a right click calls it off. **Ctrl** with an ability's key levels it.
- **1, 2, 3, 5, 6, 7** use the items in the six slots, **4** eats a treat (League's item keys).
  While an augment screen is open, **1, 2, 3** take its cards instead, and **Shift** with one
  rerolls it. **P** the shop, **Tab** the scoreboard (with the
  button that opens a vote to surrender, from 8:00; an open vote shows its yes and no).
- **G** (or **Alt**) with a left click pings "look here", **Ctrl** with one "careful", on the unit
  under the cursor if there is one. Five at once, then one a second; the team alone sees them.
- **The camera** looks across the lane diagonally, as ARAM's does: Dawn's end at the bottom left,
  Dusk's at the top right, 56° down through a 40° lens as League's is (`el-client/src/camera.rs`,
  `VIEW`); the screen's directions (the edges, the arrows, WASD) are turned onto the map with it. It is free to
  start, as MOBA players keep it: it pans at the screen's edge, with the
  arrows, or dragged with the **middle button**; **Space** (or **F1**) held brings it to your
  champion, **F2** to **F5** held to each ally; a left click (or drag) on the minimap looks there;
  it comes back to your champion as the match starts and at each respawn. **Y** locks it on your
  champion or frees it (`?camera=locked`, remembered); locked, it looks a little ahead, towards the
  enemy's end (League's "per-side offset"), and the minimap's look lasts while the button is held.
  The wheel zooms a little, eased. The minimap, a square at the bottom right, is the map turned as
  the camera turns it (the lane from its bottom left to its top right), with the camera's view on it.
- **The HUD**: the stats (attack damage, ability power, armour, magic resist, attack speed, haste,
  move speed, range), the portrait (the champion's own model, drawn live:
  `el-client/src/portrait.rs`; the allies' down the left are drawn the same way, a turn each a
  frame) and level, the abilities (each with an icon painted from its look, `el_client_core::icons`:
  its family's colours, a mark for how it reaches, a pip for its crowd control, a gold corner on an
  ultimate; the key on each, the cooldown swept away clockwise with its seconds counted over it and
  a flash as it comes back, the ranks under it, a **+** over it while a point may go there, which a
  click spends too, a blue border while the mana's short), health, mana, the items (each with an
  icon: the mark of what it gives most of, a badge for the next, a gold rim on a finished item; an
  active's key at its corner and its seconds over it while it cools down), the trinket's treats
  beside the gold, in one panel at the bottom's middle; the score and clock at the top; the allies down the
  left (each one's ultimate an orb on its portrait, lit while it's ready: only its team is told);
  the target frame at the top left (the unit last clicked or attacked: its name, level, health and
  shield, mana and what holds it, `hud_target.rs`); a tooltip over an ability, an item or a shop's
  item says what it does (`el_client_core::tips`, read from the same tables the sim plays). Every
  champion's health bar in the world has a mark each hundred and a heavier one each thousand, a
  shield a white stretch past the health. The shop (P) opens in the middle: a Recommended row for
  the champion's role (`el_client_core::tips::recommended`, starred wherever the item shows), then
  the components and the finished items, each a tile with its icon. Dying greys the screen, puts the
  seconds to respawn over the portrait and shows the death recap: who killed the champion and what
  hurt it in the last fifteen seconds, each line split physical, magic and true
  (`el_client_core::recap`, `hud_death.rs`). The scoreboard (Tab) has a row a player: the portrait,
  the player and the champion, the level, kills and deaths, and the items (the player's own;
  everyone's watching a record).
- **Around a match**: a card in the middle while the page connects, signs in, waits in the lobby
  (**Find a match**) or the queue (how long it's waited, who else is, the seconds before the bots
  take the empty seats, **Leave the queue**), reconnects or has failed (**Reload**), `hud_lobby.rs`.
  Champion select is a screen of its own (`hud_select.rs`): the seconds and a bar running down, the
  team down the left (each seat's champion badge, name, role and who plays it, the player's own in
  gold), the champion held in the middle with its title, role and its four abilities' icons (a
  tooltip over each, at its first rank), the bench under it as tiles a click takes, and the reroll
  (greyed once spent) and ready (green once said) buttons. A match over darkens the world and says
  **Victory** or **Defeat** over the score, the clock it ended on and the player's own line, the
  scoreboard under it, and **Play again** queues straight back in.
- **WASD** as an option (`?keys=wasd`, remembered), League's own layout for it (2025): **W, A, S,
  D** move; left click attacks what's under the cursor; the abilities are right click, **Shift**,
  **E** and **R**; the summoner spells **Q** and **F**; **C** calls an aimed cast off; **Ctrl** with
  a number rerolls an augment card. The steer rides each tick's command (`InputCmd`), predicted like
  walking.
- **The menu** (**Esc**, or the button at the top left; `el-client/src/menu.rs`), drawn in the game
  as the HUD is, so a desktop build has it too: a rail of pages and **Resume** at its foot, the
  world going on dimmed under it (the champion keeps its last order; the keys, the wheel and the
  map's clicks are the menu's while it's up). **Controls**: the keys' layout (League's default or
  WASD), casting (quick, or with an indicator), the camera (free or locked), and under them every
  key of the layout chosen, two columns of groups. **Graphics**: the tier (low or high, at the next
  load: a reload keeps the seat in a match, and a button does it) and what's drawing now, the
  frame rate with it. **Sound**: the master's volume and each bus's, and mute. Esc is one thing at
  a time, the top one first: an aimed cast called off, then the shop closed, then the menu. Each
  choice is kept for the next visit (the URL's `?keys=`, `?indicators=`, `?camera=` and `?quality=`
  still choose too, and a choice in the menu takes its parameter off the address). The keys are
  one table, `el_client_core::controls`: the menu's page, the first match's strip and the key over
  each ability on the HUD read it, and the input code casts with its keys.
- **A first match** shows a strip under the score: the least that plays (right click walks and
  attacks, Q W E R cast, Ctrl with one levels it, P the shop, Tab the scores; WASD's own), and Esc
  for every other key, until **Got it**, three minutes in, or the match's end; then never again
  (`hud_first.rs`). Not in practice, nor watching.

## Sources

- The League Wiki: [ARAM](https://wiki.leagueoflegends.com/en-us/ARAM),
  [ARAM: Mayhem](https://wiki.leagueoflegends.com/en-us/ARAM:_Mayhem),
  [Howling Abyss](https://wiki.leagueoflegends.com/en-us/Howling_Abyss),
  [Health Relic](https://wiki.leagueoflegends.com/en-us/Health_Relic).
- Riot: [Bringing Mayhem to ARAM](https://www.leagueoflegends.com/en-us/news/dev/dev-bringing-mayhem-to-aram/) (2025),
  [Augmentmaxxing ARAM: Mayhem](https://www.leagueoflegends.com/en-us/news/dev/dev-augmentmaxxing-aram-mayhem/) (2026),
  [Patch 26.3 notes](https://www.leagueoflegends.com/en-us/news/game-updates/patch-26-3-notes/),
  [the support page](https://support.riotgames.com/league-of-legends/events/league-of-legends-aram-mayhem-game-mode),
  [the keyboard (WASD) input FAQ](https://support.riotgames.com/league-of-legends/gameplay/league-of-legends-keyboard-wasd-input-faq)
  and the wiki's [Controls and Hotkeys](https://wiki.leagueoflegends.com/en-us/Hotkeys) for WASD's layout.
- Unsettled in the sources, so ours to choose: Mayhem's starting gold since February 2026, and
  whether the first screen and level 3 are one screen or two (here they are one).

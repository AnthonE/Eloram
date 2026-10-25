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
  one holds; two a seat (League banks rerolls across games; nothing persists here yet). Anyone on
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
- **Surrender** is offered from 8:00.
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
- **Items**: bought in the shop while it is open, six slots and a trinket. The trinket slot holds
  Poro-Snax-like treats, with one more charge for each enemy turret destroyed.

## Mayhem: augments

- **Four augment screens**: at the start (level 3), and at levels 7, 11 and 15. Some augments grant
  a fifth.
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
  is on the press). With indicators on (`?indicators=1`, remembered; League's "quick cast with
  indicator"), a held key shows the ability's range and its shape where it would land (a
  skillshot's lane, an area's circle kept in range, a dash's path, a blink's landing) and letting go
  casts; **Esc** or a right click calls it off. **Ctrl** with an ability's key levels it.
- **1, 2, 3** take an augment card while a screen is open, **Shift** with one rerolls it (items have
  no actives yet, so the number keys are free). **P** the shop, **Tab** the scoreboard.
- **Y** locks the camera on your champion (locked to start) or frees it; free, it pans at the
  screen's edge and with the arrows, and **Space** held brings it back. The wheel zooms a little.
- **WASD** movement as an option (League added it in 2025): the wire carries it (`InputCmd`'s
  steer), the page doesn't bind it yet.

## Sources

- The League Wiki: [ARAM](https://wiki.leagueoflegends.com/en-us/ARAM),
  [ARAM: Mayhem](https://wiki.leagueoflegends.com/en-us/ARAM:_Mayhem),
  [Howling Abyss](https://wiki.leagueoflegends.com/en-us/Howling_Abyss),
  [Health Relic](https://wiki.leagueoflegends.com/en-us/Health_Relic).
- Riot: [Bringing Mayhem to ARAM](https://www.leagueoflegends.com/en-us/news/dev/dev-bringing-mayhem-to-aram/) (2025),
  [Augmentmaxxing ARAM: Mayhem](https://www.leagueoflegends.com/en-us/news/dev/dev-augmentmaxxing-aram-mayhem/) (2026),
  [Patch 26.3 notes](https://www.leagueoflegends.com/en-us/news/game-updates/patch-26-3-notes/),
  [the support page](https://support.riotgames.com/league-of-legends/events/league-of-legends-aram-mayhem-game-mode).
- Unsettled in the sources, so ours to choose: Mayhem's starting gold since February 2026, and
  whether the first screen and level 3 are one screen or two (here they are one).

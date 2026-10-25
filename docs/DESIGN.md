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
- **The draw**: each player is dealt a random champion from the pool, with one reroll; a bench
  shows rerolled champions anyone on the team can take.
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
- **Summoner spells**: two a champion. Every champion has Flash. The other is the Mark: a thrown
  snowball that, on a hit, lets you dash to its target.
- **The pool** starts with five archetypes (a marksman, a mage, a tank, an assassin, an enchanter)
  and grows.

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
- **A headliner**: one match-wide modifier a game.
- **The offers** are drawn from the match's seed, the seat, the screen and the offer's number, so
  they don't depend on the order anything else happened in. A seat's offers are sent to that seat
  only.

## Controls

League's defaults, because that is what players' hands know:
- **Right click** to move, or to attack what's under the cursor. **A** then click to attack-move.
  **S** stops, **H** holds.
- **Q, W, E, R** cast; **D, F** are the summoner spells. Smart cast by default (cast where the cursor
  is on the press), with indicators on a setting. **Ctrl** with an ability's key levels it.
- **1–6** items, **4** the trinket. **P** the shop, **Tab** the scoreboard.
- **Space** centres the camera on your champion, **Y** locks it there; the camera pans at the
  screen's edge.
- **WASD** movement as an option (League added it in 2025).

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

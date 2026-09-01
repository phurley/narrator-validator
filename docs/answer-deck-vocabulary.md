# Answer-deck vocabulary canon (`ruleset.standard_mystery@7.0.0`)

This document is the authoritative card list for the three answer decks
Format 3.7 introduces: `answer.motive.*`, `answer.time.*`, and
`answer.method.*`. [Story Format 3.7](story-format-3.7.md) defines *where and
how* these subjects are declared, bound, and disclosed; this document defines
*what the cards are*. It is the source the ruleset `7.0.0` catalog is built
from.

The vocabulary is deliberately fixed and small. These are generic accessory
cards, printed once and reused by every story that opts into them — the same
way the two ENTER control cards are fixed across every edition
(`src/scanner_control.rs`). A story never authors an `answer.*` subject and
never chooses its `tag_id`; it only declares, in `deck.yaml`, which of these
cards its printed edition carries.

## Catalog

29 cards: 10 motive, 8 time, 11 method. `tag_id`s are assigned descending from
2112, deck by deck, and occupy 2084–2112 of the reserved 2000–2112 range.
2000–2083 remain unassigned headroom for future ruleset-owned decks.
Assignments are append-only and immutable once `7.0.0` ships.

```yaml
answers:
  # --- answer.motive.*: why the person did it -------------------------------
  - id: answer.motive.greed
    tag_id: 2112
    name: Greed
    description: >
      Done for money, property, a payout, an inheritance, or the value of the
      thing itself.

  - id: answer.motive.jealousy
    tag_id: 2111
    name: Jealousy
    description: >
      Done because a rival had what the culprit wanted — a person, a place, a
      reputation, a prize.

  - id: answer.motive.revenge
    tag_id: 2110
    name: Revenge
    description: >
      Done to settle a past wrong, real or only believed.

  - id: answer.motive.fear_of_exposure
    tag_id: 2109
    name: Fear of exposure
    description: >
      Done to keep a secret buried — to silence a witness, destroy a record,
      or stop an investigation.

  - id: answer.motive.self_preservation
    tag_id: 2108
    name: Self-preservation
    description: >
      Done to escape immediate danger to the culprit's own body: self-defence,
      panic in a struggle, a way out of a trap.

  - id: answer.motive.fear_for_another
    tag_id: 2107
    name: Fear for someone else
    description: >
      Done to shield another person from harm, blame, or loss — a child, a
      partner, an accomplice.

  - id: answer.motive.love
    tag_id: 2106
    name: Love
    description: >
      Done out of attachment to a person: to win them, keep them, or refuse to
      let them go.

  - id: answer.motive.ambition
    tag_id: 2105
    name: Ambition
    description: >
      Done for position, standing, credit, or a legacy rather than for money.

  - id: answer.motive.desperation
    tag_id: 2104
    name: Desperation
    description: >
      Done by someone cornered — debt, illness, eviction, a deadline — seeking
      relief rather than gain.

  - id: answer.motive.loyalty
    tag_id: 2103
    name: Loyalty
    description: >
      Done on behalf of a person, family, employer, or cause the culprit felt
      bound to, including acting on orders.

  # --- answer.time.*: when it happened ---------------------------------------
  - id: answer.time.dawn
    tag_id: 2102
    name: At dawn
    description: First light until the sun is up.

  - id: answer.time.morning
    tag_id: 2101
    name: In the morning
    description: Sunrise until late morning.

  - id: answer.time.midday
    tag_id: 2100
    name: Around midday
    description: Late morning until early afternoon, across the midday meal.

  - id: answer.time.afternoon
    tag_id: 2099
    name: In the afternoon
    description: Early afternoon until the light starts to go.

  - id: answer.time.evening
    tag_id: 2098
    name: In the evening
    description: Sunset through the early part of the night.

  - id: answer.time.night
    tag_id: 2097
    name: At night
    description: Full dark, but still before midnight.

  - id: answer.time.after_midnight
    tag_id: 2096
    name: After midnight
    description: Midnight until the small hours.

  - id: answer.time.before_dawn
    tag_id: 2095
    name: Before dawn
    description: The small hours until first light.

  # --- answer.method.*: what physically happened -----------------------------
  - id: answer.method.struck
    tag_id: 2094
    name: Struck with something
    description: Blunt force — a weapon, a tool, an ordinary heavy object.

  - id: answer.method.stabbed
    tag_id: 2093
    name: Stabbed or cut
    description: A blade, a point, or broken glass.

  - id: answer.method.shot
    tag_id: 2092
    name: Shot
    description: A firearm or other projectile.

  - id: answer.method.poisoned
    tag_id: 2091
    name: Poisoned or drugged
    description: >
      A substance given, hidden in food or drink, or substituted — whether
      meant to kill or only to incapacitate.

  - id: answer.method.strangled
    tag_id: 2090
    name: Strangled or smothered
    description: Air cut off by hands, a ligature, or an obstruction.

  - id: answer.method.drowned
    tag_id: 2089
    name: Drowned
    description: Held under, or unable to get out of the water.

  - id: answer.method.fell
    tag_id: 2088
    name: Killed by a fall
    description: >
      A fall from height, down stairs, or onto something hard — pushed,
      dropped, or lost footing.

  - id: answer.method.fire
    tag_id: 2087
    name: Burned in a fire
    description: Fire, smoke, or an explosion.

  - id: answer.method.crushed
    tag_id: 2086
    name: Crushed or run down
    description: A vehicle, machinery, or a collapsing structure or load.

  - id: answer.method.neglect
    tag_id: 2085
    name: Left without help
    description: >
      Medicine withheld, an injury left untreated, an alarm ignored, someone
      abandoned somewhere they could not survive.

  - id: answer.method.not_killed
    tag_id: 2084
    name: Not killed by anyone
    description: >
      Illness, a failing heart, or a death nothing external caused — including
      the case where nobody died at all.
```

## Narration

Every `name` above is written to survive substitution into ordinary prose and
into TTS, not just to label a grid column. The two frames a narrator actually
uses are:

- **Commit frame** — "You named [[answer.motive.greed]] as the motive."
- **Prose frame** — "[[character.mara_voss]] did it out of
  [[answer.motive.greed]]."; "[[character.rowan_vale]] was
  [[answer.method.stabbed]]."; "It happened [[answer.time.after_midnight]]."

Each deck is worded to one uniform grammatical shape so that a single authored
sentence works whichever card fills it:

- **Motive** — a bare abstract noun, so both "the motive was ___" and "did it
  out of ___" read. This is why the protection card is *Fear for someone else*
  rather than *Protecting someone*: the latter breaks the "out of ___" frame.
- **Time** — a prepositional phrase, so "it happened ___" reads. Bare nouns
  ("Night") fail that frame, which is the dominant one for the mid-game
  question-topic use.
- **Method** — a past participle or participial phrase, so "the victim was
  ___" reads for every card without exception, including *Not killed by
  anyone*.

Names are sentence case, matching every other kind's display name in this
system (`Antique diving knife`, `Solve`). No name contains a bracketed
reference, a proper noun, or anything story-specific; the decks are printed
once and must not go stale when a story is edited.

## `answer.motive.*` — 10 cards

**Coverage principle.** Each card answers "what was the culprit trying to get
or to avoid?" The set tiles that question along four axes: things wanted
(*Greed*, *Ambition*, *Love*), things feared (*Fear of exposure*,
*Self-preservation*, *Fear for someone else*), grievances (*Revenge*,
*Jealousy*), and pressures (*Desperation*, *Loyalty*).

**Why ten and not five.** The pairs that a mystery's whole twist can turn on
have to be separable, or the deck cannot express the answer the players
deduced. *Fear of exposure* (silencing someone who knew) versus
*Self-preservation* (he came at me) is the difference between murder and
self-defence. *Greed* (money) versus *Ambition* (position) versus *Jealousy*
(a specific rival has it) are three different suspects in the same house.
Collapsing any of those pairs into a single "fear" or "gain" card would make
the deck unable to record a correct answer.

**Why ten and not twenty.** Beyond this set the candidates stop being motives
and start being *methods of motive* or *specific instances*, which a solve row
should express with the story's own character, entity, and event cards
instead. Considered and rejected:

- *Blackmail*, *extortion*, *inheritance*, *insurance*, *theft* — these name a
  mechanism or a specific prize, not a driver. Every one resolves to *Greed*
  or *Fear of exposure*, and printing both would make many rows ambiguous.
- *Cover-up* — indistinguishable from *Fear of exposure*.
- *Madness*, *insanity* — not deducible from evidence, so it makes a poor
  answer card, and the framing is one this system should not print.
- *Thrill*, *cruelty* — a serial-killer register that is out of tone for a
  system whose stories run down to age nine (`wrong_floor`), and unavailable
  to deduction for the same reason madness is.
- *Mercy* — real, but rare enough not to earn a permanent slot, and it lands
  on *Fear for someone else* or *Love* in every case examined.
- *Accident* / *no motive* — belongs in the method deck (*Not killed by
  anyone*), and see the accidental-death note below.
- *Ideology*, *duty*, *orders* — all three are *Loyalty*: acting for a person,
  group, or cause one feels bound to.

**Accidental deaths still get a motive row.** When a death was unintended, the
motive row should name the motive for the *deed behind* the accident, which is
almost always the interesting answer. In `quiet_kennel`, Cal Mercer's death is
an accident, but he was there to sabotage a champion dog for a wager: the
motive is *Greed*. Omitting the motive row instead would leak the twist,
because a step's row count is player-visible (see the disclosure boundary in
[Story Format 3.7](story-format-3.7.md)).

## `answer.time.*` — 8 cards

**Coverage principle.** The eight buckets tile a full 24 hours with no gaps and
no overlaps, in this canonical order:

> At dawn → In the morning → Around midday → In the afternoon → In the evening
> → At night → After midnight → Before dawn

That order is the sequence an `ordered` row uses when a story asks players to
place two times in sequence. It is cyclic — *Before dawn* precedes *At dawn* on
the following day — so a row that spans a midnight boundary should name the
day with the story's own event cards rather than relying on the bucket order.

**Buckets, never clock hours.** Precision stays in the evidence: a watch, a
log, a witness. The card is the *committed* answer, so it must be coarse enough
that a player who reasoned correctly cannot lose on a minute. Boundaries are
deliberately soft and light-relative rather than numeric, so the deck works for
a story set in a northern winter as well as a summer island.

**Deliberately uneven resolution.** Three cards cover the twelve hours from
sunrise to sunset and five cover the hours of darkness. That is not sloppiness:
it is where mysteries actually happen and where alibis are actually contested.
`island_retreat`'s death is at 21:18 (*In the evening*); `simple_mystery`'s is
a night incident; `quiet_kennel`'s runs from night through *Before dawn*. A
uniform 3-hour tiling would have spent slots on distinctions nobody asks about
("mid-morning" versus "late morning") while forcing every night case into one
card.

**The midnight cut is the point.** *At night* and *After midnight* exist as
separate cards specifically so the genre's most-asked question — "was it before
or after midnight?" — has two distinct answers. This is also the deck's
mid-game use: per Format 3.7, `answer.time.*` cards may answer an ordinary
`command.question` topic, so a player can put that question to a witness and
receive one of these cards back. The names read correctly standing alone as a
spoken reply.

Considered and rejected:

- *At midnight* as its own card — a boundary, not an interval; it would create
  an ambiguous overlap with both neighbours for the sake of one dramatic word.
- *Early evening* / *Late evening* — the illustrative split in the Format 3.7
  draft. It buys resolution in the one bucket that needs it least (evening
  events are the ones with the most witnesses) while leaving daytime uncovered.
- *Sunset*, *Twilight*, *Small hours* — flavour synonyms for buckets already
  present; *In the small hours* in particular was dropped for *Before dawn*
  because it does not read for a young player.
- *Yesterday*, *The night before* — relative to the telling, not a time of day.
  Which day is a separate question and belongs on the story's own event cards.

## `answer.method.*` — 11 cards

**Coverage principle.** Each card names the *physical cause of the harm* and
nothing else. Intent is never encoded here — that is the motive row's job —
which keeps every method row unambiguous: *Killed by a fall* is the right card
whether the victim was pushed, tripped, or stepped wrong in the dark.

**The one disambiguation rule.** *Not killed by anyone* is correct only when
there was no external physical cause at all: illness, a failing heart, or a
case where nobody actually died. A fall, a fire, or a drowning is an external
physical cause even when no person caused it, so an accidental death takes the
mechanism card, not this one. Without that rule, `quiet_kennel` would have two
defensible answers; with it, the answer is *Killed by a fall*.

**Why eleven.** The first eight (*Struck*, *Stabbed*, *Shot*, *Poisoned*,
*Strangled*, *Drowned*, *Fall*, *Fire*) are the standard mechanisms and cover
the overwhelming majority of cases; each is physically distinct enough that a
reasonable player and a reasonable author will pick the same card from the same
evidence. Three more earn their slots by covering families nothing else
reaches:

- *Crushed or run down* — vehicles, machinery, and collapses. A death by car is
  common and has no home among the other eight.
- *Left without help* — withheld medicine, an untreated injury, an ignored
  alarm. A whole category of mystery turns on a killing by omission, and it is
  not a variant of any mechanism above.
- *Not killed by anyone* — the "there was no murder" resolution, and the reason
  a story can print a method row at all in a case with no killer.

**Why not more.** Considered and rejected:

- *Exposure*, *hypothermia* — narrow, and in every case examined it reads as
  *Left without help*.
- *Electrocution*, *explosion*, *suffocation in a confined space* — one-story
  mechanisms. A story that needs the specific mechanism can name the entity
  card (the generator, the gas line) instead; the coarse bucket for each is
  already present (*Crushed*, *Fire*, *Strangled or smothered*).
- *Drugged* as its own card, separate from *Poisoned* — the difference is
  intent, and intent does not live in this deck. One card, named for both.
- *Asphyxiation* as a card distinct from *Strangled* — the same mechanism at a
  different level of clinical description. Folded into *Strangled or smothered*.
- *Missing*, *Taken*, *Stolen* — not harm mechanisms. A disappearance or a
  theft is expressed with the story's own character and entity cards; a story
  whose incident involved no injury simply does not print a method row
  (unlike the motive row, that omission spoils nothing, because "how did they
  die" only exists if someone died).

## Fit against the existing stories

The four maintained stories are the coverage test. Each true answer must fall
inside the canon, with one obvious card rather than two defensible ones:

| Story | Motive | Time | Method |
| --- | --- | --- | --- |
| `simple_mystery` — Lena Ortiz kills Adrian Bell over the cash ledger | *Fear of exposure* | *At night* | *Struck with something* (the brass service bell) |
| `island_retreat` — Mara Voss kills Rowan Vale at 21:18 with a diving knife | *Fear of exposure* | *In the evening* | *Stabbed or cut* |
| `quiet_kennel` — Cal Mercer sabotages Echo for a wager and dies in the fall | *Greed* | *Before dawn* | *Killed by a fall* |
| `wrong_floor` — Elias conceals Sam; Ruby's aid goes to the wrong child | *Fear for someone else* | *In the evening* | *(no method row — nobody is harmed)* |

`wrong_floor` is the deliberate negative case: a story with no killing prints
no method deck at all, and the motive deck still has the right card for it.

## What `narrator-validator#79` consumes from this document

- The card IDs, display names, and `tag_id` assignments above, verbatim, as the
  ruleset `7.0.0` answer-deck catalog, merged into a story's definitions the
  way `merge_ruleset_commands` already merges `command.*`.
- The reserved range check: 2084–2112 assigned, 2000–2083 reserved but
  unassigned, both inside the 2000–2112 reservation
  (`deck.tag_id_reserved_ruleset_answer_deck`,
  `deck.answer_tag_id_mismatch`).
- The canonical time order above, if `ordered` rows over `answer.time.*` are to
  be checked for authoring sanity.

Schema validation, the `Kind`/`CommandParameterType` extension, and the
`subject.answer_no_world_state` rule remain that ticket's work; nothing here
changes validator behaviour on its own.

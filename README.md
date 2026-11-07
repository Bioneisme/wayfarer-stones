# Wayfarer Stones

A mod for [Horizon](https://github.com/horizon-rust/horizon) (Minecraft: Java Edition 26.3 in Rust). Place stones around
your world, activate them, and travel between the ones you know.

## How to play

- **Place** a stone. You own it, and it gets a name you can change. It is two blocks tall, so it needs room above it.
- **Activate** a stone by right-clicking it once. It joins your list.
- **Travel** by right-clicking a stone: pick a destination on the screen, or search for it. You land beside the stone, or
  on top if its sides are blocked. A stone with nowhere safe to land refuses the travel and charges nothing.
- **Away from a stone**, a Wayfarer's Scroll opens the same screen from anywhere, and the travel uses it up.
- **Villages** generated with the mod on have a stone of their own, of the plains, desert, savanna, snowy and taiga kinds.
  Anyone may break it, and only an operator renames it.

Only a stone's owner or an operator can break it. Breaking either block removes the stone and gives one item.

The text is in English and Russian, following the game's language.

## Recipes

**Wayfarer Stone**, on a crafting table:

|   |   |   |
|---|---|---|
|   | Ender Pearl |   |
| Stone Bricks | Amethyst Shard | Stone Bricks |
| Stone Bricks | Stone Bricks | Stone Bricks |

**Wayfarer's Scroll**, anywhere: Paper and an Ender Pearl.

## Commands

| Command | What it does |
|---|---|
| `/wayfarer` | Opens the screen |
| `/wayfarer list` | Lists the stones you know |
| `/wayfarer to <name>` | Travels to a stone |
| `/wayfarer name <name>` | Renames your stone, within 4 blocks of it |
| `/wayfarer forget <name>` | Takes a stone off your list |
| `/wayfarer global <name>` | Makes a stone known to everyone (operators) |
| `/wayfarer remove <name>` | Removes a stone from the list, leaving its blocks (operators) |

## Settings

Each world keeps its settings in `horizon-wayfarer-stones/config.json`, in the world's folder.

| Setting | Default | What it does |
|---|---|---|
| `xp_levels_cost` | 1 | Levels a travel costs (0: free) |
| `blocks_per_level` | 500 | One more level per this many blocks (0: never) |
| `max_levels_cost` | 5 | The most a travel costs (0: no limit) |
| `cross_dimension_multiplier` | 3 | Multiplies the cost between dimensions |
| `free_within_blocks` | 0 | Free within this many blocks (0: never) |
| `cooldown_seconds` | 10 | Wait between travels (0: none) |
| `warmup_seconds` | 0 | Stand still this long before travelling |
| `owner_only_break` | true | Only the owner or an operator breaks a stone |
| `wayfarers_scroll` | true | Scrolls work; without them, travel only from a stone |
| `travel_anywhere` | false | Travel from anywhere, without a stone or a scroll |
| `max_stones_per_player` | 0 | Stones one player may own (0: no limit) |
| `name_pool` | a list | Names new stones draw from |

## Good to know

- The mod is one file, `wayfarer_stones_native`: its textures, text, recipes and villages are inside it. The old
  separate `wayfarer_stones` (assets) is no longer needed since 0.6.2; a copy you already have still works.
- Every player needs the mod to use the screen. A gamepad can close it but not click on it.
- It runs with your account's permissions, as every native mod does.

## Build

```sh
cargo test                                      # the rules
tools/build.sh package                          # Linux or WSL: dist/wayfarer_stones_native.hnmod, the content inside
tools/build.sh check                            # cargo check of the game with the mod applied
```

`tools/build.sh package` builds `content/` into `mixins/wayfarer_stones_native/data/wayfarer_stones-content.hmod`
(`native_data!`), which the game loads as a dev content mod.
`cargo horizon-mod` comes from `cargo install --git https://github.com/horizon-rust/horizon cargo-horizon-mod`.
`python tools/make_textures.py` and `python tools/make_structures.py` redraw the textures and the village templates.

## License

MIT

# Tell each player, once, when the native half of the mod isn't there: that half
# tags every player as they join, and the tag is missing without it.
execute as @a[tag=!wayfarer_stones.native,tag=!wayfarer_stones.told] run tellraw @s {"translate":"wayfarer_stones.missing.native","color":"red","extra":[{"text":" "},{"text":"https://playhorizon.dev/mods/wayfarer_stones_native","color":"aqua","underlined":true,"click_event":{"action":"open_url","url":"https://playhorizon.dev/mods/wayfarer_stones_native"}}]}
tag @a[tag=!wayfarer_stones.native,tag=!wayfarer_stones.told] add wayfarer_stones.told

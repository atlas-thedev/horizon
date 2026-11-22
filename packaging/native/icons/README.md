# Native icons

The icon library of the Native Client: 105 line icons in one style, plus full-colour artwork.

- `svg/<name>.svg`: the icon, 24 x 24 viewBox, filled shapes in `currentColor`. This is the source.
- `png/<name>.png`: the same icon, 64 px, white on transparent.
- `color/<name>.png`: full-colour artwork the UI draws as images (`emotes`, `marker-normal`, `marker-danger`,
  `marker-star`, `marker-flag`).
- `order.txt`: the library order. **Append only**: a name's position is its codepoint (U+F600 + index).
- `manifest.json`: generated; names, codepoints and which icon each module uses.

## Using an icon

- For a module: add `(id, label, icon name)` at the end of `MODULES` in `scripts/native-module-icons.py` and run
  `python3 scripts/native-module-icons.py --sync-horizon ../horizon`. Its card, settings page and every list use it.
- Anywhere else in the UI: `ModuleIcons.named("rocket", fallback)` in Java, `icons::named("rocket", fallback)` in Rust,
  then draw it with `canvas.icon(...)` like any other icon.

## Adding an icon

Put `<name>.svg` in `svg/` (and a 64 px `<name>.png` in `png/`), append `<name>` to `order.txt`, and run the script.

## All icons

| Icon | Name |
|---|---|
| ![gauge](png/gauge.png) | `gauge` |
| ![cursor](png/cursor.png) | `cursor` |
| ![keyboard](png/keyboard.png) | `keyboard` |
| ![keys-wasd](png/keys-wasd.png) | `keys-wasd` |
| ![move](png/move.png) | `move` |
| ![crosshair](png/crosshair.png) | `crosshair` |
| ![brightness](png/brightness.png) | `brightness` |
| ![sword](png/sword.png) | `sword` |
| ![shield](png/shield.png) | `shield` |
| ![shirt](png/shirt.png) | `shirt` |
| ![cube](png/cube.png) | `cube` |
| ![layers](png/layers.png) | `layers` |
| ![eye](png/eye.png) | `eye` |
| ![eye-off](png/eye-off.png) | `eye-off` |
| ![sun](png/sun.png) | `sun` |
| ![sparkles](png/sparkles.png) | `sparkles` |
| ![brush](png/brush.png) | `brush` |
| ![palette](png/palette.png) | `palette` |
| ![image](png/image.png) | `image` |
| ![video](png/video.png) | `video` |
| ![film](png/film.png) | `film` |
| ![music](png/music.png) | `music` |
| ![volume](png/volume.png) | `volume` |
| ![volume-mute](png/volume-mute.png) | `volume-mute` |
| ![mic](png/mic.png) | `mic` |
| ![mic-off](png/mic-off.png) | `mic-off` |
| ![broadcast](png/broadcast.png) | `broadcast` |
| ![shuffle](png/shuffle.png) | `shuffle` |
| ![repeat](png/repeat.png) | `repeat` |
| ![infinity](png/infinity.png) | `infinity` |
| ![map](png/map.png) | `map` |
| ![map-pin](png/map-pin.png) | `map-pin` |
| ![route](png/route.png) | `route` |
| ![flag](png/flag.png) | `flag` |
| ![flag-checkered](png/flag-checkered.png) | `flag-checkered` |
| ![warning](png/warning.png) | `warning` |
| ![star](png/star.png) | `star` |
| ![diamond](png/diamond.png) | `diamond` |
| ![crown](png/crown.png) | `crown` |
| ![trophy](png/trophy.png) | `trophy` |
| ![medal](png/medal.png) | `medal` |
| ![book](png/book.png) | `book` |
| ![backpack](png/backpack.png) | `backpack` |
| ![chest](png/chest.png) | `chest` |
| ![coins](png/coins.png) | `coins` |
| ![flask](png/flask.png) | `flask` |
| ![cauldron](png/cauldron.png) | `cauldron` |
| ![apple](png/apple.png) | `apple` |
| ![drumstick](png/drumstick.png) | `drumstick` |
| ![heart](png/heart.png) | `heart` |
| ![heart-broken](png/heart-broken.png) | `heart-broken` |
| ![bolt](png/bolt.png) | `bolt` |
| ![leaf](png/leaf.png) | `leaf` |
| ![feather](png/feather.png) | `feather` |
| ![rocket](png/rocket.png) | `rocket` |
| ![hourglass](png/hourglass.png) | `hourglass` |
| ![stopwatch](png/stopwatch.png) | `stopwatch` |
| ![calendar](png/calendar.png) | `calendar` |
| ![cloud-download](png/cloud-download.png) | `cloud-download` |
| ![cloud-upload](png/cloud-upload.png) | `cloud-upload` |
| ![document](png/document.png) | `document` |
| ![checkbox](png/checkbox.png) | `checkbox` |
| ![signal](png/signal.png) | `signal` |
| ![trend](png/trend.png) | `trend` |
| ![server](png/server.png) | `server` |
| ![database](png/database.png) | `database` |
| ![network](png/network.png) | `network` |
| ![plug](png/plug.png) | `plug` |
| ![cpu](png/cpu.png) | `cpu` |
| ![memory](png/memory.png) | `memory` |
| ![thermometer](png/thermometer.png) | `thermometer` |
| ![snowflake](png/snowflake.png) | `snowflake` |
| ![flame](png/flame.png) | `flame` |
| ![droplet](png/droplet.png) | `droplet` |
| ![wind](png/wind.png) | `wind` |
| ![frame](png/frame.png) | `frame` |
| ![window](png/window.png) | `window` |
| ![copy](png/copy.png) | `copy` |
| ![layers-plus](png/layers-plus.png) | `layers-plus` |
| ![grid](png/grid.png) | `grid` |
| ![fullscreen](png/fullscreen.png) | `fullscreen` |
| ![zoom-out](png/zoom-out.png) | `zoom-out` |
| ![zoom-in](png/zoom-in.png) | `zoom-in` |
| ![expand](png/expand.png) | `expand` |
| ![collapse](png/collapse.png) | `collapse` |
| ![swap](png/swap.png) | `swap` |
| ![code](png/code.png) | `code` |
| ![terminal](png/terminal.png) | `terminal` |
| ![bug](png/bug.png) | `bug` |
| ![shield-check](png/shield-check.png) | `shield-check` |
| ![settings](png/settings.png) | `settings` |
| ![sliders](png/sliders.png) | `sliders` |
| ![tune](png/tune.png) | `tune` |
| ![filter](png/filter.png) | `filter` |
| ![filter-plus](png/filter-plus.png) | `filter-plus` |
| ![trash](png/trash.png) | `trash` |
| ![save](png/save.png) | `save` |
| ![upload](png/upload.png) | `upload` |
| ![download](png/download.png) | `download` |
| ![refresh](png/refresh.png) | `refresh` |
| ![lock](png/lock.png) | `lock` |
| ![unlock](png/unlock.png) | `unlock` |
| ![user](png/user.png) | `user` |
| ![users](png/users.png) | `users` |
| ![magic](png/magic.png) | `magic` |

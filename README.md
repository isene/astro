# Astro - Comprehensive Amateur Astronomy

<img src="img/astro.svg" align="left" width="150" height="150">

![Rust](https://img.shields.io/badge/language-Rust-f74c00) ![License](https://img.shields.io/badge/license-Unlicense-green) ![Platform](https://img.shields.io/badge/platform-Linux%20%7C%20macOS-blue) ![Stay Amazing](https://img.shields.io/badge/Stay-Amazing-important)

One-stop terminal app for amateur astronomers. Sky panel + telescope/eyepiece catalog in a single binary, sharing your location, Bortle rating, and conditions across both modes.

Merger of [nova](https://github.com/isene/nova) (sky panel) and [scope](https://github.com/isene/scope) (gear catalog), built on [crust](https://github.com/isene/crust). Part of the [Fe₂O₃ Rust terminal suite](https://github.com/isene/fe2o3).

<br clear="left"/>

## What it does

### Sky mode (default)
- Hourly weather forecast with conditions thresholds (cloud, humidity, temp, wind)
- Ephemeris table per date (Sun, Moon, Mercury, Venus, Mars, Jupiter, Saturn, Uranus, Neptune)
- Visibility bars per hour with moon-phase shading
- Astronomical events (meteor showers, conjunctions, eclipses)
- Inline starcharts (Stelvision) and NASA APOD
- "Tonight summary" fallback when there are no notable events

### Gear mode (press `g`)
- Telescope and eyepiece catalog (focal ratio, aperture, AFOV, magnification, exit pupil, true FOV)
- Combination calculations: which eyepieces with which scopes
- Tag observation logs from picked equipment
- CSV / JSON export

## Install

```bash
git clone https://github.com/isene/astro
cd astro
PATH="/usr/bin:$PATH" cargo build --release
ln -sf "$PWD/target/release/astro" ~/bin/astro
```

## Migration

On first launch, astro looks for `~/.nova/config.yml` and `~/.scope/data.json` and copies them into `~/.astro/` so existing nova/scope users carry their settings and gear catalogs forward.

## Sky-mode keys

| Key | Action |
|---|---|
| `?` | Help |
| `g` | Switch to Gear mode |
| `UP`/`DOWN` / `k`/`j` | Move row |
| `PgUP`/`PgDOWN` | Page |
| `HOME`/`END` | First/Last |
| `e` | Show all events |
| `s` | Get starchart for selected hour |
| `S` | Open starchart in image viewer |
| `A` | Astronomy Picture Of the Day |
| `ENTER` | Refresh image |
| `l/a/o` | Location / Latitude / Longitude |
| `c/h/t/w/b` | Cloud / Humidity / Temp / Wind / Bortle limits |
| `r` | Redraw |
| `R` | Refetch weather + events |
| `W` | Save config |
| `q` | Quit |

## Gear-mode keys

| Key | Action |
|---|---|
| `g` | Back to Sky mode |
| `t` | Add telescope |
| `e` | Add eyepiece |
| `ENTER` | Edit selected |
| `TAB` | Switch focus (telescope ↔ eyepiece) |
| `UP`/`DOWN` | Move cursor |
| `Shift-UP`/`Shift-DOWN` | Reorder |
| `SPACE` | Tag/untag |
| `u` | Untag all |
| `A` | Tag all |
| `o` | Toggle sort |
| `Ctrl-O` | Observation log from tagged |
| `x` | Export tagged to CSV |
| `X` | Export all to JSON |
| `D` | Delete selected |
| `q` | Quit astro |

## License

Public domain (Unlicense).

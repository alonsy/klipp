# Contributing to Klipp

Thanks for wanting to help. Klipp is small and young, so every report and every pull request makes a real difference.

## Ways to help

- **Something doesn't work?** [Open a bug report](https://github.com/alonsy/klipp/issues/new/choose). The form asks for **Copy details** from Klipp (Settings → About), which tells us your PC, settings and the end of the log in one paste.
- **A game records badly** (black clips, missing sound, stutter): use the *A game doesn't record well* form.
- **Want auto-clips for a game?** Use the *Auto-clips for a game* form, or 👍 an existing request. The most requested games go first.
- **Different hardware.** AMD and Intel graphics, gaming laptops and Windows 10 get far less testing than NVIDIA desktops. A short report that it works on yours is useful too.
- **No GitHub account?** [klipp.dev/feedback](https://klipp.dev/feedback) reaches us as well.

## Before you write code

For anything bigger than a small fix, open an issue first and say what you'd like to change. It saves you building something that clashes with what's already planned on the [roadmap](ROADMAP.md).

A few things every change keeps:

- **Nothing touches the game.** Klipp never reads or writes another program's memory, never injects into a game and never draws inside a game's window. Game events come from official interfaces (like Counter-Strike 2's Game State Integration) or from the screen Klipp already records. This is what keeps players safe from anti-cheat bans, so changes that break it can't be merged.
- **Light by design.** The recorder runs all day. Changes to it shouldn't add CPU time, memory or late frames; say how you measured if it's close.
- **Your clips stay yours.** Klipp works offline and sends nothing anywhere unless the person asks it to.
- **No ads, ever.** See the promises in the [roadmap](ROADMAP.md).

## Building and checking

[docs/BUILDING.md](docs/BUILDING.md) covers the tools, FFmpeg and how to run everything. Before opening a pull request:

```powershell
cargo build --release          # no new warnings
cd studio
npm run check                  # svelte-check, 0 errors and 0 warnings
```

Then try your change for real: record, save a few clips, and play them back.

## Pull requests

- One change per pull request, with a title that says what it does for people ("Clip cards: a click plays the clip"), not how.
- Fill in the template: what changes, how you checked it, and the checklist.
- Text in the app is short, plain and friendly; look at how nearby text is written and match it.

By contributing, you agree that your work is released under the [MIT license](LICENSE), like the rest of Klipp.

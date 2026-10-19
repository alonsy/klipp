## What this changes

<!-- What it does for people using Klipp, and why. Link the issue it closes: "Fixes #12". -->

## How you checked it

<!-- The game or app you tried it with, your graphics card and Windows version, and what you saw. A screenshot or a short clip helps for anything visible. -->

## Checklist

- [ ] `cargo build --release` adds no warnings, and `npm run check` in `studio` is clean
- [ ] Nothing reads or writes another program's memory, injects into a game, or draws inside a game's window
- [ ] For recorder changes: recording costs no more than before (CPU, memory, late frames over a few minutes)
- [ ] New settings have a default that suits most people, and new text reads like the rest of the app

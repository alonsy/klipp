# Roadmap

Klipp is young. Here's where it's going, and what won't change on the way.

## Our promises

- **No ads.** Not in the free version, not in any plan, not ever.
- **Open source, always.** Every version of Klipp is published here under the MIT license.
- **Your clips are yours.** Klipp works fully offline and needs no account. Nothing leaves your PC unless you send it somewhere.
- **Light by design.** A recorder that runs all day has to cost nothing while it does. Every release is checked for memory growth.

## Now: 0.3

- A one-click installer, with a setup that checks what your PC can record and asks for your audio and video choices.
- Updates inside the app, in one click, installed quietly in the background.
- Always-on replay buffer with a save hotkey you can test in setup; presses close together become one clip.
- Recording that looks after itself: when the screen sleeps or the graphics driver resets, it picks the picture up again.
- Auto-clips through what games officially share: Counter-Strike 2 (Game State Integration), League of Legends (Live Client Data API) and Rocket League (Stats API).
- A PC test, video presets and a Performance mode that keep Klipp out of your game's way.
- Squeeze space, and old clips that tidy themselves up if you want them to.
- Separate audio tracks (Discord on its own, experimental), a session-grouped library, an editor with waveforms, and exports for sharing, with or without voices.
- Optional indicators on top of your game ("Auto-clips ON", clip saved), never in your clips.
- The save sound stays out of your clips by itself.
- NVIDIA, AMD and Intel graphics (AV1 or H.264), Windows 10 and 11, any of your screens.

## Next

### More games

We want auto-clips for the mainstream games people actually play, not just CS2.

Counter-Strike 2 was first because Valve offers **Game State Integration**: the game itself tells other apps about kills, rounds and deaths, officially and safely. Most games don't. Valorant, Rainbow Six Siege and many others have no such interface, and their anti-cheat systems rightly forbid reading a game's memory or injecting into it.

So we're building a **non-invasive event capturer**: a model that recognizes moments like kills and round wins from what's already on your screen, the same frames Klipp records anyway. It never touches the game process, reads no memory and injects nothing, so it stays on the right side of every anti-cheat. It will arrive in a future version, one game at a time.

### More hardware

- Gaming laptops recording on their NVIDIA GPU (today they record on the processor's built-in graphics).
- Signed installers (no more Windows warning), and installs through winget and Scoop.

### Cloud, your way, free

Klipp will connect to a **self-hosted cloud** for backing up and sharing clips, **at no cost from us**. You choose your own hosting plan and pay the hosting company directly; Klipp never sits in the middle and never takes a cut. Setting it up will happen inside the app: pick a provider, connect, done.

Local-only stays the default. The cloud is opt-in, and turning it off leaves everything working.

### Smaller things

- Switching from SteelSeries Moments in one step, clips and settings included.
- Screenshots, and stitching several clips into one.

## Ideas welcome

Missing something? Open an issue and tell us what you'd use Klipp for.

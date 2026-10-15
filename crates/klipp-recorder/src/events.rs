//! Game events, independent of which game they came from.

/// What happened. Game integrations produce these; the auto-clip engine only
/// ever sees this type, so adding a game doesn't touch the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    Kill,
    /// A kill that was a headshot (not also reported as `Kill`).
    Headshot,
    /// The player's n-th kill in the current round (n >= 2), as the scoreboard counts it.
    MultiKill(u8),
    /// Comes with the `Kill`/`Headshot` it describes.
    KnifeKill,
    /// Comes with the `Kill`/`Headshot` it describes.
    ZeusKill,
    Death,
    Mvp,
    /// Round results: timeline markers only, never a reason to clip.
    RoundWin,
    RoundLoss,
    /// Your team defused the bomb. A marker only.
    BombDefused,
    /// You pressed the save hotkey here (marks each press in a merged clip).
    Capture,
}

impl EventKind {
    /// Short name used in settings, file names, clip info and the studio's icons:
    /// "headshot", "3k", "ace", "victory", …
    pub fn label(self) -> String {
        match self {
            EventKind::Kill => "kill".into(),
            EventKind::Headshot => "headshot".into(),
            EventKind::MultiKill(n) if n >= 5 => "ace".into(),
            EventKind::MultiKill(n) => format!("{n}k"),
            EventKind::KnifeKill => "knife".into(),
            EventKind::ZeusKill => "zeus".into(),
            EventKind::Death => "death".into(),
            EventKind::Mvp => "mvp".into(),
            EventKind::RoundWin => "victory".into(),
            EventKind::RoundLoss => "defeat".into(),
            EventKind::BombDefused => "defuse".into(),
            EventKind::Capture => "capture".into(),
        }
    }

    /// Human-readable name, for MP4 chapters.
    pub fn title(self) -> String {
        match self {
            EventKind::Kill => "Kill".into(),
            EventKind::Headshot => "Headshot".into(),
            EventKind::MultiKill(n) if n >= 5 => "Ace".into(),
            EventKind::MultiKill(n) => format!("{n}K"),
            EventKind::KnifeKill => "Knife kill".into(),
            EventKind::ZeusKill => "Zeus kill".into(),
            EventKind::Death => "Death".into(),
            EventKind::Mvp => "MVP".into(),
            EventKind::RoundWin => "Round won".into(),
            EventKind::RoundLoss => "Round lost".into(),
            EventKind::BombDefused => "Bomb defused".into(),
            EventKind::Capture => "Saved here".into(),
        }
    }

    /// Which event names a clip after itself when several are in it.
    pub fn rank(self) -> u8 {
        match self {
            EventKind::MultiKill(n) => 100 + n,
            EventKind::KnifeKill => 50,
            EventKind::ZeusKill => 49,
            EventKind::Headshot => 40,
            EventKind::Mvp => 30,
            EventKind::Kill => 20,
            EventKind::Death => 10,
            EventKind::BombDefused => 5,
            EventKind::RoundWin => 4,
            EventKind::RoundLoss => 3,
            EventKind::Capture => 1,
        }
    }

    pub fn is_kill(self) -> bool {
        matches!(self, EventKind::Kill | EventKind::Headshot)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GameEvent {
    pub kind: EventKind,
    /// When it happened, on the shared clock.
    pub time: i64,
    /// Extra detail, e.g. the weapon ("ak47").
    pub detail: String,
}

impl GameEvent {
    pub fn new(kind: EventKind, time: i64, detail: impl Into<String>) -> Self {
        Self { kind, time, detail: detail.into() }
    }
}

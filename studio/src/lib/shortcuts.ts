/** The editor's keys, as its own list (?) and Settings → About show them. */
export const EDITOR_KEYS: readonly (readonly [keys: readonly string[], what: string])[] = [
  [["Space", "K"], "Play / pause"],
  [["I"], "Start the clip here"],
  [["O"], "End the clip here"],
  [["L"], "Loop the trimmed part"],
  [["←", "→"], "Back / forward 1 s (Shift: 5 s)"],
  [[",", "."], "Previous / next frame"],
  [["[", "]"], "Previous / next event"],
  [["Home", "End"], "Start / end of the trim"],
  [["E"], "Export"],
  [["M"], "Mute the preview"],
  [["F"], "Fullscreen"],
  [["?"], "These shortcuts"],
  [["Esc"], "Back to clips"],
  [["Alt"], "Hold to drag without snapping"],
];

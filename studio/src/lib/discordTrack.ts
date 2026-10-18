// What the window needs to know for Discord's own track (experimental).

/** Audio routers (Sonar, Voicemeeter, virtual cables…) play other apps' sound
 * again from their own program, so Klipp can't leave Discord out of the Game
 * track while one is installed. The same names the recorder checks. */
const ROUTER = /sonar|voicemeeter|vb-audio|cable|wave link|broadcast|oculus/i;

export function routerInstalled(playback: string[]): boolean {
  return playback.some((name) => ROUTER.test(name));
}

/** The track kind that records only the Discord app. */
export const DISCORD = "discord";

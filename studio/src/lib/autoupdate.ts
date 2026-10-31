// Installing the next version by itself, for test builds and normal
// releases: the recorder starts `klipp.exe --install-update` between games or
// right after it starts, without showing the window (or passes it to the open
// window). The same signed download and quiet installer as the update card,
// logged step by step in install.log for the developer.
import * as api from "./api";
import { toast } from "./state.svelte";

let busy = false;

export async function installByItself() {
  if (busy) return;
  busy = true;
  try {
    if (await api.exporting()) {
      // Updating closes Klipp, which would cut the export (or squeeze) off. The recorder asks again later.
      await api.installLog("asked to install the next version, but an export or a squeeze is running; not now");
      return;
    }
    const update = await api.checkUpdate(true);
    if (!update) {
      await api.installLog("asked to install the next version: nothing newer than this one");
      await api.exitIfHidden();
      return;
    }
    await api.installLog(`installing ${update.version} by itself (this is ${update.currentVersion}): downloading`);
    toast(`Installing ${update.version.includes("-test") ? "test build " : "Klipp "}${update.version}. Klipp closes and opens again in a few seconds.`);
    await update.downloadAndInstall((e) => {
      if (e.event === "Finished") api.installLog(`downloaded ${update.version}; checking its signature, then its installer takes over`);
    });
  } catch (e) {
    await api.installLog(`installing by itself failed: ${e}`);
    await api.exitIfHidden();
  } finally {
    busy = false;
  }
}

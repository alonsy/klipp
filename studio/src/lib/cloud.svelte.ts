// Who is signed in to klipp.dev on this PC. Asked once when something needs
// it (the Account page, the first upload), then kept until it changes.
import * as api from "./api";

export const cloud = $state<{ status: api.CloudStatus | null; loading: boolean }>({ status: null, loading: false });

let asking: Promise<api.CloudStatus | null> | null = null;

/** Asks klipp.dev who is signed in (nobody is a fine answer). */
export function refreshCloud(): Promise<api.CloudStatus | null> {
  if (asking) return asking;
  cloud.loading = true;
  asking = api
    .cloudStatus()
    .then(
      (status) => (cloud.status = status),
      // The account couldn't even be looked up (no connection library, a broken install): treated as signed out.
      () => (cloud.status = { signedIn: false, account: null, error: null, backedUp: 0 }),
    )
    .finally(() => {
      cloud.loading = false;
      asking = null;
    });
  return asking;
}

/** The plan's name as people know it. */
export function planName(id: string | undefined): string {
  return id === "pro" ? "Pro" : id === "plus" ? "Plus" : "Free";
}

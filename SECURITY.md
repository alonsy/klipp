# Security

## Reporting a problem

Please report security problems **privately**: on this repository's **Security** tab, choose **Report a vulnerability** ([direct link](https://github.com/alonsy/klipp/security/advisories/new)). Don't open a public issue for them.

Include what an attacker could do, the steps to reproduce it, and the Klipp version (Settings → About). You'll get an answer within a few days, and credit in the release notes once it's fixed, if you'd like.

## What counts

- The recorder (`klipp-recorder.exe`) and the window (`klipp.exe`), including the local connection Counter-Strike 2 uses to send game events.
- The installer and in-app updates: updates are signed, and Klipp refuses any update whose signature doesn't match.
- Anything that could leak clips, settings or files from a PC, or run code that isn't Klipp's.

klipp.dev is a separate project; problems with it are welcome through the same form.

## Supported versions

Only the latest release gets fixes. Klipp offers updates itself, in one click.

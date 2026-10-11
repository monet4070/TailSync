# TailSync source context extension (optional, Chromium MV3)

This optional helper implements the browser half of TailSync's O05
**consent-bound source adapter**. It is *not* part of the TailSync app build and
is never installed automatically. TailSync works without it; without a ticket or
without this extension, the original clipboard item is kept unchanged and the
missing images stay an explicit limitation.

## What it does

1. The user, in the TailSync app, picks a history row and chooses
   **Authorize browser source**. TailSync writes a short-lived *source ticket*
   file (origin, exact image-node indexes and SHA-256 of each original `src`,
   a random grant id, an expiry and a byte budget). The ticket is not a
   long-term App token and contains no URL query, page body, cookie or session.
2. The user loads that ticket file into this extension's popup. The popup shows
   the authorized origin and the number of nodes.
3. The user opens the authorized page and clicks **Collect current page images**.
   Only then does the extension, in the page's isolated world:
   - match each authorized node by the SHA-256 of the element's `getAttribute("src")`
     or its resolved `.src` (no DOM-order or image-count guessing);
   - accept only same-origin `http(s)` or `blob:` references whose canonical
     origin equals the ticket origin;
   - `fetch(url, { credentials: "same-origin", redirect: "error" })`, stream with
     a per-image cap, and keep only static PNG bytes (APNG and other types are
     refused);
   - download a bounded `tailsync-source-packet.json`.
4. Back in TailSync, the user chooses **Import source packet** and selects that
   file. TailSync validates the packet against the still-live grant, consumes it
   once, and binds the bytes to the exact original `img/src` spans.

Revoke the authorization from TailSync at any time; an in-flight import is
cancelled and a late packet is refused.

The grant expires after five minutes and can be imported only once. Save the
ticket to a new file; TailSync refuses to overwrite an existing file. A partial
import needs a new explicit grant to fill remaining nodes. Import the packet
through the same history row that issued it.

Ticket input is capped at 64 KiB. Collection is capped at eight nodes, 8 MiB per
image, and 16 MiB total while reading, with a 10-second request and 30-second
overall deadline. The collector rechecks the actual page origin and refuses
`currentSrc`/srcset drift, ambiguous matches and excessive DOM/source inventories.
Empty collections do not download a packet.

This manual file bridge is not browser attestation: a user can edit the packet,
and the stored scope proof cannot cryptographically certify where its PNG bytes
came from. PNG content/metadata may be sensitive. Protect and delete exported
files as appropriate; they are not content-redacted. Real Chrome/Edge,
login/blob pages and native file-dialog acceptance remain unverified.

## Permissions

`manifest.json` requests only `activeTab` and `scripting`. There is **no**
`cookies`, `host_permissions`, `storage`, `history`, `clipboard` or
`nativeMessaging` permission. The popup downloads the packet with a blob URL and
an `<a download>` element, so no `downloads` permission is needed either.

## Install (manual, explicit)

1. Open `chrome://extensions` (or `edge://extensions`).
2. Enable **Developer mode**.
3. Choose **Load unpacked** and select this `tools/browser-source-extension`
   directory.

## Tests

```
cd tools/browser-source-extension
node --test
```

The tests exercise the real `collectInPage` function against mock `document` and
`fetch` fixtures, including the credential/redirect options, exact-hash matching,
ambiguous/missing/cross-origin refusals, non-PNG/APNG/oversize refusal, and the
packet kind the TailSync importer expects. They are local unit tests; they do not
claim a real Chrome or a real login-gated page was verified.

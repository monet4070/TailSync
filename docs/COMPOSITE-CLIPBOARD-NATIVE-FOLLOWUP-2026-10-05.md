# Composite clipboard — single-Mac native follow-up (2026-10-05)

Records only the new native evidence from the `mac_native` follow-up (CLI session id
`01a10c02-2517-7121-9640-72515ddf1f36`). It does not modify
[`COMPOSITE-CLIPBOARD-SINGLE-MAC-ACCEPTANCE-2026-10-05.md`](COMPOSITE-CLIPBOARD-SINGLE-MAC-ACCEPTANCE-2026-10-05.md);
final integration belongs to the supervising agent. Evidence root:
`artifacts/composite-repair/2026-10-05-single-mac-native-followup/` (`REPORT.md`).
Signed-byte identity of the notification measurement copy is recorded in
`artifacts/composite-repair/2026-10-05-single-mac-supervision/native-identity-addendum.md` and
`notification-signature-identity.json`.

## Full-App delayed provider on the general pasteboard — PASS (5/5)
Old `c022bcb8…`-era candidate `TailSync-enabled-native.app` (`4350767b…` / `b22ad035…` /
helper `470f0814…`), NOT the frozen formal source `54582e18…`, with the isolated root
`/private/tmp/ts-native-followup`. A real `NSPasteboardItemDataProvider` on the general pasteboard
blocked the shipped helper for 45 s:

- 4 helper spawns, each ~5 s (0.06-4.99 / 5.26-10.21 / 11.09-16.00 / 18.09-23.04 s) = 4 bounded
  attempts (5 s deadline + CaptureRetry 200/800/2000 ms), then a terminal `capture_failed` for
  revision 229 at +23.09 s.
- The provider returned at +45 s; history rows and the general-pasteboard changeCount were
  unchanged afterwards. This is a capture/READ-only probe on the old candidate (no native publish
  request), so it shows no late capture / echo write on that candidate. It cannot certify the new
  frozen formal source `54582e18…`, and it does not claim that all publish late-write paths are
  closed.
- During the block the full App stayed responsive (cua transcript in
  `delayed-provider-gui-during-block.json`, calls at 12:41:55.266 / 12:42:03.858 inside the
  provider window): Settings AX 50 ms, menu 128 ms, History window 13 ms, live API rows 410 ms,
  plus the "could not be captured completely" sync warning.

## Notifications — queue / Swift consumption / UN add observed on an instrumented copy
Production chain: `start_storage_monitor` -> `log::warn!(src/lib.rs:253)` ->
`api::push_runtime_notification` -> Swift `startNotificationPoller` ->
`UNUserNotificationCenter.add` -> system presents. An independent instrumented Swift GUI copy of the
candidate package (read-only `getNotificationSettings` instead of `requestAuthorization`) measured
two real production notifications triggered by the storage monitor in the isolated root.

Identity caveat: the daemon and helper in this measurement copy are NOT byte-identical to the sealed
originals — `codesign --deep` re-signing changed the signed SHA256 (daemon `b22ad035…` -> `3cf73cac…`,
helper `470f0814…` -> `a3d17ec9…`). Root stripped signatures on independent temporary copies and
confirmed the unsigned SHA256 are equal (daemon `07fa0bcf…`, helper `c1ece702…`): the executable code
bytes match, the signed bytes do not. The GUI instrumentation and permission adaptation also differ.
So these results bind to the measurement signed hashes (daemon `3cf73cac…`, helper `a3d17ec9…`, copy
GUI `4d02b450…`) and are not inherited by the new formal candidate:
- queue ids observed by the poller: revision 3 -> `eventIds [1]`, revision 5 -> `eventIds [2]`;
- Swift consumption: `event_consumed id=1 level=error` (57 B) and `id=2 level=info` (59 B);
- `UNUserNotificationCenter.add` returned without error for both (`un_add ok=true`).
The copy reported `auth_status=2` (authorized), but the copy's permission adaptation differs from the
official signed GUI, so permission is not inherited as the official GUI's. On-screen display remains
UNKNOWN (no banner observed; transient banners are not reachable with the available cua capture).

## Installed 2.3.2 GUI per-payload restore — UNKNOWN (tool-blocked)
The v11 store was copied to `/private/tmp/ts-native-followup/old-gui/store` and hash-verified
(never the sealed original). The installed GUI launched with the isolated root (gui 82276,
daemon 82279) and opened that store, but its History window could not be opened: no History menu
item, the Cmd+Shift+H Carbon hotkey does not fire from cua key events, and cua Vec2 clicks are
window-relative so the menu-bar status item is unreachable (`ControlCenter`/`SystemUIServer` AX
queries time out). No API restore was used as a substitute; a human left-click on the status item
is the next step.

## Cleanup
Candidate and old Apps stopped, no provider/helper left, TCP 19890 free, test volumes detached.
General clipboard restored byte-for-byte to the pre-test user snapshot (private text omitted;
bytes/hash in the private-preserved backup). A later user copy observed during the run was NOT saved,
so there is NO complete lossless restoration claim.
`~/Library/Logs/TailSync/tailsyncd.log` restored to 138731421 bytes with matching sha256
`98abedd1…`. Lock `/tmp/tailsync-single-mac.lock` removed.

No complete-device gate is closed; Release remains NO-GO.

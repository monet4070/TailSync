# ADR-003：History favorites and long-press interaction

- Status: Accepted for implementation
- Scope: `shared/rust-core/src/db/favorites.rs`, history APIs, and both history UIs

## Context

History entries need a durable save-for-later action without changing their
timestamp order. A saved entry must survive the normal history clear action and
must not be removable accidentally through the existing history context-menu
path. File transfers are represented by multiple rows, so a file batch must be
treated as one logical item rather than allowing a partially saved batch.

The history window is intentionally small. The save action therefore uses a
long press with the existing single-click selection and double-click restore
interaction. The reference interaction is scheme C from
`docs/features/favorites-long-press.md`: a row-wide fill charges from left to
right, then a star stamp confirms completion.

## Decisions

1. **Keep the storage column and wire field named `pinned` for compatibility.**
   The user-facing and new API vocabulary is `favorite`, but existing v8/v9
   databases and clients can still decode `pinned`. Schema v10 adds the
   favorites index and normalizes any old partially pinned file batch.
2. **Favorite a logical item atomically.** A row without `batch_id` is one item;
   every row sharing a `batch_id` is one item. `set_favorite` and
   `delete_favorite` return all affected IDs so adapters can update visible
   siblings consistently.
3. **Use an explicit collection query.** `HistoryCollection::All` preserves
   the existing query shape; `HistoryCollection::Favorites` adds
   `pinned = 1` without changing sort order. Search, category, and date filters
   apply inside either collection.
4. **Make deletion authority explicit.** Core `HistoryDB::delete` rejects a
   logical favorite. The history-window right-click path calls that operation
   and shows a protection message on failure. Only the favorites-window path
   calls `delete_favorite`, which removes the complete logical item.
5. **Make clear non-destructive to favorites.** `clear_all` removes only
   unfavorited logical items. Automatic count/quota cleanup and duplicate
   replacement also skip logical favorites, so a background maintenance path
   cannot become an implicit deletion escape hatch.
6. **Use one responder/two-timer long-press state machine per platform.** The
   grace period is 220 ms and the charge period is 420 ms. A movement beyond
   8 px cancels the gesture. Swift uses one AppKit responder; Windows uses one
   pointer hook. The fill is a single declarative animation; no per-frame
   timer is used. Once committed, the completed fill remains as the favorite
   row's persistent theme-colour tint; removing the favorite fades it back to
   the ordinary row surface. The gesture suppresses the resulting click and
   double-click so it cannot restore the entry.
7. **Use a separate reusable favorites window.** Both platforms expose an
   `open_favorites_window`/close lifecycle adapter and render the same history
   view with the favorites collection. History and favorites own independent
   visibility, polling, close, and release lifecycles after the favorites
   window has been opened from history.

## Consequences

- Core owns the business invariants and both platform adapters remain thin.
- Existing `set_history_pinned` callers remain available as compatibility
  adapters, but they now delegate to the atomic favorite operation.
- The history UI has a star entry point and a favorites-only deletion surface;
  the clear confirmation explains that favorites are preserved.
- A schema migration is required once per installation, but it is idempotent
  and does not alter unpinned entries.
- The visible row gains a small amount of animated state and corresponding
  unit/integration coverage on both platforms.

## Verification

- Core tests cover collection queries, deletion protection, clear behavior,
  atomic batch mutation, duplicate/cleanup protection, and v10 migration.
- Windows tests cover the pointer timing hook, protected context-menu path,
  favorites collection query/deletion, separate-window command, and batch-row
  state propagation.
- macOS tests cover long-press commit, drag cancellation, click/double-click
  compatibility, and the full-size AppKit responder overlay.

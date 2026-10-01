# Reliable clipboard event receipts

Identity/base compatibility remains v4; wire frames remain negotiated v4/v5.
History schema remains v11. No new Settings fields, wire commands or history
migration are introduced. A **new persistent sidecar format v1** is introduced:
`remote-event-receipts-v1.json` beside identity/settings in the private data root,
independent of bulk storage relocation. Older builds ignore this file; downgrade
loses the new replay protection but preserves configuration/history readability.

Keys are the authenticated Noise static public-key fingerprint + event ID, not
the display hostname or content hash. A digest binds command, original timestamp
and content to that key. Same ID with different payload/timestamp is rejected;
same bytes with a genuinely new ID are accepted. A renamed authenticated peer
keeps its receipts; a different key starts a different identity scope.

Before native clipboard I/O, a claim is atomically persisted using private_fs
(owner-only permissions, file fsync, atomic replacement, parent fsync on Unix).
A known claim suppresses repeated clipboard application. History is retried
separately; completion is persisted after persistent history commits and before
ACK. Degraded RAM history is never marked durable in this sidecar. Its event can
still be acknowledged under the existing degraded contract; after restart its
history can be reconstructed without applying the old clipboard again.

Crash semantics: a crash between durable claim and native effect is uncertain.
On restart the receiver conservatively suppresses the effect and completes
history on retry. This is an at-most-once **attempt**, not exactly-once OS/SQL
atomicity; the user can copy again with a new event ID. A reported native failure
removes the claim to allow normal retry. Failure to persist/read receipts leaves
the event unacknowledged and does not start new native I/O. Corrupt/future format
files are preserved and fail closed. A local receipt-write failure may require
restoring access/space or manual recovery of a backed-up sidecar; no silent reset.

Capacity is 4096 receipts (2 MiB read limit). GC runs on event access/mutation and
only removes entries after their original timestamp + the 5-minute wire admission
window. A valid future-skewed event can occupy a slot for up to 10 minutes. Live
receipts are never evicted to admit new events; a full store returns retryable
backpressure until expiry. These bounds assume the same wall-clock timestamp
admission contract as the existing protocol. No clipboard content or local paths
are stored. The existing single-instance lease protects each production data root;
independent harness peers must use distinct roots.

Degraded configured-history recovery merges by `(type, data_hash)`, keeps the
existing persistent payload, promotes favorites if either copy is favorited,
and commits the handoff as one SQLite transaction. Relocation and explicit root
switching are rejected until configured recovery succeeds. No RAM row is replaced
by a copy of the old disk tree. Image/file references are retained with their
original managed payload directories during recovery before ordinary relocation.

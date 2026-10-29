# Walrus

An append-only, Bitcask-style key-value store written in Rust.

Keys and `{offset, len}` locations are held in an in-memory `HashMap`. Payloads stay on disk. Reads perform an in-memory lookup followed by a single file seek and read. Writes append directly to the log.

---

## Known Quirks & Roadmap

- **CRC Validation:** Record CRCs are validated during startup replay. `get()` currently reads slices directly without re-hashing on the hot path.
- **Sync Guarantees:** Writes rely on the OS page cache (`sync_all` is deferred). Explicit batch fsyncing for the async worker is in progress.

---

## Architecture

- **Append-only Writes:** O(1) sequential appends with CRC-32 framing.
- **Bitcask Index:** In-memory key directory pointing to disk offsets.
- **Compaction:** Re-indexes live keys into a `.tmp` log, flushes, syncs, and performs an atomic rename.
- **Concurrency:** Dedicated single-threaded worker actor communicating via Tokio MPSC and oneshot channels.

---

## File Layout

Records are big-endian and framed sequentially.

### Delete / Tombstone (`op = 0x00`)

```text
+----------+---------+-----------+-----+
| crc (4b) | op (1b) | klen (4b) | key |
+----------+---------+-----------+-----+
```

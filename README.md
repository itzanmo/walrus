# Walrus

An append-only, log-structured key-value storage engine in rust backed by an in-memory index and crc-32 record framing.

## Features

- **Append-only durability:** Fast, sequential $o(1)$ writes.
- **Hardware-accelerated integrity:** records guarded by crc-32 checks on startup and recovery.
- **Tombstone deletions:** non-destructive deletes appended to the log; zero random disk seeks.
- **Atomic compaction:** reclaim disk space from old versions and deleted records via temporary file staging and atomic renames.

---

## On-disk format

Walrus formats records sequentially into a single append-only log file using big-endian framing:

### Del record (`op = 0x00`)

### Put record (`op = 0x01`)

```text
+----------+---------+-------------+-----+-------------+-------+
| crc (4b) | op (1b) | klen (4b)   | key | vlen (8b)   | value |
+----------+---------+-------------+-----+-------------+-------+
```

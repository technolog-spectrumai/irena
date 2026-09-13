# Block validation

`prunella_verify::check_block` is the single implementation of what makes a block
acceptable. The store's acceptance policy, full-chain verification and XML import all
call it; none reimplements any part of it. Two components that judged blocks separately
could drift, and the core invariant would be the thing that broke.

Every rule is evaluated on every block. The function never stops at the first failure,
so one pass reports every defect rather than making an operator fix them one at a time.

## The rules

| Rule | Finding when broken |
|---|---|
| Header version is one this build understands | `unsupported_header_version` |
| Block's network id matches the chain's | `network_mismatch` |
| Height is `parent.height + 1`, or 0 with no parent | `height_out_of_order` |
| `previous_hash` equals the parent's block hash, or zero at genesis | `previous_hash_mismatch` |
| Recomputed hash matches the hash the block was looked up by | `block_hash_mismatch` |
| `tx_count` equals the number of transactions carried | `tx_count_mismatch` |
| Recomputed transaction root matches `tx_root` | `tx_root_mismatch` |
| Each transaction's recomputed id matches its declared id | `tx_id_mismatch` |
| Each signature verifies against its signer over the recomputed signing message | `signature_invalid` |
| No transaction id appears twice in one block | `duplicate_tx_id_in_block` |
| No transaction id is already committed in the chain | `duplicate_tx_id_in_chain` |
| `timestamp_millis` is not earlier than the parent's | `timestamp_regression` |

Four further findings come from reading the chain rather than from a block's contents:
`missing_block` when a height inside the verified range holds nothing, `decode_error`
when stored bytes are not a block, `source_failure` when the chain cannot be read at
all, and `genesis_mismatch` when the block at height 0 is not the genesis the chain
recorded. Sixteen kinds in total.

### On timestamps

`timestamp_millis` is declared by whoever produced the block. Prunella attaches no trust
to it. The only rule applied is that it may not go backwards, which is deterministic and
locally checkable from the parent alone. Anything stronger — clock skew bounds, drift
limits, agreement between producers — is a consensus concern and is deliberately absent.

### On nonces

The nonce is carried and hashed but never interpreted. Prunella does not enforce nonce
ordering or per-signer uniqueness: doing so would require account state, which is an
application concern. The field exists so that two otherwise identical payloads from the
same signer produce distinct transaction ids.

### Why three overlapping transaction rules

The transaction root commits to declared ids, not to payloads. So:

* rewriting a payload leaves the root intact, and is caught by the id derivation and by
  the signature;
* rewriting a declared id breaks the root;
* swapping a signature breaks the id, because the id commits to the signature.

No single rule covers all three, which is why all three exist.

## Reports

`verify_chain` walks genesis to head and returns a `VerificationReport`:

```
network:     demo
genesis:     d8ca680db344eb8be3587b7a92a6b0afae47923eb2e854513d9b7c7d81203463
head:        height 3 (749c6a36…)
checked:     heights 0..=3
blocks:      4
txs:         3
result:      valid
```

`is_valid()` is true only when no finding was produced. There is no warning level: a
ledger that is almost consistent is inconsistent.

Every finding carries a `Location` with whatever coordinates are exact: height, block
hash, transaction index, transaction id. `detail` is human text — it is never hashed,
never parsed, and never part of any decision the code makes.

### Cascades are reported, not hidden

Rewriting a block changes its hash, so its child no longer links to it. Verification
reports both, each at its own height. That is deliberate: the second finding is real,
and suppressing it would mean deciding which damage is "the cause", which verification
is not in a position to know.

### Missing blocks end the walk

A block that cannot be read ends the pass at that height. Every later block's parent is
unknown, so continuing would produce a cascade of findings all describing the same one
gap. The report says how far it got via `range_end`.

## Scope

`check_block` is a pure function of the block, its parent and the transaction ids
already committed. It never consults a clock, a network, a configuration file or a
random source. Two instances given the same inputs always return the same findings.

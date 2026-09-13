# Canonical serialization and hashing

Every hash in Prunella is derived from a canonical byte encoding. No hash is ever
computed from `Debug`, `Display`, JSON or XML text. This document specifies the
encoding precisely enough to reimplement in another language, because the core
invariant depends on independent implementations agreeing byte for byte.

## The encoding

The encoding is [Borsh](https://borsh.io), restricted to a subset with exactly one
representation per value:

| Rust type | Encoding |
|---|---|
| `u16`, `u32`, `u64` | fixed width, little-endian |
| `[u8; N]` | the N bytes, verbatim |
| `Vec<u8>` | `u32` little-endian length, then the bytes |
| `String` | `u32` little-endian length, then UTF-8 bytes |
| `Vec<T>` | `u32` little-endian length, then each element |
| `struct` | each field in declaration order, no padding, no tags |

Types that are hashed or signed **must not** contain maps, sets, floating point numbers
or `Option`s. Map and set iteration order is not guaranteed; floats have several bit
patterns for one value; `Option` invites an encoder to treat an absent value and a
default value as interchangeable.

Decoding rejects trailing bytes. A byte string that decodes to a value followed by
anything else is not canonical, even when the prefix is perfectly valid.

### Worked example

```rust
struct Sample { a: u64, b: Vec<u8>, c: String }
Sample { a: 1, b: vec![0xde, 0xad], c: "hi".into() }
```

```
0100000000000000  a = 1, little-endian u64
02000000          b.len() = 2, little-endian u32
dead              b
02000000          c.len() = 2, little-endian u32
6869              c, UTF-8
```

This vector is asserted in `crates/prunella-canonical/tests/canonical.rs`, derived by
hand from the table above rather than captured from a run.

## Domain-separated hashing

```
hash_domain(tag, bytes) = BLAKE3( u32_le(tag.len()) || tag || bytes )
```

The tag is length-prefixed so that no tag can be a prefix of another tag plus payload:
without it, tag `"ab"` over payload `"c"` and tag `"a"` over payload `"bc"` would share
a pre-image.

| Tag | Purpose |
|---|---|
| `PRUNELLA/v1/tx-sign` | the message a signer signs |
| `PRUNELLA/v1/tx-id` | a transaction identifier |
| `PRUNELLA/v1/tx-root` | the root of an empty transaction tree |
| `PRUNELLA/v1/tx-leaf` | a Merkle leaf: one transaction id |
| `PRUNELLA/v1/tx-node` | a Merkle interior node |
| `PRUNELLA/v1/block-header` | a block hash |

All digests are 32 bytes and render as exactly 64 lowercase hex characters. Uppercase
hex is rejected on input, so each value has exactly one textual form.

## The four derivations

### Signing message

```
signing_message = hash_domain("PRUNELLA/v1/tx-sign", encode(TxSigningBody))

TxSigningBody { namespace: String, schema_version: u32, payload: Vec<u8>,
                signer: [u8; 32], nonce: u64 }
```

Signers sign this 32-byte digest, not the full pre-image, so signing cost does not grow
with payload size.

### Transaction id

```
tx_id = hash_domain("PRUNELLA/v1/tx-id", encode(TxIdBody))

TxIdBody { namespace: String, schema_version: u32, payload: Vec<u8>,
           signer: [u8; 32], nonce: u64, signature: [u8; 64] }
```

The id commits to the signature as well as to the signed body. Swapping a valid
signature for another therefore changes the id, and is caught by the id check before
signature verification even runs. Ed25519 signing is deterministic and verification is
strict, so one signed message has exactly one acceptable signature encoding — which is
what makes it safe for the id to depend on it.

### Transaction root

A binary Merkle tree over the transaction ids in block order, with the shape of
RFC 6962:

```
leaf(id)           = hash_domain("PRUNELLA/v1/tx-leaf", id)
node(l, r)         = hash_domain("PRUNELLA/v1/tx-node", l || r)

root([])           = hash_domain("PRUNELLA/v1/tx-root", "")
root([id])         = leaf(id)
root(ids), n > 1   = node(root(ids[..k]), root(ids[k..]))   where k = 2^⌊log2(n-1)⌋
```

`k` is the largest power of two strictly below `n`, so the left subtree is always
complete and the right holds the remainder. Nothing is padded or duplicated to reach a
power of two, which is what rules out the classic second-preimage trick where
duplicating the last leaf reproduces the original root.

Leaves, interior nodes and the empty tree are in three different domains, so a leaf can
never be reinterpreted as a node and the empty root can never equal a leaf.

The tree is what makes **inclusion proofs** possible: see
[`prunella_core::merkle`](../crates/prunella-core/src/merkle.rs) and PROTOCOL_V1.md §7.1.
A proof verifies against a block header alone — `tx_root` and `tx_count` — without the
block's transactions.

Note what the root does and does not cover. It commits to *declared* transaction ids,
not to payloads. Rewriting a payload leaves the root intact; it is caught by the id
derivation and by the signature. All three rules exist because each covers what the
others do not.

### Block hash

```
block_hash = hash_domain("PRUNELLA/v1/block-header", encode(BlockHeader))

BlockHeader { version: u16, network_id: String, height: u64, previous_hash: [u8; 32],
              tx_root: [u8; 32], tx_count: u32, timestamp_millis: u64 }
```

The header commits to the chain identity, the position, the parent and — through
`tx_root` — every transaction in order. A block's hash is its header's hash.

## Genesis

A genesis block is derived from a `GenesisSpec { network_id, timestamp_millis,
transactions }` at height 0 with an all-zero previous hash. The same specification
always yields the same genesis hash, which is where the core invariant begins.

Tooling defaults `timestamp_millis` to zero so that a chain identifier alone determines
a genesis hash. This is verifiable:

```console
$ prunella --chain any.chain init --network prunella.example
genesis: 4cfcf0687ebd6e97e1ae8aab69ed46793088cfb705c63c91475e1fd75fed507c
```

That value is pinned as a regression vector in
`crates/prunella-core/tests/derivation.rs`.

## Stability

This document describes the implementation. The **normative** specification is
[`PROTOCOL_V1.md`](../PROTOCOL_V1.md), and it is frozen.

Borsh is pinned to a single major version, and the encoding, the domain tags and every
derivation are covered by permanent golden vectors under
[`test-vectors/v1/`](../test-vectors/v1/). Those vectors are checked against the
implementation *and* against an independent encoder written from the specification
alone, so a dependency upgrade that changed a layout, a hash or a signature fails a test
naming the vector and the field that moved. See
[`crates/prunella-conformance`](../crates/prunella-conformance).

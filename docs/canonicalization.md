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
| `PRUNELLA/v1/tx-root` | a block's transaction root |
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

```
tx_root = hash_domain("PRUNELLA/v1/tx-root",
                      u32_le(count) || tx_id[0] || tx_id[1] || … || tx_id[n-1])
```

A linear digest over the ordered ids. The count is absorbed first, so a shorter list
can never share a pre-image with a longer one. There is no Merkle tree: nothing in
Prunella needs inclusion proofs yet, and a naively built Merkle tree is a
second-preimage hazard. A Merkle root is the intended option for header version 2, at
which point both roots would be distinguishable by the header version.

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
genesis: 012673fe1d4bd19b206c326ae34913cf285673955f03f022352a4524e2641b84
```

That value is pinned as a regression vector in
`crates/prunella-core/tests/derivation.rs`.

## Stability

Borsh is pinned to a single major version, and the encoding, the domain tags and the
derivations are covered by golden vectors. Any change to any of them breaks those tests
loudly, because such a change silently rewrites every hash in every existing chain.

# Prunella Protocol V1

**Status: frozen.** Every rule in this document is fixed. The types, field orders,
encodings, domain tags and derivations below may not change. A change to any of them
is not a revision of V1; it is a V2, introduced as new types alongside these.

The normative test vectors for this document live in [`test-vectors/v1/`](test-vectors/v1/)
and are checked by `crates/prunella-conformance` against both the implementation and an
independent encoder written from this text alone.

---

## 1. Primitives

### 1.1 Integers

All integers are unsigned, fixed width, **little-endian**. No varints, no signed types,
no floats anywhere in a hashed structure.

| Type | Width |
|---|---|
| `u8` | 1 byte |
| `u16` | 2 bytes |
| `u32` | 4 bytes |
| `u64` | 8 bytes |

### 1.2 Fixed-size byte arrays

`[u8; N]` is encoded as its `N` bytes, verbatim, with no length prefix.

### 1.3 Variable-length byte strings and text

`bytes` is a `u32` little-endian length followed by that many bytes.
`string` is a `u32` little-endian length followed by that many bytes of UTF-8. The
length counts bytes, not characters.

### 1.4 Sequences

`vec<T>` is a `u32` little-endian element count followed by each element encoded in
turn.

### 1.5 Structures

A structure is its fields encoded in **declaration order**, concatenated, with no tags,
padding, alignment or field count.

### 1.6 Enumerations

An enumeration is a `u8` discriminant, numbered from zero in declaration order,
followed by the variant's fields (none, for every V1 enumeration).

### 1.7 Borsh

The above is exactly the [Borsh](https://borsh.io) encoding restricted to these
constructs. V1 permits **no** maps, sets, options, floats, signed integers, `u128` or
unit types in any hashed structure. Decoders must reject input that is not consumed
completely: trailing bytes make a byte string non-canonical even when a prefix of it
decodes.

### 1.8 Hashing

The hash function is **BLAKE3** with a 32-byte output. Every hash in V1 is
domain-separated:

```
hash_domain(tag, data) = BLAKE3( u32_le(len(tag)) || tag || data )
```

where `tag` is one of the ASCII strings in §2 and `||` is concatenation. The length
prefix on the tag makes `(tag, data)` unambiguous: no tag can be a prefix of another
tag plus data.

Hashes are rendered as exactly 64 lowercase hexadecimal characters. Uppercase is not
accepted on input.

### 1.9 Signatures

Signatures are **Ed25519** (RFC 8032, pure, no pre-hash) over the 32-byte signing
message of §5. Public keys are 32 bytes, signatures 64 bytes. Verification is strict:
small-order public keys and non-canonically encoded signature scalars are rejected.
Because Ed25519 signing is deterministic, one key and one message yield exactly one
acceptable signature.

## 2. Domain tags

| Tag | Used for |
|---|---|
| `PRUNELLA/v1/tx-sign` | transaction signing message (§5) |
| `PRUNELLA/v1/tx-id` | transaction id (§6) |
| `PRUNELLA/v1/tx-leaf` | Merkle leaf over one transaction id (§7) |
| `PRUNELLA/v1/tx-node` | Merkle interior node (§7) |
| `PRUNELLA/v1/tx-root` | root of the empty transaction tree (§7) |
| `PRUNELLA/v1/block-header` | block hash (§8) |

## 3. Labels

A **label** — used for network ids and namespaces — is a `string` of 1 to 64 bytes
whose first byte is `a`–`z` or `0`–`9` and whose remaining bytes are `a`–`z`, `0`–`9`,
`.`, `_` or `-`. Uppercase is rejected rather than folded, so a label has exactly one
textual form. Labels carry no meaning to the protocol; they are compared for equality
only.

## 4. Canonical types

Field order is normative. `String` means §1.3 `string`; `Vec<u8>` means §1.3 `bytes`.

### 4.1 `TxSigningBody` — the signing pre-image

| # | Field | Type |
|---|---|---|
| 1 | `namespace` | `String` (a label) |
| 2 | `schema_version` | `u32` |
| 3 | `payload` | `Vec<u8>` |
| 4 | `signer` | `[u8; 32]` |
| 5 | `nonce` | `u64` |

### 4.2 `TxIdBody` — the id pre-image

| # | Field | Type |
|---|---|---|
| 1 | `namespace` | `String` (a label) |
| 2 | `schema_version` | `u32` |
| 3 | `payload` | `Vec<u8>` |
| 4 | `signer` | `[u8; 32]` |
| 5 | `nonce` | `u64` |
| 6 | `signature` | `[u8; 64]` |

### 4.3 `TransactionV1` — the stored and transported transaction

| # | Field | Type |
|---|---|---|
| 1 | `id` | `[u8; 32]` |
| 2 | `namespace` | `String` (a label) |
| 3 | `schema_version` | `u32` |
| 4 | `payload` | `Vec<u8>` |
| 5 | `signer` | `[u8; 32]` |
| 6 | `nonce` | `u64` |
| 7 | `signature` | `[u8; 64]` |

`id` must equal §6 and `signature` must verify per §5; a transaction where either does
not hold is invalid. `payload`, `schema_version` and `nonce` are opaque: the protocol
stores and hashes them and assigns them no meaning.

### 4.4 `BlockHeaderV1`

| # | Field | Type |
|---|---|---|
| 1 | `version` | `u16`, always `1` |
| 2 | `network_id` | `String` (a label) |
| 3 | `height` | `u64` |
| 4 | `previous_hash` | `[u8; 32]` |
| 5 | `tx_root` | `[u8; 32]` |
| 6 | `tx_count` | `u32` |
| 7 | `timestamp_millis` | `u64` |

### 4.5 `BlockV1`

| # | Field | Type |
|---|---|---|
| 1 | `header` | `BlockHeaderV1` |
| 2 | `transactions` | `vec<TransactionV1>` |

`header.tx_count` must equal `len(transactions)` and `header.tx_root` must equal §7
over the transactions' ids in order.

### 4.6 `InclusionProofV1`

| # | Field | Type |
|---|---|---|
| 1 | `index` | `u32` |
| 2 | `steps` | `vec<ProofStep>` |

`ProofStep` is `side: Side` (`u8`: `0` = Left, `1` = Right) then `hash: [u8; 32]`.

## 5. Transaction signing message

```
signing_message = hash_domain("PRUNELLA/v1/tx-sign", encode(TxSigningBody))
```

The signer signs these 32 bytes with Ed25519. The message covers every field a
transaction carries except its id and its signature.

## 6. Transaction id

```
tx_id = hash_domain("PRUNELLA/v1/tx-id", encode(TxIdBody))
```

The id commits to the signature as well as the signed body. Two transactions with the
same body but different signatures have different ids.

## 7. Transactions root

The root is a binary Merkle tree over the transaction ids in block order, with the
shape of RFC 6962 §2.1:

```
leaf(id)              = hash_domain("PRUNELLA/v1/tx-leaf", id)
node(l, r)            = hash_domain("PRUNELLA/v1/tx-node", l || r)

root([])              = hash_domain("PRUNELLA/v1/tx-root", "")
root([id])            = leaf(id)
root(ids), n = len(ids) > 1:
    k = largest power of two with k < n
    root(ids) = node(root(ids[0..k]), root(ids[k..n]))
```

The left subtree is always complete; the right subtree carries the remainder. Nothing
is padded or duplicated to reach a power of two. Leaves, nodes and the empty root are
in three different domains and cannot be confused with one another.

### 7.1 Inclusion proofs

The proof for the leaf at `index` is its audit path, leaf-most step first: at each
level, the root of the sibling subtree and which side it is on. Verification takes the
proof, the transaction id, and from the block header `tx_count` and `tx_root`:

1. `index < tx_count`, else reject.
2. The proof's side sequence must equal the side sequence that `(tx_count, index)`
   implies by the recursion above, else reject. This is computed from the shape alone.
3. Fold: start from `leaf(id)`; for each step, `running = node(step.hash, running)` if
   the side is Left, `node(running, step.hash)` if Right.
4. The result must equal `tx_root`, else reject.

Altering the id, the index, any step's hash or side, or the number of steps is caught
by step 2 or step 4.

What binds a proof to a particular block is the **root**, not the count. Step 2 rejects
an out-of-range index and a path whose shape is impossible for the claimed position, but
it is not an independent binding to tree size: some sizes share a path shape for a given
index — trees of 3 and 4 leaves both give index 0 a two-step path. Since a block header
carries `tx_root` and `tx_count` together, and two different transaction lists have
different roots, a proof still cannot be replayed from one block to another.

## 8. Block hash

```
block_hash = hash_domain("PRUNELLA/v1/block-header", encode(BlockHeaderV1))
```

A block's hash is its header's hash. The header commits to the chain (`network_id`),
the position (`height`), the parent (`previous_hash`) and, through `tx_root`, every
transaction in order.

## 9. Genesis

A genesis block is `BlockV1` with:

* `header.version = 1`
* `header.height = 0`
* `header.previous_hash = [0u8; 32]`
* `header.network_id`, `header.timestamp_millis` and `transactions` from the genesis
  specification
* `header.tx_root` and `header.tx_count` derived per §7 and §4.5

The **genesis hash** is the block hash of that block. Two parties given the same
specification derive the same genesis hash without communicating.

## 10. Validity of a block against its parent

A block is valid against a parent header when all of the following hold. A genesis
block has no parent; the parent-dependent rules then require `height = 0` and
`previous_hash = [0u8; 32]`.

1. `header.version = 1`.
2. `header.network_id` equals the chain's network id.
3. `header.height = parent.height + 1`.
4. `header.previous_hash = block_hash(parent)`.
5. `header.timestamp_millis >= parent.timestamp_millis`.
6. `header.tx_count = len(transactions)`.
7. `header.tx_root = root(ids)` per §7.
8. For every transaction: `id` equals §6, and `signature` verifies per §5.
9. No transaction id appears twice in the block.
10. No transaction id is already committed anywhere earlier in the chain.

The protocol does not interpret `timestamp_millis` beyond rule 5, does not interpret
`nonce` at all, and does not inspect `payload`.

## 11. Limits

The protocol itself imposes no size limits beyond those implied by the types: a label
is at most 64 bytes, a `u32` length prefix bounds byte strings and sequences at
2³²−1. Implementations must bound the *input* they accept from untrusted sources —
document size, decoded allocation — but such bounds are implementation policy, not
protocol rules, and two implementations with different bounds still agree on every
hash.

## 12. What V1 does not specify

* Consensus, finality, fork choice, validator sets.
* Any meaning for `payload`, `namespace`, `schema_version` or `nonce`.
* Storage layout or transport format. The XML transport is separately versioned and is
  never a hash pre-image.
* Time. `timestamp_millis` is a producer's claim, constrained only to be non-decreasing.

## 13. Assumptions

* BLAKE3 is collision- and second-preimage-resistant at 256 bits.
* Ed25519 strict verification accepts exactly one encoding per valid signature, so the
  transaction id's dependence on signature bytes is well defined.
* A `u32` transaction count per block and `u64` heights are sufficient for any chain V1
  will hold.
* Implementations never hash a `Debug`, `Display`, JSON or XML rendering; only §1
  encodings are hash pre-images.

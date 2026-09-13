# XML format

Prunella exports and imports chains as XML. The schema is published as
[`schemas/prunella-chain-v1.xsd`](../schemas/prunella-chain-v1.xsd).

## XML is transport, never authority

A document carries the hashes its producer derived. An importer never believes them.
Every block is rebuilt into `prunella-core` types and re-derived through
`prunella-canonical`; the declared hashes are compared against those derivations and
then discarded. No chain hash is ever computed from XML text, so indentation, attribute
order, escaping and line wrapping cannot change a single hash.

## Versioning

* XML namespace: `urn:prunella:chain:1`
* `format-version="1"`

The namespace and the `format-version` attribute move together. A document declaring an
unknown `format-version` is refused outright rather than read on a best-effort basis: a
backup that silently ignores the parts it does not recognise is not a backup.

A future version 2 would use `urn:prunella:chain:2`, so a version 1 reader rejects it at
the namespace check and never has to guess.

## Document shape

```xml
<?xml version="1.0" encoding="UTF-8"?>
<prunella-chain xmlns="urn:prunella:chain:1" format-version="1" kind="full"
                network-id="demo"
                genesis-hash="d8ca680db344eb8be3587b7a92a6b0afae47923eb2e854513d9b7c7d81203463"
                range-start="0" range-end="3" block-count="4"
                exported-at-millis="1789260227010">
  <block height="1" hash="3f6b4217c573fbf4261d7a589ead13d0345997ca1dc176be1d700cd130a3f403">
    <header version="1"
            previous-hash="d8ca680db344eb8be3587b7a92a6b0afae47923eb2e854513d9b7c7d81203463"
            tx-root="754153da01e27dad4289fb82668f2e03a10642d0fa2ffb1b9829613eb84085a1"
            tx-count="1" timestamp-millis="1000"/>
    <transactions>
      <transaction index="0"
                   id="b62ac27081b4c1bc59fce99b60579448de1d550b575dedd7af95c2d25fb9853a"
                   namespace="app.demo" schema-version="1" nonce="1">
        <signer>mTU7PUOCjIeOUbCgu+6xG/RBc36/L6R4Xwc6z63sYFQ=</signer>
        <payload>SGVsbG8=</payload>
        <signature>q38M+Z+2McEbZBPHQV2mww…</signature>
      </transaction>
    </transactions>
  </block>
</prunella-chain>
```

* Hashes and ids: exactly 64 lowercase hex characters.
* Keys, signatures and payloads: `xs:base64Binary`. Whitespace inside base64 content is
  stripped before decoding, as XSD permits, so the document can be indented for reading
  without changing a single payload byte.
* An empty byte string is written as an empty element (`<payload/>`), so "no payload"
  and "payload of zero bytes" never depend on whitespace handling.
* `exported-at-millis` is the producer's clock. It is informational: never hashed, never
  verified, never interpreted.

## Kinds

| `kind` | Contents | Can be imported | Can create a chain |
|---|---|---|---|
| `full` | genesis through head | yes | yes |
| `range` | a contiguous height range | yes | no |
| `projection` | namespace-filtered | **no** | **no** |

### Projections are not backups

A projection keeps only transactions in one namespace. Its blocks therefore carry fewer
transactions than their headers commit to, and can no longer reproduce their own
`tx-root`. That is not a defect to work around — it is why a projection can never round
trip, and the format says so in three places:

* `kind="projection"` on the root;
* a mandatory `<projection filter-namespace="…"/>` element, required by the XSD;
* `included-count` on each `<transactions>` element, recording how many survived the
  filter, while `tx-count` on the header still records the block's true count.

Blocks with no matching transactions are still present, with an empty `<transactions/>`,
so heights stay contiguous and a reader never mistakes a filtered block for a missing
one. The importer refuses a projection outright, and `prunella export --namespace`
prints a warning saying so.

## Import semantics

* **Atomic.** Blocks are committed in one database transaction. A rejection anywhere
  leaves the chain with nothing written.
* **Idempotent.** A block already committed with identical contents is skipped and
  counted. Re-importing a document that was already applied succeeds as a no-op.
* **Dry-runnable.** `plan_import` runs every check a real import runs and writes
  nothing, so "the dry run passed" means the import will be accepted for the same
  reasons.

### What is refused

| Condition | Error |
|---|---|
| Malformed XML, unknown element or attribute, missing attribute | `Malformed` |
| Wrong XML namespace, or `format-version` this build does not read | `Malformed`, `UnsupportedFormatVersion` |
| Heights inside the document are not a contiguous ascending run | `NonContiguousDocument` |
| Declared range does not match the blocks carried | `Malformed` |
| Block contents do not hash to the declared block hash | `DeclaredHashMismatch` |
| Document is from another network | `NetworkMismatch` |
| Document is from a chain with another genesis | `GenesisMismatch` |
| Document does not continue the chain | `Gap` |
| Document contradicts a committed block | `Fork` |
| Any block breaks a deterministic rule | `InvalidBlocks`, carrying the findings |
| The document is a projection | `NotImportable` |
| Input exceeds the size limit (default 1 GiB) | `TooLarge` |

Note which error catches which tampering. Rewriting a header changes the block hash and
is caught as `DeclaredHashMismatch`. Rewriting a payload does *not* change the block
hash — it is caught as `InvalidBlocks`, by the transaction id derivation and the
signature. Both layers are needed.

## The schema and the parser

Rust has no mature XSD validator, so the importer implements the equivalent structural
checks in code and the XSD is the published contract for other tooling. The two are kept
honest by a test that validates full, range and projection exports with `xmllint` where
it is installed, and skips cleanly where it is not.

The schema constrains shape and lexical form only. It cannot establish that a document
is a valid chain — hashes, ids, signatures, linkage and ordering are verified by
re-derivation. A document that validates against the schema may still be rejected on
import, and that division of labour is intended.

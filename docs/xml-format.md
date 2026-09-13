# XML format

Prunella exports and imports chains as XML. The schema is published as
[`schemas/prunella-chain-v2.xsd`](../schemas/prunella-chain-v2.xsd);
[`prunella-chain-v1.xsd`](../schemas/prunella-chain-v1.xsd) describes version 1
documents, which this build still reads.

## XML is transport, never authority

A document carries the hashes its producer derived. An importer never believes them.
Every block is rebuilt into `prunella-core` types and re-derived through
`prunella-canonical`; the declared hashes are compared against those derivations and
then discarded. No chain hash is ever computed from XML text, so indentation, attribute
order, escaping and line wrapping cannot change a single hash.

## Versioning

| Version | Namespace | Written | Read |
|---|---|---|---|
| 1 | `urn:prunella:chain:1` | no | yes |
| 2 | `urn:prunella:chain:2` | **yes** | yes |

The namespace and the `format-version` attribute move together, and a document whose
namespace does not match its declared version is malformed. A document declaring a
`format-version` this build does not read is refused outright rather than read on a
best-effort basis: a backup that silently ignores the parts it does not recognise is not
a backup.

Version 2 differs from version 1 in exactly one place: the `<payload>` element may
carry its bytes as a nested XML element instead of base64 (below). Every hash, id,
signature and rule is unchanged; PROTOCOL_V1 is not touched by the transport version.

## Document shape

```xml
<?xml version="1.0" encoding="UTF-8"?>
<prunella-chain xmlns="urn:prunella:chain:2" format-version="2" kind="full"
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
        <payload encoding="base64">SGVsbG8=</payload>
        <signature>q38M+Z+2McEbZBPHQV2mww…</signature>
      </transaction>
    </transactions>
  </block>
</prunella-chain>
```

* Hashes and ids: exactly 64 lowercase hex characters.
* Keys and signatures: `xs:base64Binary`. Whitespace inside base64 content is stripped
  before decoding, as XSD permits, so the document can be indented for reading without
  changing a single byte.
* Payloads: base64 by default, or nested XML — see the next section.
* An empty byte string is written as an empty element (`<payload/>`), so "no payload"
  and "payload of zero bytes" never depend on whitespace handling.
* `exported-at-millis` is the producer's clock. It is informational: never hashed, never
  verified, never interpreted.

## Nested payloads

A payload is opaque bytes and the chain commits to exactly those bytes. Base64 is
always correct and always unreadable. Since version 2, a payload that *is* an XML
element travels as that element, so an application's records are legible inside the
block that holds them:

```xml
<payload encoding="xml"><irena-record version="1.0" kind="share-structure" company="acme">…</irena-record></payload>
<payload encoding="base64">SGVsbG8=</payload>
```

`encoding` defaults to `base64` when absent. In a version 1 document the attribute does
not exist, and one appearing there is refused as an unknown attribute.

**Prunella still never interprets a payload.** What makes nesting safe is that neither
side ever re-serialises the element:

* The **exporter** decides by the bytes alone (`PayloadEncoding::choose`), so two
  exporters of one chain write one document. A payload is nested if and only if it is
  valid UTF-8 and consists of exactly one well-formed element with nothing before or
  after it — no whitespace, no declaration, no comment beside it. It is then written
  straight into the output, unescaped and unindented.
* The **importer** does not rebuild the element from parse events, because a parser
  normalises line endings, attribute quoting and entity spelling and the chain
  committed to the original bytes. It uses the parser only to find where the element
  starts and ends, and takes the exact slice of the source text between those points
  as the payload. Whitespace between the payload tags and the element is dropped, so a
  reformatted document still yields the same bytes; anything else beside the element
  (text, a comment, a second element) is refused as ambiguous. The slice is re-checked
  to be one well-formed element, and the transaction id then commits to it as it does
  to every payload, so a nested payload that was edited in any way — reflowed,
  requoted, respelled — is caught as `InvalidBlocks` exactly like a tampered base64
  one.

Everything that cannot survive that trip goes as base64: an empty payload, binary,
prose, a document with an XML declaration, XML with surrounding whitespace, an
undeclared entity, a second element. The nested element is the application's, in
whatever namespace it chooses or none. It inherits nothing from the chain document
and is not validated against the chain schema (`processContents="skip"`).

`MAX_BINARY_FIELD_CHARS` bounds a nested payload's source length exactly as it bounds a
base64 one.

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
| Input exceeds the byte limit (default 1 GiB) | `TooLarge` |
| Declared or carried blocks exceed 16,000,000 | `TooManyBlocks` |
| A block carries more than 4,000,000 transactions | `TooManyTransactions` |
| One base64 or nested-payload element exceeds 96 MiB of characters | `FieldTooLarge` |
| A nested payload holds anything but one element and whitespace, or an unknown `encoding` | `Malformed` |
| `block-count` disagrees with the declared height range | `Malformed` |

Note which error catches which tampering. Rewriting a header changes the block hash and
is caught as `DeclaredHashMismatch`. Rewriting a payload does *not* change the block
hash — it is caught as `InvalidBlocks`, by the transaction id derivation and the
signature. Both layers are needed.

### Input limits

A document is untrusted input, so the reader bounds what it will accept before it
allocates for it:

| Limit | Default | Why it is separate |
|---|---|---|
| `DEFAULT_MAX_DOCUMENT_BYTES` | 1 GiB | Checked before parsing begins |
| `MAX_BLOCKS` | 16,000,000 | `block-count` is an attribute the document supplies; without a cap, a few bytes could ask for capacity for billions of blocks |
| `MAX_TRANSACTIONS_PER_BLOCK` | 4,000,000 | Each transaction costs at least a key and a signature |
| `MAX_BINARY_FIELD_CHARS` | 96 MiB | Bounds one element independently, so a single enormous `<payload>` cannot force a large allocation from an otherwise small document |

`block-count` is additionally required to equal `range-end - range-start + 1`, so the
declared size cannot disagree with the declared range.

These are **import policy, not protocol rules** (PROTOCOL_V1.md §11). Two
implementations with different bounds still agree on every hash; they only differ in
what they are willing to read.

## The schema and the parser

Rust has no mature XSD validator, so the importer implements the equivalent structural
checks in code and the XSD is the published contract for other tooling. The two are kept
honest by tests that validate full, range, projection and nested-payload exports, and a
version 1 document, with `xmllint` where it is installed, and skip cleanly where it is
not.

The schema constrains shape and lexical form only. It cannot establish that a document
is a valid chain — hashes, ids, signatures, linkage and ordering are verified by
re-derivation. A document that validates against the schema may still be rejected on
import, and that division of labour is intended.

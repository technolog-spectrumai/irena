# Prunella Protocol V1 test vectors

These files are the normative examples for [`PROTOCOL_V1.md`](../../PROTOCOL_V1.md).
They are **frozen**. A change to any byte in any of them is a protocol break.

Each file is self-contained: `block.input` is everything needed to rebuild the block
(network id, height, previous hash, timestamp, and for each transaction its namespace,
schema version, payload, signer key and nonce; keys are given as seeds so signatures
are reproducible), and `block.expected` is everything the block derives — canonical
bytes for every structure, every hash, every signature, every leaf, and an inclusion
proof for every transaction — as lowercase hex.

`cargo test -p prunella-conformance` holds these files against the implementation and
against an independent encoder written from the specification alone. If a dependency
upgrade ever changed an encoding, a hash or a signature, the test names the vector and
the field that moved.

`cargo run -p prunella-conformance --bin generate-v1-vectors` regenerates them. A test
asserts regeneration reproduces the committed files byte for byte, so the generator can
never quietly change a vector: if it disagrees with the files, the implementation has
changed, and that is a break to revert or a V2 to introduce.

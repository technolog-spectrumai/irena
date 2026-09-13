# CLI usage

The `prunella` binary is a thin front end over the libraries. It contains no hashing,
validation, encoding or storage logic: a second implementation of any of those would be
a second answer to what a chain says, and a ledger can only have one.

## Global options

| Option | Meaning |
|---|---|
| `--chain <path>` | Chain file. Environment: `PRUNELLA_CHAIN`. Default: `prunella.chain` |
| `--json` | Machine-readable output instead of text |

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success |
| `1` | The chain is invalid, or the lookup found nothing |
| `2` | The command could not be carried out |

`1` and `2` are separate so a script can tell "the chain is bad" from "the command could
not run".

## Commands

### `init`

```console
$ prunella --chain demo.chain init --network demo
created demo.chain
network: demo
genesis: d8ca680db344eb8be3587b7a92a6b0afae47923eb2e854513d9b7c7d81203463
```

`--genesis-timestamp <ms>` defaults to `0` so that a network identifier alone determines
the genesis hash. Running `init --network demo` on any machine, at any time, produces
the hash above. That reproducibility is the core invariant at its root, so change the
default only when you specifically want a genesis that cannot be re-derived from the
name.

### `keygen`

```console
$ prunella keygen --out signer.key
wrote signer.key
public key: 99353b3d43828c878e51b0a0bbeeb11bf441737ebf2fa4785f073acfadec6054
```

Writes a 64-character hex ed25519 seed, `0600` where the platform supports it. Without
`--out` the seed goes to standard output. Prunella has no key-management policy beyond
the file format: where a key lives and who may read it are your decisions.

### `append`

```console
$ prunella --chain demo.chain append --signing-key signer.key \
      --namespace app.demo --nonce 1 --payload-hex 48656c6c6f
appended block 3f6b4217… at height 1
transactions: 1
  b62ac27081b4c1bc59fce99b60579448de1d550b575dedd7af95c2d25fb9853a
```

| Option | Meaning |
|---|---|
| `--signing-key <file>` | Key to sign with |
| `--namespace <label>` | Application domain label. Never interpreted |
| `--schema-version <n>` | Payload schema version, default `1`. Never interpreted |
| `--nonce <n>` | Signer-scoped ordinal, default `0` |
| `--payload-file` / `--payload-hex` / `--payload-base64` | Payload source, mutually exclusive |
| `--tx-file <file>` | Append a pre-signed transaction in canonical encoding. Repeatable |
| `--timestamp <ms>` | Block timestamp. Defaults to the system clock |

No payload source means an empty payload, which is a legitimate transaction: Prunella
never reads payload bytes, so it has no opinion about how many there should be.

`--tx-file` lets a signing key stay on a machine that never touches the chain. The file
holds one canonically encoded `Transaction`.

The block is validated by the acceptance policy before anything is written. A rejected
block leaves the chain untouched.

### `status`

```console
$ prunella --chain demo.chain status
path:              demo.chain
network:           demo
genesis:           d8ca680db344eb8be3587b7a92a6b0afae47923eb2e854513d9b7c7d81203463
head height:       3
head hash:         749c6a368a09bb7fc113db2266c85719516bf1d723d5c7f77e8603eb2453b37a
blocks:            4
transactions:      3
store format:      1
acceptance policy: local-deterministic
```

Reads bookkeeping only. It does not walk the chain — that is `verify`.

### `verify`

```console
$ prunella --chain demo.chain verify
network:     demo
genesis:     d8ca680db344eb8be3587b7a92a6b0afae47923eb2e854513d9b7c7d81203463
head:        height 3 (749c6a36…)
checked:     heights 0..=3
blocks:      4
txs:         3
result:      valid
```

Walks genesis to head, re-deriving every hash, id, root and signature. Exits `1` with
every finding and its exact location when anything is wrong.

| Option | Meaning |
|---|---|
| `--from` / `--to` | Verify a contiguous range. Genesis identity is still checked |
| `--skip-duplicate-check` | Skip detection of a transaction committed in two blocks |

The duplicate check holds every transaction id seen so far in memory. Skip it for a
cheap spot check of a very long chain; leave it on for a real audit, because a replayed
transaction is a genuine ledger defect.

### `block`

```console
$ prunella --chain demo.chain block 2
$ prunella --chain demo.chain block 607670855596c87b…
```

Takes a height or a 64-character block hash. `--transactions` includes every transaction
in full. Exits `1` if nothing matches.

### `tx`

```console
$ prunella --chain demo.chain tx b62ac27081b4c1bc59fce99b60579448de1d550b575dedd7af95c2d25fb9853a
```

Shows the transaction with the height and index it was committed at. Exits `1` if
nothing matches.

### `export`

```console
$ prunella --chain demo.chain export --out full.xml
$ prunella --chain demo.chain export --out part.xml --from 2 --to 5
$ prunella --chain demo.chain export --out alpha.xml --namespace app.demo
```

`--out -` writes to standard output.

Without `--from`/`--to` this is a `full` export: a complete chain backup. A partial range
is a `range` export, importable but not a backup.

`--namespace` produces a **projection**: a partial view that omits transactions, cannot
reproduce its own transaction roots, cannot be imported and cannot create a chain. The
command prints a warning saying so. Export without `--namespace` when you want a backup.

### `import`

```console
$ prunella --chain copy.chain import --in full.xml --dry-run
$ prunella --chain copy.chain import --in full.xml
$ prunella --chain restored.chain import --in full.xml --create
```

| Option | Meaning |
|---|---|
| `--dry-run` | Run every check and write nothing |
| `--create` | Create the chain from the document, which must be a `full` export |

Import is atomic and idempotent. `--dry-run` and `--create` cannot be combined: there is
no chain to check the document against until it has been created.

## A complete session

```console
$ prunella keygen --out signer.key
$ prunella --chain demo.chain init --network demo
$ prunella --chain demo.chain append --signing-key signer.key \
      --namespace app.demo --nonce 1 --payload-hex 48656c6c6f
$ prunella --chain demo.chain verify
$ prunella --chain demo.chain export --out backup.xml
$ prunella --chain restored.chain import --in backup.xml --create
$ prunella --chain restored.chain verify
```

The two chains now have the same genesis hash, the same head hash and the same block
hash at every height.

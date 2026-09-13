//! End-to-end tests driving the built binary.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

struct Cli {
    directory: TempDir,
}

struct Run {
    output: Output,
}

impl Run {
    fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.output.stdout).into_owned()
    }

    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.output.stderr).into_owned()
    }

    fn code(&self) -> i32 {
        self.output.status.code().unwrap_or(-1)
    }

    fn ok(self) -> Self {
        assert_eq!(
            self.code(),
            0,
            "expected success\nstdout:\n{}\nstderr:\n{}",
            self.stdout(),
            self.stderr()
        );
        self
    }

    fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.stdout())
            .unwrap_or_else(|error| panic!("stdout is not json ({error}):\n{}", self.stdout()))
    }
}

impl Cli {
    fn new() -> Self {
        Self {
            directory: TempDir::new().expect("temp dir"),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }

    fn run(&self, args: &[&str]) -> Run {
        let output = Command::new(env!("CARGO_BIN_EXE_prunella"))
            .current_dir(self.directory.path())
            .args(args)
            .output()
            .expect("run prunella");
        Run { output }
    }

    /// A chain named `demo` with a key and three appended blocks.
    fn demo(&self) -> PathBuf {
        self.run(&["keygen", "--out", "signer.key"]).ok();
        self.run(&["--chain", "demo.chain", "init", "--network", "demo"])
            .ok();
        for (nonce, payload) in [(1u8, "48656c6c6f"), (2, "776f726c64"), (3, "21")] {
            self.run(&[
                "--chain",
                "demo.chain",
                "append",
                "--signing-key",
                "signer.key",
                "--namespace",
                if nonce == 3 { "app.other" } else { "app.demo" },
                "--nonce",
                &nonce.to_string(),
                "--payload-hex",
                payload,
                "--timestamp",
                &(u64::from(nonce) * 1000).to_string(),
            ])
            .ok();
        }
        self.path("demo.chain")
    }
}

#[test]
fn init_reports_the_genesis_and_is_reproducible() {
    let first = Cli::new();
    let second = Cli::new();
    let left = first
        .run(&["--chain", "a.chain", "init", "--network", "demo", "--json"])
        .ok();
    let right = second
        .run(&["--chain", "b.chain", "init", "--network", "demo", "--json"])
        .ok();

    // Two machines, no communication, same genesis: the core invariant at its root.
    assert_eq!(left.json()["genesis_hash"], right.json()["genesis_hash"]);
    assert_eq!(left.json()["network_id"], "demo");
}

#[test]
fn init_refuses_an_invalid_network_id_and_an_occupied_path() {
    let cli = Cli::new();
    let bad = cli.run(&["--chain", "a.chain", "init", "--network", "Demo"]);
    assert_eq!(bad.code(), 2);
    assert!(
        bad.stderr().contains("invalid network id"),
        "{}",
        bad.stderr()
    );

    cli.run(&["--chain", "a.chain", "init", "--network", "demo"])
        .ok();
    let again = cli.run(&["--chain", "a.chain", "init", "--network", "demo"]);
    assert_eq!(again.code(), 2);
    assert!(
        again.stderr().contains("already exists"),
        "{}",
        again.stderr()
    );
}

#[test]
fn status_reports_the_chain_summary() {
    let cli = Cli::new();
    cli.demo();
    let run = cli.run(&["--chain", "demo.chain", "status", "--json"]).ok();
    let status = run.json();

    assert_eq!(status["network_id"], "demo");
    assert_eq!(status["block_count"], 4);
    assert_eq!(status["transaction_count"], 3);
    assert_eq!(status["head"]["height"], 3);
    assert_eq!(status["acceptance_policy"], "local-deterministic");
    assert_eq!(status["store_format_version"], 1);
}

#[test]
fn verify_reports_a_valid_chain_and_exits_zero() {
    let cli = Cli::new();
    cli.demo();
    let run = cli.run(&["--chain", "demo.chain", "verify"]).ok();
    assert!(
        run.stdout().contains("result:      valid"),
        "{}",
        run.stdout()
    );

    let json = cli
        .run(&["--chain", "demo.chain", "verify", "--json"])
        .ok()
        .json();
    assert_eq!(json["blocks_checked"], 4);
    assert_eq!(json["transactions_checked"], 3);
    assert_eq!(json["findings"].as_array().expect("findings").len(), 0);
}

#[test]
fn verify_can_check_a_range() {
    let cli = Cli::new();
    cli.demo();
    let json = cli
        .run(&[
            "--chain",
            "demo.chain",
            "verify",
            "--from",
            "2",
            "--to",
            "3",
            "--json",
        ])
        .ok()
        .json();
    assert_eq!(json["blocks_checked"], 2);
    assert_eq!(json["range_start"], 2);
    assert_eq!(json["range_end"], 3);
}

#[test]
fn verify_exits_one_and_names_the_damage_on_a_corrupted_chain() {
    let cli = Cli::new();
    let chain = cli.demo();
    corrupt_block(&chain, 2);

    let run = cli.run(&["--chain", "demo.chain", "verify"]);
    assert_eq!(run.code(), 1, "{}", run.stdout());
    assert!(run.stdout().contains("INVALID"), "{}", run.stdout());
    assert!(run.stdout().contains("height 2"), "{}", run.stdout());
}

/// Overwrites a stored block with bytes that are not a block.
fn corrupt_block(chain: &Path, height: u64) {
    use redb::{Database, TableDefinition};
    const BLOCKS: TableDefinition<'static, u64, &[u8]> = TableDefinition::new("prunella_blocks");
    let database = Database::open(chain).expect("open raw");
    let write = database.begin_write().expect("write txn");
    {
        let mut blocks = write.open_table(BLOCKS).expect("blocks table");
        blocks
            .insert(height, b"not a block".as_slice())
            .expect("insert");
    }
    write.commit().expect("commit");
}

#[test]
fn block_can_be_found_by_height_and_by_hash() {
    let cli = Cli::new();
    cli.demo();
    let by_height = cli
        .run(&["--chain", "demo.chain", "block", "2", "--json"])
        .ok()
        .json();
    let hash = by_height["hash"].as_str().expect("hash").to_owned();

    let by_hash = cli
        .run(&["--chain", "demo.chain", "block", &hash, "--json"])
        .ok()
        .json();
    assert_eq!(by_height, by_hash);
    assert_eq!(by_height["header"]["height"], 2);
    assert_eq!(by_height["header"]["tx_count"], 1);
}

#[test]
fn block_exits_one_when_nothing_matches() {
    let cli = Cli::new();
    cli.demo();
    let missing = cli.run(&["--chain", "demo.chain", "block", "99"]);
    assert_eq!(missing.code(), 1);
    assert!(
        missing.stderr().contains("no block matches"),
        "{}",
        missing.stderr()
    );

    let nonsense = cli.run(&["--chain", "demo.chain", "block", "not-a-selector"]);
    assert_eq!(nonsense.code(), 2);
}

#[test]
fn tx_shows_a_transaction_and_where_it_lives() {
    let cli = Cli::new();
    cli.demo();
    let block = cli
        .run(&["--chain", "demo.chain", "block", "1", "--json"])
        .ok()
        .json();
    let id = block["transaction_ids"][0].as_str().expect("id").to_owned();

    let found = cli
        .run(&["--chain", "demo.chain", "tx", &id, "--json"])
        .ok()
        .json();
    assert_eq!(found["height"], 1);
    assert_eq!(found["index"], 0);
    assert_eq!(found["transaction"]["id"], id);
    assert_eq!(found["transaction"]["namespace"], "app.demo");
    assert_eq!(found["transaction"]["payload_hex"], "48656c6c6f");

    let absent = cli.run(&["--chain", "demo.chain", "tx", &"aa".repeat(32)]);
    assert_eq!(absent.code(), 1);
}

#[test]
fn append_accepts_every_payload_source() {
    let cli = Cli::new();
    cli.run(&["keygen", "--out", "k.key"]).ok();
    cli.run(&["--chain", "c.chain", "init", "--network", "demo"])
        .ok();
    std::fs::write(cli.path("payload.bin"), [0xff, 0x00, 0x80]).expect("write payload");

    for (nonce, source, value) in [
        (1, "--payload-hex", "0a0b"),
        (2, "--payload-base64", "CgsM"),
        (3, "--payload-file", "payload.bin"),
    ] {
        cli.run(&[
            "--chain",
            "c.chain",
            "append",
            "--signing-key",
            "k.key",
            "--namespace",
            "app.demo",
            "--nonce",
            &nonce.to_string(),
            source,
            value,
        ])
        .ok();
    }

    // An empty payload is a legitimate transaction: Prunella never reads payload bytes.
    cli.run(&[
        "--chain",
        "c.chain",
        "append",
        "--signing-key",
        "k.key",
        "--namespace",
        "app.demo",
        "--nonce",
        "4",
    ])
    .ok();

    cli.run(&["--chain", "c.chain", "verify"]).ok();
    let status = cli
        .run(&["--chain", "c.chain", "status", "--json"])
        .ok()
        .json();
    assert_eq!(status["transaction_count"], 4);
}

#[test]
fn append_refuses_a_replayed_transaction() {
    let cli = Cli::new();
    cli.demo();
    let repeat = cli.run(&[
        "--chain",
        "demo.chain",
        "append",
        "--signing-key",
        "signer.key",
        "--namespace",
        "app.demo",
        "--nonce",
        "1",
        "--payload-hex",
        "48656c6c6f",
    ]);
    assert_eq!(repeat.code(), 2);
    assert!(
        repeat.stderr().contains("already committed"),
        "{}",
        repeat.stderr()
    );
}

#[test]
fn append_refuses_a_timestamp_that_moves_the_clock_backwards() {
    // An explicit timestamp before the parent's is refused rather than adjusted:
    // quietly changing a value the operator asked for would mean the block committed is
    // not the block they described.
    let cli = Cli::new();
    cli.demo();
    let backwards = cli.run(&[
        "--chain",
        "demo.chain",
        "append",
        "--signing-key",
        "signer.key",
        "--namespace",
        "app.demo",
        "--nonce",
        "9",
        "--payload-hex",
        "00",
        "--timestamp",
        "500",
    ]);
    assert_eq!(backwards.code(), 2);
    assert!(
        backwards.stderr().contains("backwards"),
        "{}",
        backwards.stderr()
    );

    let status = cli
        .run(&["--chain", "demo.chain", "status", "--json"])
        .ok()
        .json();
    assert_eq!(
        status["head"]["height"], 3,
        "a refused append must write nothing"
    );

    // The parent's own timestamp is allowed: the rule is non-decreasing, not increasing.
    cli.run(&[
        "--chain",
        "demo.chain",
        "append",
        "--signing-key",
        "signer.key",
        "--namespace",
        "app.demo",
        "--nonce",
        "9",
        "--payload-hex",
        "00",
        "--timestamp",
        "3000",
    ])
    .ok();
    cli.run(&["--chain", "demo.chain", "verify"]).ok();
}

#[test]
fn append_needs_something_to_append() {
    let cli = Cli::new();
    cli.demo();
    let empty = cli.run(&["--chain", "demo.chain", "append"]);
    assert_ne!(empty.code(), 0);
}

#[test]
fn keygen_writes_a_usable_key() {
    let cli = Cli::new();
    let run = cli.run(&["keygen", "--out", "k.key", "--json"]).ok();
    let public = run.json()["public_key"]
        .as_str()
        .expect("public key")
        .to_owned();
    assert_eq!(public.len(), 64);

    let seed = std::fs::read_to_string(cli.path("k.key")).expect("read key");
    assert_eq!(seed.trim().len(), 64);

    cli.run(&["--chain", "c.chain", "init", "--network", "demo"])
        .ok();
    let appended = cli
        .run(&[
            "--chain",
            "c.chain",
            "append",
            "--signing-key",
            "k.key",
            "--namespace",
            "app.demo",
            "--nonce",
            "1",
            "--payload-hex",
            "00",
            "--json",
        ])
        .ok();
    assert_eq!(appended.json()["height"], 1);
}

#[test]
fn export_and_import_reproduce_the_chain_exactly() {
    let cli = Cli::new();
    cli.demo();
    let source = cli
        .run(&["--chain", "demo.chain", "status", "--json"])
        .ok()
        .json();

    cli.run(&["--chain", "demo.chain", "export", "--out", "full.xml"])
        .ok();
    cli.run(&["--chain", "copy.chain", "init", "--network", "demo"])
        .ok();

    let dry = cli
        .run(&[
            "--chain",
            "copy.chain",
            "import",
            "--in",
            "full.xml",
            "--dry-run",
            "--json",
        ])
        .ok()
        .json();
    assert_eq!(dry["blocks_to_append"], 3);
    assert_eq!(dry["blocks_already_present"], 1);
    // Nothing was written.
    let untouched = cli
        .run(&["--chain", "copy.chain", "status", "--json"])
        .ok()
        .json();
    assert_eq!(untouched["head"]["height"], 0);

    cli.run(&["--chain", "copy.chain", "import", "--in", "full.xml"])
        .ok();
    let copied = cli
        .run(&["--chain", "copy.chain", "status", "--json"])
        .ok()
        .json();
    assert_eq!(copied["head"], source["head"]);
    assert_eq!(copied["transaction_count"], source["transaction_count"]);
    cli.run(&["--chain", "copy.chain", "verify"]).ok();
}

#[test]
fn import_create_restores_a_chain_from_a_backup() {
    let cli = Cli::new();
    cli.demo();
    let source = cli
        .run(&["--chain", "demo.chain", "status", "--json"])
        .ok()
        .json();
    cli.run(&["--chain", "demo.chain", "export", "--out", "full.xml"])
        .ok();

    let restored = cli
        .run(&[
            "--chain",
            "restored.chain",
            "import",
            "--in",
            "full.xml",
            "--create",
            "--json",
        ])
        .ok()
        .json();
    assert_eq!(restored["created"], true);
    assert_eq!(restored["genesis_hash"], source["genesis_hash"]);

    let status = cli
        .run(&["--chain", "restored.chain", "status", "--json"])
        .ok()
        .json();
    assert_eq!(status["head"], source["head"]);
    cli.run(&["--chain", "restored.chain", "verify"]).ok();
}

#[test]
fn importing_the_same_document_twice_changes_nothing() {
    let cli = Cli::new();
    cli.demo();
    cli.run(&["--chain", "demo.chain", "export", "--out", "full.xml"])
        .ok();
    cli.run(&["--chain", "copy.chain", "init", "--network", "demo"])
        .ok();

    cli.run(&["--chain", "copy.chain", "import", "--in", "full.xml"])
        .ok();
    let again = cli
        .run(&[
            "--chain",
            "copy.chain",
            "import",
            "--in",
            "full.xml",
            "--json",
        ])
        .ok()
        .json();
    assert_eq!(again["appended"], 0);
    assert_eq!(again["skipped"], 4);
}

#[test]
fn ranges_can_be_exported_and_imported_incrementally() {
    let cli = Cli::new();
    cli.demo();
    cli.run(&[
        "--chain",
        "demo.chain",
        "export",
        "--out",
        "a.xml",
        "--from",
        "0",
        "--to",
        "1",
    ])
    .ok();
    cli.run(&[
        "--chain",
        "demo.chain",
        "export",
        "--out",
        "b.xml",
        "--from",
        "2",
        "--to",
        "3",
    ])
    .ok();
    cli.run(&["--chain", "copy.chain", "init", "--network", "demo"])
        .ok();

    cli.run(&["--chain", "copy.chain", "import", "--in", "a.xml"])
        .ok();
    cli.run(&["--chain", "copy.chain", "import", "--in", "b.xml"])
        .ok();
    let status = cli
        .run(&["--chain", "copy.chain", "status", "--json"])
        .ok()
        .json();
    assert_eq!(status["head"]["height"], 3);
}

#[test]
fn importing_a_range_out_of_order_is_refused() {
    let cli = Cli::new();
    cli.demo();
    cli.run(&[
        "--chain",
        "demo.chain",
        "export",
        "--out",
        "b.xml",
        "--from",
        "2",
        "--to",
        "3",
    ])
    .ok();
    cli.run(&["--chain", "copy.chain", "init", "--network", "demo"])
        .ok();

    let gap = cli.run(&["--chain", "copy.chain", "import", "--in", "b.xml"]);
    assert_eq!(gap.code(), 2);
    assert!(
        gap.stderr().contains("must start at height 1"),
        "{}",
        gap.stderr()
    );
}

#[test]
fn a_namespace_export_is_marked_a_projection_warned_about_and_refused_on_import() {
    let cli = Cli::new();
    cli.demo();
    let run = cli
        .run(&[
            "--chain",
            "demo.chain",
            "export",
            "--out",
            "p.xml",
            "--namespace",
            "app.demo",
            "--json",
        ])
        .ok();
    assert_eq!(run.json()["kind"], "projection");
    assert_eq!(run.json()["is_backup"], false);
    assert!(
        run.stderr().contains("not a chain backup"),
        "{}",
        run.stderr()
    );

    let xml = std::fs::read_to_string(cli.path("p.xml")).expect("read");
    assert!(xml.contains("kind=\"projection\""), "{xml}");

    cli.run(&["--chain", "copy.chain", "init", "--network", "demo"])
        .ok();
    let refused = cli.run(&["--chain", "copy.chain", "import", "--in", "p.xml"]);
    assert_eq!(refused.code(), 2);
    assert!(
        refused.stderr().contains("cannot be imported"),
        "{}",
        refused.stderr()
    );
}

#[test]
fn a_tampered_document_is_refused_and_leaves_the_chain_alone() {
    let cli = Cli::new();
    cli.demo();
    cli.run(&["--chain", "demo.chain", "export", "--out", "full.xml"])
        .ok();
    let xml = std::fs::read_to_string(cli.path("full.xml")).expect("read");
    std::fs::write(
        cli.path("bad.xml"),
        xml.replace("<payload>SGVsbG8=</payload>", "<payload>dGFtcGVy</payload>"),
    )
    .expect("write");

    cli.run(&["--chain", "copy.chain", "init", "--network", "demo"])
        .ok();
    let refused = cli.run(&["--chain", "copy.chain", "import", "--in", "bad.xml"]);
    assert_eq!(refused.code(), 2);

    let status = cli
        .run(&["--chain", "copy.chain", "status", "--json"])
        .ok()
        .json();
    assert_eq!(
        status["head"]["height"], 0,
        "a refused import must write nothing"
    );
}

#[test]
fn a_document_from_another_chain_is_refused() {
    let cli = Cli::new();
    cli.demo();
    cli.run(&["--chain", "demo.chain", "export", "--out", "full.xml"])
        .ok();
    cli.run(&["--chain", "other.chain", "init", "--network", "othernet"])
        .ok();

    let refused = cli.run(&["--chain", "other.chain", "import", "--in", "full.xml"]);
    assert_eq!(refused.code(), 2);
    assert!(
        refused.stderr().contains("othernet"),
        "{}",
        refused.stderr()
    );
}

#[test]
fn dry_run_and_create_cannot_be_combined() {
    let cli = Cli::new();
    cli.demo();
    cli.run(&["--chain", "demo.chain", "export", "--out", "full.xml"])
        .ok();

    let refused = cli.run(&[
        "--chain",
        "new.chain",
        "import",
        "--in",
        "full.xml",
        "--create",
        "--dry-run",
    ]);
    assert_eq!(refused.code(), 2);
    assert!(
        refused.stderr().contains("cannot be combined"),
        "{}",
        refused.stderr()
    );
}

#[test]
fn commands_that_need_a_chain_say_so_when_there_is_none() {
    let cli = Cli::new();
    for args in [
        vec!["status"],
        vec!["verify"],
        vec!["block", "0"],
        vec!["tx", &"00".repeat(32)],
    ] {
        let mut full = vec!["--chain", "absent.chain"];
        full.extend(args.iter().copied());
        let run = cli.run(&full);
        assert_eq!(run.code(), 2, "{:?} should have failed", full);
        assert!(run.stderr().contains("no such file"), "{}", run.stderr());
    }
}

#[test]
fn the_chain_path_can_come_from_the_environment() {
    let cli = Cli::new();
    let output = Command::new(env!("CARGO_BIN_EXE_prunella"))
        .current_dir(cli.directory.path())
        .env("PRUNELLA_CHAIN", "from-env.chain")
        .args(["init", "--network", "demo", "--json"])
        .output()
        .expect("run prunella");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(cli.path("from-env.chain").exists());
}

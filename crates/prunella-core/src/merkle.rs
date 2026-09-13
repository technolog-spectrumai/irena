//! The transaction Merkle tree and inclusion proofs.
//!
//! A block's `tx_root` is the root of a binary Merkle tree over its transaction ids, in
//! order. The shape follows RFC 6962 (Certificate Transparency): a tree of `n` leaves
//! splits at the largest power of two strictly less than `n`, so the left subtree is
//! always complete and the right subtree holds the remainder. Nothing is padded or
//! duplicated to reach a power of two, which is what rules out the second-preimage
//! trick where a duplicated last leaf yields the same root as the original list.
//!
//! Leaves and interior nodes are hashed in different domains, so a leaf can never be
//! reinterpreted as a node or vice versa, and the empty tree has a root in a third
//! domain that no leaf or node can equal.
//!
//! ```text
//! root([])            = hash_domain(TX_ROOT, "")
//! root([id])          = hash_domain(TX_LEAF, id)
//! root(ids), n > 1    = hash_domain(TX_NODE, root(ids[..k]) || root(ids[k..]))
//!                       where k is the largest power of two with k < n
//! ```
//!
//! An [`InclusionProof`] is the audit path from one leaf to the root: the sibling hash
//! at each level and which side it sits on. Verifying a proof against a block header
//! needs only the header's `tx_root` and `tx_count`, not the block's transactions.
//! Proofs are over transaction ids alone and know nothing about namespaces, payloads
//! or what a transaction means.
//!
//! The construction itself is generic: [`TreeTags`] names the three hash domains, and
//! [`root`], [`InclusionProof::generate_with`] and [`InclusionProof::verify_with`] take
//! any tags over any 32-byte leaves. [`TreeTags::PRUNELLA_V1`] is the block tree; an
//! application committing to its own list (ballots, records, anything) supplies its own
//! tags and gets the same shape, the same proofs and no way to collide with a block
//! tree. The convenience functions without a `tags` argument are the Prunella V1 tree.

use crate::hash::{Hash, TxId};
use borsh::{BorshDeserialize, BorshSerialize};
use prunella_canonical::{Canonical, DomainHasher, domain, hash_domain};

/// The three hash domains a Merkle tree is built in.
///
/// Separate tags keep leaves, interior nodes and the empty tree in distinct domains, and
/// keep one application's tree from ever equalling another's over the same leaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TreeTags {
    /// Domain for a leaf: `hash_domain(leaf, content)`.
    pub leaf: &'static str,
    /// Domain for an interior node: `hash_domain(node, left || right)`.
    pub node: &'static str,
    /// Domain for the root of the empty tree: `hash_domain(empty, "")`.
    pub empty: &'static str,
}

impl TreeTags {
    /// The Prunella V1 block tree over transaction ids, as frozen in `PROTOCOL_V1.md`.
    pub const PRUNELLA_V1: Self = Self {
        leaf: domain::TX_LEAF,
        node: domain::TX_NODE,
        empty: domain::TX_ROOT,
    };
}

/// Hashes one 32-byte leaf content in `tags.leaf`.
#[must_use]
pub fn leaf_hash_with(tags: TreeTags, content: &Hash) -> Hash {
    Hash::from_bytes(hash_domain(tags.leaf, content.as_bytes()))
}

/// Hashes two child roots as an interior node in `tags.node`.
#[must_use]
pub fn node_hash_with(tags: TreeTags, left: &Hash, right: &Hash) -> Hash {
    let mut hasher = DomainHasher::new(tags.node);
    hasher.update(left.as_bytes()).update(right.as_bytes());
    Hash::from_bytes(hasher.finalize())
}

/// The root of the empty tree in `tags.empty`.
#[must_use]
pub fn empty_root_with(tags: TreeTags) -> Hash {
    Hash::from_bytes(hash_domain(tags.empty, b""))
}

/// Computes the Merkle root over an ordered list of leaf contents under `tags`.
#[must_use]
pub fn root(tags: TreeTags, leaves: &[Hash]) -> Hash {
    match leaves {
        [] => empty_root_with(tags),
        [only] => leaf_hash_with(tags, only),
        _ => {
            let split = split_point(leaves.len());
            node_hash_with(
                tags,
                &root(tags, &leaves[..split]),
                &root(tags, &leaves[split..]),
            )
        }
    }
}

/// Hashes one transaction id as a leaf of the Prunella V1 tree.
#[must_use]
pub fn leaf_hash(id: &TxId) -> Hash {
    leaf_hash_with(TreeTags::PRUNELLA_V1, &id.hash())
}

/// Hashes two child roots as an interior node of the Prunella V1 tree.
#[must_use]
pub fn node_hash(left: &Hash, right: &Hash) -> Hash {
    node_hash_with(TreeTags::PRUNELLA_V1, left, right)
}

/// The root of the empty Prunella V1 tree.
#[must_use]
pub fn empty_root() -> Hash {
    empty_root_with(TreeTags::PRUNELLA_V1)
}

/// Computes the Prunella V1 Merkle root over an ordered list of transaction ids.
#[must_use]
pub fn merkle_root(ids: &[TxId]) -> Hash {
    root(TreeTags::PRUNELLA_V1, &id_hashes(ids))
}

/// The leaf contents of a Prunella V1 tree: the ids' digests, in order.
fn id_hashes(ids: &[TxId]) -> Vec<Hash> {
    ids.iter().map(|id| id.hash()).collect()
}

/// The largest power of two strictly less than `n`, for `n >= 2`.
fn split_point(n: usize) -> usize {
    debug_assert!(n >= 2);
    let mut k = 1usize;
    while k * 2 < n {
        k *= 2;
    }
    k
}

/// Which side of the path a sibling sits on.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// The sibling is the left child; the running hash is the right child.
    Left,
    /// The sibling is the right child; the running hash is the left child.
    Right,
}

/// One level of an inclusion proof.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ProofStep {
    /// Where the sibling sits relative to the running hash.
    pub side: Side,
    /// The sibling subtree's root.
    pub hash: Hash,
}

/// Proof that a transaction id sits at a given position in a block's tree.
///
/// The tree size is not part of the proof: it comes from the block header's
/// `tx_count` at verification time, so a proof cannot claim a tree shape the header
/// does not commit to.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct InclusionProof {
    /// Position of the transaction within the block.
    pub index: u32,
    /// Sibling hashes from the leaf up to the root.
    pub steps: Vec<ProofStep>,
}

impl Canonical for InclusionProof {}

/// Why a proof was rejected.
#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone)]
#[non_exhaustive]
pub enum ProofError {
    /// The index is not below the header's transaction count.
    #[error("proof index {index} is not below the block's transaction count {count}")]
    IndexOutOfRange {
        /// The index claimed.
        index: u32,
        /// The block's transaction count.
        count: u32,
    },
    /// The proof's path does not have the shape a tree of this size and index has.
    #[error(
        "proof path has the wrong shape for index {index} in a tree of {count}: expected {expected} step(s) with sides {expected_sides:?}, found {found} with {found_sides:?}"
    )]
    WrongShape {
        /// The index claimed.
        index: u32,
        /// The block's transaction count.
        count: u32,
        /// Steps the shape requires.
        expected: usize,
        /// Sides the shape requires, leaf first.
        expected_sides: Vec<Side>,
        /// Steps supplied.
        found: usize,
        /// Sides supplied, leaf first.
        found_sides: Vec<Side>,
    },
    /// The path folds to a different root than the header commits to.
    #[error("proof folds to {computed} but the block commits to {expected}")]
    RootMismatch {
        /// The root the header carries.
        expected: Hash,
        /// The root the proof produced.
        computed: Hash,
    },
}

impl InclusionProof {
    /// Builds the proof for the transaction at `index` among `ids` in the Prunella V1
    /// tree.
    ///
    /// Returns `None` if `index` is out of range.
    #[must_use]
    pub fn generate(ids: &[TxId], index: usize) -> Option<Self> {
        Self::generate_with(TreeTags::PRUNELLA_V1, &id_hashes(ids), index)
    }

    /// Builds the proof for the leaf at `index` among `leaves` in a tree under `tags`.
    ///
    /// Returns `None` if `index` is out of range or exceeds [`u32::MAX`].
    #[must_use]
    pub fn generate_with(tags: TreeTags, leaves: &[Hash], index: usize) -> Option<Self> {
        if index >= leaves.len() {
            return None;
        }
        let mut steps = Vec::new();
        audit_path(tags, leaves, index, &mut steps);
        Some(Self {
            index: u32::try_from(index).ok()?,
            steps,
        })
    }

    /// Verifies that `id` sits at this proof's index in a tree of `count` leaves whose
    /// root is `root`.
    ///
    /// # Errors
    ///
    /// Returns [`ProofError`] naming exactly what did not hold: an out-of-range index, a
    /// path whose shape does not match the claimed position, or a path that folds to a
    /// different root. Any alteration of the id, the index or a step is caught by one
    /// of these.
    pub fn verify(&self, id: &TxId, count: u32, root: &Hash) -> Result<(), ProofError> {
        self.verify_with(TreeTags::PRUNELLA_V1, &id.hash(), count, root)
    }

    /// Verifies that `leaf` sits at this proof's index in a tree of `count` leaves under
    /// `tags` whose root is `root`.
    ///
    /// # Errors
    ///
    /// As [`InclusionProof::verify`].
    pub fn verify_with(
        &self,
        tags: TreeTags,
        leaf: &Hash,
        count: u32,
        root: &Hash,
    ) -> Result<(), ProofError> {
        if self.index >= count {
            return Err(ProofError::IndexOutOfRange {
                index: self.index,
                count,
            });
        }
        let expected_sides = expected_sides(count as usize, self.index as usize);
        let found_sides: Vec<Side> = self.steps.iter().map(|step| step.side).collect();
        if expected_sides != found_sides {
            return Err(ProofError::WrongShape {
                index: self.index,
                count,
                expected: expected_sides.len(),
                expected_sides,
                found: found_sides.len(),
                found_sides,
            });
        }

        let mut running = leaf_hash_with(tags, leaf);
        for step in &self.steps {
            running = match step.side {
                Side::Left => node_hash_with(tags, &step.hash, &running),
                Side::Right => node_hash_with(tags, &running, &step.hash),
            };
        }
        if running == *root {
            Ok(())
        } else {
            Err(ProofError::RootMismatch {
                expected: *root,
                computed: running,
            })
        }
    }

    /// Verifies this proof against a block header.
    ///
    /// # Errors
    ///
    /// As [`InclusionProof::verify`].
    pub fn verify_against(
        &self,
        id: &TxId,
        header: &crate::block::BlockHeader,
    ) -> Result<(), ProofError> {
        self.verify(id, header.tx_count, &header.tx_root)
    }
}

/// Appends the audit path for leaf `index`, leaf-most step first.
fn audit_path(tags: TreeTags, leaves: &[Hash], index: usize, steps: &mut Vec<ProofStep>) {
    if leaves.len() <= 1 {
        return;
    }
    let split = split_point(leaves.len());
    if index < split {
        audit_path(tags, &leaves[..split], index, steps);
        steps.push(ProofStep {
            side: Side::Right,
            hash: root(tags, &leaves[split..]),
        });
    } else {
        audit_path(tags, &leaves[split..], index - split, steps);
        steps.push(ProofStep {
            side: Side::Left,
            hash: root(tags, &leaves[..split]),
        });
    }
}

/// The side sequence a valid path for `index` in a tree of `count` must have.
///
/// Derived from the shape alone, without any hashes, so a proof that claims one index
/// but carries the path of another is rejected before any hashing happens.
fn expected_sides(count: usize, index: usize) -> Vec<Side> {
    let mut sides = Vec::new();
    fn walk(count: usize, index: usize, sides: &mut Vec<Side>) {
        if count <= 1 {
            return;
        }
        let split = split_point(count);
        if index < split {
            walk(split, index, sides);
            sides.push(Side::Right);
        } else {
            walk(count - split, index - split, sides);
            sides.push(Side::Left);
        }
    }
    walk(count, index, &mut sides);
    sides
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(byte: u8) -> TxId {
        TxId::from_hash(Hash::from_bytes([byte; 32]))
    }

    #[test]
    fn split_point_is_the_largest_power_of_two_below_n() {
        assert_eq!(split_point(2), 1);
        assert_eq!(split_point(3), 2);
        assert_eq!(split_point(4), 2);
        assert_eq!(split_point(5), 4);
        assert_eq!(split_point(8), 4);
        assert_eq!(split_point(9), 8);
    }

    #[test]
    fn small_trees_match_the_documented_construction() {
        let ids: Vec<TxId> = (1..=3).map(id).collect();
        assert_eq!(merkle_root(&[]), empty_root());
        assert_eq!(merkle_root(&ids[..1]), leaf_hash(&ids[0]));
        assert_eq!(
            merkle_root(&ids[..2]),
            node_hash(&leaf_hash(&ids[0]), &leaf_hash(&ids[1]))
        );
        assert_eq!(
            merkle_root(&ids),
            node_hash(
                &node_hash(&leaf_hash(&ids[0]), &leaf_hash(&ids[1])),
                &leaf_hash(&ids[2])
            )
        );
    }

    #[test]
    fn duplicating_the_last_leaf_changes_the_root() {
        // The Bitcoin-style construction would produce the same root for [a, b, c] and
        // [a, b, c, c]. This one must not.
        let three: Vec<TxId> = (1..=3).map(id).collect();
        let mut four = three.clone();
        four.push(id(3));
        assert_ne!(merkle_root(&three), merkle_root(&four));
    }

    #[test]
    fn leaves_and_nodes_cannot_be_confused() {
        // A "leaf" whose id bytes equal a node's children would collide under a single
        // hash domain. With separate domains it cannot.
        let a = leaf_hash(&id(1));
        let b = leaf_hash(&id(2));
        let node = node_hash(&a, &b);
        assert_ne!(node, leaf_hash(&TxId::from_hash(a)));
        assert_ne!(node, leaf_hash(&TxId::from_hash(b)));
        assert_ne!(empty_root(), leaf_hash(&id(0)));
    }

    #[test]
    fn every_position_in_every_small_tree_proves_and_verifies() {
        for count in 1..=17usize {
            let ids: Vec<TxId> = (0..count).map(|i| id(i as u8 + 1)).collect();
            let root = merkle_root(&ids);
            for index in 0..count {
                let proof = InclusionProof::generate(&ids, index).expect("in range");
                proof
                    .verify(&ids[index], count as u32, &root)
                    .unwrap_or_else(|error| {
                        panic!("index {index} of {count}: {error}");
                    });
            }
        }
    }

    #[test]
    fn an_out_of_range_index_yields_no_proof() {
        let ids: Vec<TxId> = (1..=3).map(id).collect();
        assert!(InclusionProof::generate(&ids, 3).is_none());
        assert!(InclusionProof::generate(&[], 0).is_none());
    }

    #[test]
    fn an_altered_transaction_is_rejected() {
        let ids: Vec<TxId> = (1..=5).map(id).collect();
        let root = merkle_root(&ids);
        let proof = InclusionProof::generate(&ids, 2).expect("proof");
        assert!(matches!(
            proof.verify(&id(9), 5, &root),
            Err(ProofError::RootMismatch { .. })
        ));
    }

    #[test]
    fn an_altered_index_is_rejected() {
        let ids: Vec<TxId> = (1..=5).map(id).collect();
        let root = merkle_root(&ids);
        let mut proof = InclusionProof::generate(&ids, 2).expect("proof");
        proof.index = 3;
        let error = proof.verify(&ids[2], 5, &root).expect_err("wrong index");
        assert!(
            matches!(
                error,
                ProofError::WrongShape { .. } | ProofError::RootMismatch { .. }
            ),
            "{error}"
        );
        proof.index = 5;
        assert!(matches!(
            proof.verify(&ids[2], 5, &root),
            Err(ProofError::IndexOutOfRange { .. })
        ));
    }

    #[test]
    fn an_altered_step_is_rejected() {
        let ids: Vec<TxId> = (1..=5).map(id).collect();
        let root = merkle_root(&ids);
        let good = InclusionProof::generate(&ids, 1).expect("proof");

        let mut flipped_hash = good.clone();
        flipped_hash.steps[0].hash = Hash::from_bytes([0xee; 32]);
        assert!(matches!(
            flipped_hash.verify(&ids[1], 5, &root),
            Err(ProofError::RootMismatch { .. })
        ));

        let mut flipped_side = good.clone();
        flipped_side.steps[0].side = Side::Right;
        assert!(matches!(
            flipped_side.verify(&ids[1], 5, &root),
            Err(ProofError::WrongShape { .. })
        ));

        let mut truncated = good.clone();
        truncated.steps.pop();
        assert!(matches!(
            truncated.verify(&ids[1], 5, &root),
            Err(ProofError::WrongShape { .. })
        ));

        let mut extended = good;
        extended.steps.push(ProofStep {
            side: Side::Right,
            hash: root,
        });
        assert!(matches!(
            extended.verify(&ids[1], 5, &root),
            Err(ProofError::WrongShape { .. })
        ));
    }

    #[test]
    fn a_proof_for_one_tree_does_not_verify_against_another_block() {
        let ids: Vec<TxId> = (1..=6).map(id).collect();
        let proof = InclusionProof::generate(&ids, 4).expect("proof");

        // A header claiming a different tree size can have a different shape for this
        // index, which the shape check catches on its own.
        assert!(matches!(
            proof.verify(&ids[4], 8, &merkle_root(&ids)),
            Err(ProofError::WrongShape { .. })
        ));

        // The binding that always holds is the root: a different block's root is a
        // different root, whatever its size. The shape check is a cheap early reject,
        // not the guarantee.
        let other: Vec<TxId> = (10..=15).map(id).collect();
        assert!(matches!(
            proof.verify(&ids[4], 6, &merkle_root(&other)),
            Err(ProofError::RootMismatch { .. })
        ));
    }

    #[test]
    fn proofs_round_trip_through_canonical_bytes() {
        let ids: Vec<TxId> = (1..=7).map(id).collect();
        let proof = InclusionProof::generate(&ids, 6).expect("proof");
        let decoded =
            InclusionProof::from_canonical_bytes(&proof.canonical_bytes()).expect("decode");
        assert_eq!(decoded, proof);
    }
    const OTHER: TreeTags = TreeTags {
        leaf: "TEST/other-leaf",
        node: "TEST/other-node",
        empty: "TEST/other-empty",
    };

    #[test]
    fn the_generic_tree_under_prunella_tags_is_the_block_tree() {
        for count in 0..=9usize {
            let ids: Vec<TxId> = (0..count).map(|i| id(i as u8 + 1)).collect();
            let leaves: Vec<Hash> = ids.iter().map(|id| id.hash()).collect();
            assert_eq!(root(TreeTags::PRUNELLA_V1, &leaves), merkle_root(&ids));
            for index in 0..count {
                let generic = InclusionProof::generate_with(TreeTags::PRUNELLA_V1, &leaves, index)
                    .expect("in range");
                assert_eq!(
                    generic,
                    InclusionProof::generate(&ids, index).expect("in range")
                );
            }
        }
    }

    #[test]
    fn different_tags_give_different_trees_over_the_same_leaves() {
        let leaves: Vec<Hash> = (1..=4).map(|i| id(i).hash()).collect();
        assert_ne!(root(OTHER, &leaves), root(TreeTags::PRUNELLA_V1, &leaves));
        assert_ne!(root(OTHER, &[]), root(TreeTags::PRUNELLA_V1, &[]));
        assert_ne!(
            root(OTHER, &leaves[..1]),
            root(TreeTags::PRUNELLA_V1, &leaves[..1])
        );

        // A proof built in one domain never verifies in another.
        let proof = InclusionProof::generate_with(OTHER, &leaves, 2).expect("in range");
        assert!(
            proof
                .verify_with(OTHER, &leaves[2], 4, &root(OTHER, &leaves))
                .is_ok()
        );
        assert!(matches!(
            proof.verify_with(TreeTags::PRUNELLA_V1, &leaves[2], 4, &root(OTHER, &leaves)),
            Err(ProofError::RootMismatch { .. })
        ));
        assert!(matches!(
            proof.verify_with(OTHER, &leaves[2], 4, &root(TreeTags::PRUNELLA_V1, &leaves)),
            Err(ProofError::RootMismatch { .. })
        ));
    }
}

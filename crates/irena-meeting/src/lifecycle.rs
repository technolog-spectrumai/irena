//! The meeting as a process.

use crate::error::MeetingError;
use crate::meeting::{
    AgendaBodyV1, AgendaItemV1, AgendaV1, MeetingIdV1, MeetingMetadataV1, MeetingStatusV1,
};
use crate::record::{
    FinalItemV1, MEETING_NAMESPACE, MEETING_SCHEMA_VERSION, MeetingFinalRecordV1, compose_convened,
    compose_final,
};
use borsh::{BorshDeserialize, BorshSerialize};
use irena_core::{CompanyIdV1, NotarisationV1};
use irena_ledger::company_now;
use irena_vote::{SignedBallotV1, VoteV1};
use prunella_canonical::Canonical;
use prunella_core::{BlockHeight, Namespace, SchemaVersion, TransactionDraft, TxId};
use prunella_crypto::SigningKey;
use prunella_store::LocalChainStore;

/// Where a finalised meeting landed.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct MeetingFinalizedV1 {
    /// The transaction carrying the final record.
    pub tx_id: TxId,
    /// The block it was committed in.
    pub height: BlockHeight,
    /// The record as written.
    pub record: MeetingFinalRecordV1,
}

/// A shareholder meeting from draft to final record.
///
/// A runtime state machine, like a vote: every operation checks the status first and
/// refuses with [`MeetingError::InvalidTransition`] naming both ends. The whole state
/// is canonical Borsh — including each vote item's [`VoteV1`] — so a meeting can be
/// written to a file between steps and read back to exactly the same meeting.
///
/// Only two things reach the chain, in `convene` and `finalize`. Adding an item,
/// opening, casting a ballot and closing are local: a meeting's UI state is not the
/// company's business.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ShareholderMeetingV1 {
    status: MeetingStatusV1,
    company: String,
    metadata: MeetingMetadataV1,
    items: Vec<AgendaItemV1>,
    /// One vote per vote item, in agenda order, once opened.
    votes: Vec<(u32, VoteV1)>,
    convened: Option<(TxId, BlockHeight)>,
    opened_at: Option<BlockHeight>,
    finalized: Option<(TxId, BlockHeight)>,
}

impl Canonical for ShareholderMeetingV1 {}

impl ShareholderMeetingV1 {
    /// Starts a meeting: what it is called, when it is to be held, and the notice.
    ///
    /// The company is the chain's — one company per chain — and is filled in when the
    /// meeting is convened.
    #[must_use]
    pub fn draft(metadata: MeetingMetadataV1) -> Self {
        Self {
            status: MeetingStatusV1::Draft,
            company: String::new(),
            metadata,
            items: Vec::new(),
            votes: Vec::new(),
            convened: None,
            opened_at: None,
            finalized: None,
        }
    }

    /// Where the meeting is.
    #[must_use]
    pub const fn status(&self) -> MeetingStatusV1 {
        self.status
    }

    /// The company label, once convened; empty before.
    #[must_use]
    pub fn company(&self) -> &str {
        &self.company
    }

    /// Title, time and notice.
    #[must_use]
    pub const fn metadata(&self) -> &MeetingMetadataV1 {
        &self.metadata
    }

    /// The agenda so far.
    #[must_use]
    pub fn items(&self) -> &[AgendaItemV1] {
        &self.items
    }

    /// The agenda, if it is a valid one.
    ///
    /// # Errors
    ///
    /// [`MeetingError::InvalidAgenda`] for an empty or misnumbered agenda.
    pub fn agenda(&self) -> Result<AgendaV1, MeetingError> {
        AgendaV1::new(self.items.clone())
    }

    /// The meeting id, once convened.
    #[must_use]
    pub fn id(&self) -> Option<MeetingIdV1> {
        self.convened.map(|(tx_id, _)| MeetingIdV1::from_tx(tx_id))
    }

    /// Where the convening record landed.
    #[must_use]
    pub const fn convened(&self) -> Option<(TxId, BlockHeight)> {
        self.convened
    }

    /// The height every vote item was frozen at, once opened.
    #[must_use]
    pub const fn opened_at(&self) -> Option<BlockHeight> {
        self.opened_at
    }

    /// Where the final record landed.
    #[must_use]
    pub const fn finalized(&self) -> Option<(TxId, BlockHeight)> {
        self.finalized
    }

    /// The vote of one item, once opened.
    #[must_use]
    pub fn vote(&self, number: u32) -> Option<&VoteV1> {
        self.votes
            .iter()
            .find(|(item, _)| *item == number)
            .map(|(_, vote)| vote)
    }

    /// Every vote, in agenda order, with its item number.
    pub fn votes(&self) -> impl Iterator<Item = (u32, &VoteV1)> {
        self.votes.iter().map(|(number, vote)| (*number, vote))
    }

    fn expect_status(
        &self,
        required: MeetingStatusV1,
        to: &'static str,
    ) -> Result<(), MeetingError> {
        if self.status == required {
            Ok(())
        } else {
            Err(MeetingError::InvalidTransition {
                from: self.status,
                to,
            })
        }
    }

    /// Adds an agenda item. Only while a draft; the agenda is fixed by convening.
    ///
    /// The item is numbered for you, from 1 in the order added.
    ///
    /// # Errors
    ///
    /// [`MeetingError::InvalidTransition`] unless a draft;
    /// [`MeetingError::InvalidAgenda`] for an empty title.
    pub fn add_item(
        &mut self,
        title: impl Into<String>,
        body: AgendaBodyV1,
    ) -> Result<&AgendaItemV1, MeetingError> {
        self.expect_status(MeetingStatusV1::Draft, "add an item to")?;
        let title = title.into();
        if title.trim().is_empty() {
            return Err(MeetingError::InvalidAgenda {
                detail: "an agenda item needs a title".to_owned(),
            });
        }
        let number =
            u32::try_from(self.items.len() + 1).map_err(|_| MeetingError::InvalidAgenda {
                detail: "too many items".to_owned(),
            })?;
        self.items.push(AgendaItemV1 {
            number,
            title,
            body,
        });
        Ok(self.items.last().expect("just pushed"))
    }

    /// Convenes the meeting: writes the convening record, with the whole agenda, to
    /// the chain in its own block.
    ///
    /// The transaction id becomes the meeting id. From here the agenda is fixed: what
    /// was put before the shareholders is what they were called to decide.
    ///
    /// # Errors
    ///
    /// [`MeetingError::InvalidTransition`] unless a draft;
    /// [`MeetingError::InvalidAgenda`] if the agenda or metadata is not valid; the
    /// ledger's errors if the chain holds no company; the chain's own errors.
    pub fn convene(
        &mut self,
        store: &LocalChainStore,
        key: &SigningKey,
        notarisation: &NotarisationV1,
        timestamp_millis: u64,
    ) -> Result<MeetingIdV1, MeetingError> {
        self.expect_status(MeetingStatusV1::Draft, "convene")?;
        let agenda = self.agenda()?;
        let state = company_now(store)?;
        let payload = compose_convened(&state.company, notarisation, &self.metadata, &agenda)?;
        let (tx_id, height) = self.append(store, key, payload, timestamp_millis)?;
        self.company = state.company.as_str().to_owned();
        self.convened = Some((tx_id, height));
        self.status = MeetingStatusV1::Convened;
        Ok(MeetingIdV1::from_tx(tx_id))
    }

    /// Opens the meeting: creates and freezes a vote for every vote item.
    ///
    /// Each vote freezes the company **on its own** at the current head — its own
    /// snapshot, its own electorate, its own id — so a later amendment reaches none of
    /// them, and two items with the same proposal are still two different votes
    /// because their subjects differ. A meeting with no vote items opens too: its
    /// items are informational and there is nothing to decide.
    ///
    /// # Errors
    ///
    /// [`MeetingError::InvalidTransition`] unless convened; [`MeetingError::Vote`]
    /// naming the item whose vote could not be frozen.
    pub fn open(&mut self, store: &LocalChainStore) -> Result<BlockHeight, MeetingError> {
        self.expect_status(MeetingStatusV1::Convened, "open")?;
        let at = store.head()?.height;
        let mut votes = Vec::new();
        for item in self.items.iter().filter(|item| item.body.is_vote()) {
            let AgendaBodyV1::Vote { proposal_digest } = item.body else {
                unreachable!("filtered")
            };
            let mut vote = VoteV1::draft(self.subject_of(item), proposal_digest);
            vote.freeze(store, at)
                .map_err(|source| MeetingError::Vote {
                    number: item.number,
                    source,
                })?;
            vote.open().map_err(|source| MeetingError::Vote {
                number: item.number,
                source,
            })?;
            votes.push((item.number, vote));
        }
        self.votes = votes;
        self.opened_at = Some(at);
        self.status = MeetingStatusV1::Open;
        Ok(at)
    }

    /// The subject of an item's vote: its number and title, so two items with the same
    /// wording are still two votes, and a vote can be tied back to its item.
    fn subject_of(&self, item: &AgendaItemV1) -> String {
        format!("item {}: {}", item.number, item.title)
    }

    /// Accepts a ballot for one item's vote.
    ///
    /// Only while the meeting is open. The vote applies its own checks (right vote,
    /// frozen voter, registered key, signature, not already voted).
    ///
    /// # Errors
    ///
    /// [`MeetingError::InvalidTransition`] unless open; [`MeetingError::NoSuchItem`]
    /// or [`MeetingError::NotAVoteItem`]; [`MeetingError::Vote`] carrying the
    /// rejection.
    pub fn cast(&mut self, number: u32, ballot: SignedBallotV1) -> Result<(), MeetingError> {
        self.expect_status(MeetingStatusV1::Open, "cast a ballot in")?;
        self.item_of(number)?;
        let vote = self
            .votes
            .iter_mut()
            .find(|(item, _)| *item == number)
            .map(|(_, vote)| vote)
            .ok_or(MeetingError::NotAVoteItem { number })?;
        vote.cast(ballot)
            .map_err(|source| MeetingError::Vote { number, source })
    }

    fn item_of(&self, number: u32) -> Result<&AgendaItemV1, MeetingError> {
        self.items
            .iter()
            .find(|item| item.number == number)
            .ok_or(MeetingError::NoSuchItem { number })
    }

    /// Closes every vote and counts it.
    ///
    /// # Errors
    ///
    /// [`MeetingError::InvalidTransition`] unless open; [`MeetingError::Vote`] naming
    /// the item whose vote could not be counted.
    pub fn close(&mut self, store: &LocalChainStore) -> Result<(), MeetingError> {
        self.expect_status(MeetingStatusV1::Open, "close")?;
        for (number, vote) in &mut self.votes {
            let number = *number;
            vote.close()
                .map_err(|source| MeetingError::Vote { number, source })?;
            vote.evaluate(store)
                .map_err(|source| MeetingError::Vote { number, source })?;
        }
        self.status = MeetingStatusV1::Closed;
        Ok(())
    }

    /// The final record this meeting would leave, if every vote were already on the
    /// chain.
    ///
    /// # Errors
    ///
    /// [`MeetingError::InvalidTransition`] unless closed or finalised.
    pub fn final_record(&self) -> Result<MeetingFinalRecordV1, MeetingError> {
        if !matches!(
            self.status,
            MeetingStatusV1::Closed | MeetingStatusV1::Finalized
        ) {
            return Err(MeetingError::InvalidTransition {
                from: self.status,
                to: "build the final record of",
            });
        }
        let items = self
            .items
            .iter()
            .map(|item| {
                let vote = self.vote(item.number);
                FinalItemV1 {
                    item: item.clone(),
                    vote_tx_id: vote.and_then(|vote| vote.finalized().map(|(tx, _)| tx)),
                    outcome: vote.and_then(|vote| {
                        vote.evaluation().map(|summary| {
                            if summary.accepted() {
                                "accepted"
                            } else {
                                "rejected"
                            }
                            .to_owned()
                        })
                    }),
                }
            })
            .collect();
        Ok(MeetingFinalRecordV1 {
            meeting_id: self.id().ok_or(MeetingError::InvalidTransition {
                from: self.status,
                to: "build the final record of",
            })?,
            metadata: self.metadata.clone(),
            opened_at_height: self.opened_at.ok_or(MeetingError::InvalidTransition {
                from: self.status,
                to: "build the final record of",
            })?,
            items,
        })
    }

    /// Finalises the meeting: every vote to the chain, then the final record.
    ///
    /// Each vote is finalised in its own transaction, as `irena-vote` does it, so a
    /// shareholder can verify one vote without the meeting. A vote already finalised
    /// is left alone, so an interrupted finalisation is resumed rather than repeated.
    /// The final record is the last block, naming every vote transaction.
    ///
    /// # Errors
    ///
    /// [`MeetingError::InvalidTransition`] unless closed; [`MeetingError::Vote`]
    /// naming the item whose vote could not be finalised; the chain's own errors.
    pub fn finalize(
        &mut self,
        store: &LocalChainStore,
        key: &SigningKey,
        notarisation: &NotarisationV1,
        timestamp_millis: u64,
    ) -> Result<MeetingFinalizedV1, MeetingError> {
        self.expect_status(MeetingStatusV1::Closed, "finalize")?;
        for (number, vote) in &mut self.votes {
            let number = *number;
            if vote.finalized().is_some() {
                continue;
            }
            vote.finalize(store, key, timestamp_millis)
                .map_err(|source| MeetingError::Vote { number, source })?;
        }
        let record = self.final_record()?;
        let company = CompanyIdV1::new(self.company.clone())?;
        let payload = compose_final(&company, notarisation, &record)?;
        let (tx_id, height) = self.append(store, key, payload, timestamp_millis)?;
        self.finalized = Some((tx_id, height));
        self.status = MeetingStatusV1::Finalized;
        Ok(MeetingFinalizedV1 {
            tx_id,
            height,
            record,
        })
    }

    /// Appends one meeting record in its own block.
    fn append(
        &self,
        store: &LocalChainStore,
        key: &SigningKey,
        payload: String,
        timestamp_millis: u64,
    ) -> Result<(TxId, BlockHeight), MeetingError> {
        let head = store.head()?;
        let parent = store
            .get_block(head.height)?
            .ok_or_else(|| MeetingError::Chain {
                detail: format!("the chain head is {head} but no block is stored there"),
            })?;
        let height = head.height.next()?;
        let transaction = key.sign_transaction(TransactionDraft {
            namespace: Namespace::new(MEETING_NAMESPACE)?,
            schema_version: SchemaVersion(MEETING_SCHEMA_VERSION),
            payload: payload.into_bytes(),
            signer: key.public_key(),
            nonce: height.value(),
        });
        let tx_id = transaction.id;
        let block = parent
            .header
            .child_draft(vec![transaction], timestamp_millis)?
            .build()?;
        store.append_block(block)?;
        Ok((tx_id, height))
    }
}

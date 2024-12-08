// Copyright © Aptos Foundation
// SPDX-License-Identifier: Apache-2.0

use crate::metrics::TIMER;
use aptos_metrics_core::TimerHelper;
use aptos_storage_interface::state_store::per_version_state_update_refs::PerVersionStateUpdateRefs;
use aptos_types::transaction::{Transaction, TransactionOutput, Version};
use itertools::{izip, Itertools};
use std::{
    fmt::{Debug, Formatter},
    ops::Deref,
};
use aptos_storage_interface::state_store::state_update::StateUpdateRef;
use aptos_storage_interface::state_store::state_update_ref_map::BatchedStateUpdateRefs;
use aptos_types::state_store::state_key::StateKey;
use aptos_types::write_set::WriteSet;

#[derive(Debug, Default)]
pub struct TransactionsWithOutput {
    pub transactions: Vec<Transaction>,
    pub transaction_outputs: Vec<TransactionOutput>,
}

impl TransactionsWithOutput {
    pub fn new(
        transactions: Vec<Transaction>,
        transaction_outputs: Vec<TransactionOutput>,
    ) -> Self {
        assert_eq!(transactions.len(), transaction_outputs.len());
        Self {
            transactions,
            transaction_outputs,
        }
    }

    pub fn new_empty() -> Self {
        Self::default()
    }

    pub fn push(&mut self, transaction: Transaction, transaction_output: TransactionOutput) {
        self.transactions.push(transaction);
        self.transaction_outputs.push(transaction_output);
    }

    pub fn len(&self) -> usize {
        self.transactions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Transaction, &TransactionOutput)> {
        izip!(self.transactions.iter(), self.transaction_outputs.iter(),)
    }
}

#[ouroboros::self_referencing]
pub struct TransactionsToKeep {
    transactions_with_output: TransactionsWithOutput,
    is_reconfig: bool,
    #[borrows(transactions_with_output)]
    #[covariant]
    state_update_refs: StateUpdateRefs<'this>,
}

impl TransactionsToKeep {
    pub fn index(
        first_version: Version,
        transactions_with_output: TransactionsWithOutput,
        is_reconfig: bool,
    ) -> Self {
        let _timer = TIMER.timer_with(&["transactions_to_keep__index"]);

        TransactionsToKeepBuilder {
            transactions_with_output,
            is_reconfig,
            state_update_refs_builder: |transactions_with_output| {
                let write_sets = transactions_with_output
                    .transaction_outputs
                    .iter()
                    .map(TransactionOutput::write_set);
                let last_checkpoint_index = Self::get_last_checkpoint_index(
                    is_reconfig,
                    &transactions_with_output.transactions,
                );
                StateUpdateRefs::index_write_sets(
                    first_version,
                    write_sets,
                    transactions_with_output.len(),
                    last_checkpoint_index,
                )
            },
        }
        .build()
    }

    pub fn make(
        first_version: Version,
        transactions: Vec<Transaction>,
        transaction_outputs: Vec<TransactionOutput>,
        is_reconfig: bool,
    ) -> Self {
        let txns_with_output = TransactionsWithOutput::new(transactions, transaction_outputs);
        Self::index(first_version, txns_with_output, is_reconfig)
    }

    pub fn new_empty() -> Self {
        Self::make(0, vec![], vec![], false)
    }

    pub fn new_dummy_success(txns: Vec<Transaction>) -> Self {
        let txn_outputs = vec![TransactionOutput::new_empty_success(); txns.len()];
        Self::make(0, txns, txn_outputs, false)
    }

    pub fn per_version_state_update_refs(&self) -> &PerVersionStateUpdateRefs {
        &self.borrow_state_update_refs().per_version
    }

    pub fn state_update_refs_for_last_checkpoint(&self) -> Option<&BatchedStateUpdateRefs> {
        self.borrow_state_update_refs().for_last_checkpoint.as_ref()
    }

    pub fn state_update_refs_for_latest(&self) -> &BatchedStateUpdateRefs {
        &self.borrow_state_update_refs().for_latest
    }

    pub fn is_reconfig(&self) -> bool {
        *self.borrow_is_reconfig()
    }

    pub fn ends_with_sole_checkpoint(&self) -> bool {
        let _timer = TIMER.timer_with(&["ends_with_sole_checkpoint"]);

        if self.is_reconfig() {
            !self
                .transactions
                .iter()
                .any(Transaction::is_non_reconfig_block_ending)
        } else {
            self.transactions
                .iter()
                .position(Transaction::is_non_reconfig_block_ending)
                == Some(self.len() - 1)
        }
    }

    fn get_last_checkpoint_index(
        is_reconfig: bool,
        transactions: &[Transaction],
    ) -> Option<usize> {
        let _timer = TIMER.timer_with(&["get_last_checkpoint_index"]);

        if is_reconfig {
            return Some(transactions.len() - 1);
        }

        transactions
            .iter()
            .rposition(Transaction::is_non_reconfig_block_ending)
    }
}

impl Deref for TransactionsToKeep {
    type Target = TransactionsWithOutput;

    fn deref(&self) -> &Self::Target {
        self.borrow_transactions_with_output()
    }
}

impl Debug for TransactionsToKeep {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransactionsToKeep")
            .field(
                "transactions_with_output",
                self.borrow_transactions_with_output(),
            )
            .field("is_reconfig", &self.is_reconfig())
            .finish()
    }
}

struct StateUpdateRefs<'kv> {
    per_version: PerVersionStateUpdateRefs<'kv>,
    for_last_checkpoint: Option<BatchedStateUpdateRefs<'kv>>,
    for_latest: BatchedStateUpdateRefs<'kv>,
}

impl<'kv> StateUpdateRefs<'kv> {
    pub fn index_write_sets(
        first_version: Version,
        write_sets: impl IntoIterator<Item = &'kv WriteSet>,
        num_write_sets: usize,
        last_checkpoint_index: Option<usize>,
    ) -> Self {
        let per_version = PerVersionStateUpdateRefs::index_write_sets(
            first_version,
            write_sets,
            num_write_sets,
        );
        let (for_last_checkpoint, for_latest) = Self::collect_updates(&per_version, last_checkpoint_index);
        Self {
            per_version,
            for_last_checkpoint,
            for_latest,
        }
    }

    fn collect_updates(
        per_version_updates: &PerVersionStateUpdateRefs<'kv>,
        last_checkpoint_index: Option<usize>,
    ) -> (
        Option<BatchedStateUpdateRefs<'kv>>,
        BatchedStateUpdateRefs<'kv>,
    ) {
        let _timer = TIMER.timer_with(&["txns_to_keep__collect_updates"]);

        let mut shard_iters = per_version_updates
            .shards
            .iter()
            .map(|shard| shard.iter().cloned())
            .collect::<Vec<_>>();

        let mut first_version = per_version_updates.first_version;
        let mut num_versions = per_version_updates.num_versions;
        let updates_for_last_checkpoint = last_checkpoint_index.map(|idx| {
            let ret = Self::collect_some_updates(first_version, idx + 1, &mut shard_iters);
            first_version += idx as Version + 1;
            num_versions -= idx + 1;
            ret
        });
        let updates_for_latest =
            Self::collect_some_updates(first_version, num_versions, &mut shard_iters);

        // Assert that all updates are consumed.
        assert!(shard_iters.iter_mut().all(|iter| iter.next().is_none()));

        (updates_for_last_checkpoint, updates_for_latest)
    }

    fn collect_some_updates(
        first_version: Version,
        num_versions: usize,
        shard_iters: &mut [impl Iterator<Item = (&'kv StateKey, StateUpdateRef<'kv>)> + Clone],
    ) -> BatchedStateUpdateRefs<'kv> {
        let mut ret = BatchedStateUpdateRefs::new_empty(first_version, num_versions);
        // exclusive
        let end_version = first_version + num_versions as Version;
        izip!(shard_iters, &mut ret.shards).for_each(|(shard_iter, dedupped)| {
            dedupped.extend(
                shard_iter
                    // n.b. take_while_ref so that in the next step we can process the rest of the entries from the iters.
                    .take_while_ref(|(_k, u)| u.version < end_version),
            )
        });
        ret
    }
}

// Copyright © Aptos Foundation
// Parts of the project are originally copyright © Meta Platforms, Inc.
// SPDX-License-Identifier: Apache-2.0

//! This file defines transaction store APIs that are related to committed signed transactions.

use crate::ledger_db::LedgerDb;
use aptos_db_indexer_schemas::{
    schema::{ordered_transaction_by_account::OrderedTransactionByAccountSchema, orderless_transaction_by_account::OrderlessTransactionByAccountSchema, transaction_summaries_by_account::TransactionSummariesByAccountSchema},
    utils::{AccountOrderedTransactionsIter, AccountTransactionSummariesIter},
};
use aptos_schemadb::{iterator::ScanDirection, SchemaBatch};
use aptos_storage_interface::{AptosDbError, Result};
use aptos_types::{
    account_address::AccountAddress,
    transaction::{ReplayProtector, Transaction, Version},
};
use std::sync::Arc;

#[cfg(test)]
mod test;

#[derive(Clone, Debug)]
pub struct TransactionStore {
    ledger_db: Arc<LedgerDb>,
}

impl TransactionStore {
    pub fn new(ledger_db: Arc<LedgerDb>) -> Self {
        Self { ledger_db }
    }

    /// Gets the version of a transaction by the sender `address` and `replay_protector`.
    pub fn get_account_transaction_version(
        &self,
        address: AccountAddress,
        replay_protector: ReplayProtector,
        ledger_version: Version,
    ) -> Result<Option<Version>> {
        match replay_protector {
            ReplayProtector::SequenceNumber(seq_num) => {
                if let Some(version) = self
                    .ledger_db
                    .transaction_db_raw()
                    .get::<OrderedTransactionByAccountSchema>(&(address, seq_num))?
                {
                    if version <= ledger_version {
                        return Ok(Some(version));
                    }
                }
            },
            ReplayProtector::Nonce(nonce) => {
                if let Some(version) = self
                    .ledger_db
                    .transaction_db_raw()
                    .get::<OrderlessTransactionByAccountSchema>(&(address, nonce))?
                {
                    if version <= ledger_version {
                        return Ok(Some(version));
                    }
                }
            }
        }
        

        Ok(None)
    }

    /// Gets an iterator that yields `(sequence_number, version)` for each
    /// transaction sent by an account, with minimum sequence number greater
    /// `min_seq_num`, and returning at most `num_versions` results with
    /// `version <= ledger_version`.
    /// Guarantees that the returned sequence numbers are sequential, i.e.,
    /// `seq_num_{i} + 1 = seq_num_{i+1}`.
    pub fn get_account_ordered_transactions_iter(
        &self,
        address: AccountAddress,
        min_seq_num: u64,
        num_versions: u64,
        ledger_version: Version,
    ) -> Result<AccountOrderedTransactionsIter> {
        let mut iter = self
            .ledger_db
            .transaction_db_raw()
            .iter::<OrderedTransactionByAccountSchema>()?;
        iter.seek(&(address, min_seq_num))?;
        Ok(AccountOrderedTransactionsIter::new(
            iter,
            address,
            min_seq_num
                .checked_add(num_versions)
                .ok_or(AptosDbError::TooManyRequested(min_seq_num, num_versions))?,
            ledger_version,
        ))
    }

    pub fn get_account_transaction_summaries_iter(
        &self,
        address: AccountAddress,
        start_version: Option<u64>,
        end_version: Option<u64>,
        limit: u64,
        ledger_version: Version,
    ) -> Result<AccountTransactionSummariesIter> {
        if start_version.is_some() {
            let mut iter = self
                                                                        .ledger_db
                                                                        .transaction_db_raw()
                                                                        .iter::<TransactionSummariesByAccountSchema>()?;
            iter.seek(&(address, start_version.unwrap()))?;
            Ok(AccountTransactionSummariesIter::new(
                iter,
                address,
                start_version,
                end_version,
                limit,
                ScanDirection::Forward,
                ledger_version,
            ))
        } else if end_version.is_some() {
            let mut iter = self.ledger_db
            .transaction_db_raw().rev_iter::<TransactionSummariesByAccountSchema>()?;
            iter.seek_for_prev(&(address, end_version.unwrap()))?;
            Ok(AccountTransactionSummariesIter::new(
                iter,
                address,
                start_version,
                end_version,
                limit,
                ScanDirection::Backward,
                ledger_version,
            ))
        } else {
            let mut iter = self.ledger_db
            .transaction_db_raw().rev_iter::<TransactionSummariesByAccountSchema>()?;
            iter.seek_for_prev(&(address, u64::MAX))?;
            Ok(AccountTransactionSummariesIter::new(
                iter,
                address,
                start_version,
                Some(u64::MAX),
                limit,
                ScanDirection::Backward,
                ledger_version,
            ))
        }
    }

    /// Prune the transaction by account store given a list of transaction
    pub fn prune_transaction_by_account(
        &self,
        transactions: &[Transaction],
        db_batch: &SchemaBatch,
    ) -> Result<()> {
        for transaction in transactions {
            if let Some(txn) = transaction.try_as_signed_user_txn() {
                match txn.replay_protector() {
                    ReplayProtector::SequenceNumber(seq_num) => {
                        db_batch.delete::<OrderedTransactionByAccountSchema>(&(txn.sender(), seq_num))?;
                    },
                    ReplayProtector::Nonce(nonce) => {
                        db_batch.delete::<OrderlessTransactionByAccountSchema>(&(txn.sender(), nonce))?;
                    }
                }
            }
        }
        Ok(())
    }
}

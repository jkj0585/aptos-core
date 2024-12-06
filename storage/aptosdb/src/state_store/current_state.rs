// Copyright (c) Aptos Foundation
// SPDX-License-Identifier: Apache-2.0

use aptos_storage_interface::state_store::{
    state::LedgerState, state_delta::StateDelta, state_summary::StateWithSummary,
};
use derive_more::{Deref, DerefMut};

#[derive(Clone, Debug, Deref, DerefMut)]
pub(crate) struct LedgerStateWithSummary {
    #[deref]
    #[deref_mut]
    latest: StateWithSummary,
    last_checkpoint: StateWithSummary,
}

impl LedgerStateWithSummary {
    pub fn new(latest: StateWithSummary, last_checkpoint: StateWithSummary) -> Self {
        assert!(latest.follows(&last_checkpoint));
        Self {
            latest,
            last_checkpoint,
        }
    }

    pub fn new_at_checkpoint(checkpoint: StateWithSummary) -> Self {
        Self::new(checkpoint.clone(), checkpoint)
    }

    pub fn new_dummy() -> Self {
        let empty = StateWithSummary::new_empty();
        Self::new(empty.clone(), empty)
    }

    pub fn set(&mut self, current_state: LedgerStateWithSummary) {
        *self = current_state;
    }

    pub fn last_checkpoint(&self) -> &StateWithSummary {
        &self.last_checkpoint
    }

    pub fn follows(&self, rhs: &Self) -> bool {
        self.latest.follows(&rhs.latest) && self.last_checkpoint.follows(&rhs.last_checkpoint)
    }
}

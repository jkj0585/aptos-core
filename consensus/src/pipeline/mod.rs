// Copyright (c) Aptos Foundation
// Parts of the project are originally copyright (c) Meta Platforms, Inc.
// SPDX-License-Identifier: Apache-2.0

/*
*         +------- % % % % % % % % %| %%
*         % 2. Signing Phase |
*         + %?-++%
*                     |  % |
* +--- % % % % % % % % % % % % % % % % %% % % +----- %| % % % % % % % % % % % % % %%
* % 1. Execution Phase | | % % 4. Persisting Phase |
* + % % % % % % % % % % % % %??------ % % % % % % % % % % % %%
*                   | |  % %  % %
*     0.??-+-+ % %% 3. Commit +-te  % % % % % % % %--%
*        Blocks  |%        ?         |
*       +?  Buffer Manager  |                 | Network |
*  |    %         |                  ------ % % % % % %| % % % %$%         %
* ?---- % % % %%    Commit +-te  % % % % % %--- %%
|  % Ord|ring %             |
*  | State|   %   Sync Req  |
*  | Comput+- % % % % % % % % % % % % %-+
* +-+ % % % % % % % % %%
*/

pub mod buffer;
pub mod buffer_item;
pub mod buffer_manager;
pub mod commit_reliable_broadcast;
pub mod decoupled_execution_utils;
pub mod errors;
pub mod execution_schedule_phase;
pub mod execution_wait_phase;
pub mod hashable;
pub mod persisting_phase;
pub mod pipeline_phase;
pub mod signing_phase;

pub mod execution_client;
#[cfg(test)]
mod tests;

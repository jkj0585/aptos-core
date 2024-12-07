// Copyright (c) Aptos Foundation
// SPDX-License-Identifier: Apache-2.0

use crate::{config::HistoricalDataServiceConfig, connection_manager::ConnectionManager};
use aptos_indexer_grpc_utils::file_store_operator_v2::FileStoreOperatorV2;
use aptos_protos::indexer::v1::{GetTransactionsRequest, TransactionsResponse};
use futures::executor::block_on;
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tonic::{Request, Status};
use tracing::info;
use uuid::Uuid;

// TODO(grao): This should be read from metadata instead.
const NUM_TXNS_PER_FOLDER: u64 = 10000;

pub struct HistoricalDataService {
    chain_id: u64,
    connection_manager: Arc<ConnectionManager>,
    file_store_operator: FileStoreOperatorV2,
}

impl HistoricalDataService {
    pub fn new(
        chain_id: u64,
        config: HistoricalDataServiceConfig,
        connection_manager: Arc<ConnectionManager>,
    ) -> Self {
        let file_store = block_on(config.file_store_config.create_filestore());
        let file_store_operator =
            FileStoreOperatorV2::new(chain_id, file_store, NUM_TXNS_PER_FOLDER);
        Self {
            chain_id,
            connection_manager: connection_manager.clone(),
            file_store_operator,
        }
    }

    pub fn run(
        &self,
        mut handler_rx: Receiver<(
            Request<GetTransactionsRequest>,
            Sender<Result<TransactionsResponse, Status>>,
        )>,
    ) {
        info!("Running HistoricalDataService...");
        tokio_scoped::scope(|scope| {
            while let Some((request, response_sender)) = handler_rx.blocking_recv() {
                // TODO(grao): Store request metadata.
                let request = request.into_inner();
                let id = Uuid::new_v4().to_string();
                info!("Received request: {request:?}.");

                if request.starting_version.is_none() {
                    let err = Err(Status::invalid_argument("Must provide starting_version."));
                    info!("Client error: {err:?}.");
                    let _ = response_sender.blocking_send(err);
                    continue;
                }
                let starting_version = request.starting_version.unwrap();

                let max_num_transactions_per_batch = if let Some(batch_size) = request.batch_size {
                    batch_size as usize
                } else {
                    10000
                };

                let ending_version = request
                    .transactions_count
                    .map(|count| starting_version + count);

                scope.spawn(async move {
                    self.start_streaming(
                        id,
                        starting_version,
                        ending_version,
                        max_num_transactions_per_batch,
                        response_sender,
                    )
                    .await
                });
            }
        });
    }

    pub(crate) fn get_connection_manager(&self) -> &ConnectionManager {
        &self.connection_manager
    }

    async fn start_streaming(
        &self,
        id: String,
        starting_version: u64,
        ending_version: Option<u64>,
        max_num_transactions_per_batch: usize,
        response_sender: tokio::sync::mpsc::Sender<Result<TransactionsResponse, Status>>,
    ) {
        info!("Start streaming, starting_version: {starting_version}, ending_version: {ending_version:?}.");
        self.connection_manager
            .insert_active_stream(&id, starting_version, ending_version);
        let mut next_version = starting_version;
        let ending_version = ending_version.unwrap_or(u64::MAX);
        'out: loop {
            self.connection_manager
                .update_stream_progress(&id, next_version);
            if next_version >= ending_version {
                break;
            }
            if !self.file_store_operator.can_serve(next_version).await {
                info!("next_version {next_version} is larger or equal than file store version, terminate the stream.");
                break;
            }

            // TODO(grao): Pick a better channel size here, and consider doing parallel fetching
            // inside the `get_transaction_batch` call based on the channel size.
            let (tx, mut rx) = channel(1);
            self.file_store_operator
                .get_transaction_batch(
                    next_version,
                    /*retries=*/ 3,
                    /*max_files=*/ None,
                    tx,
                )
                .await;

            let mut close_to_latest = false;
            while let Some(transactions) = rx.recv().await {
                next_version += transactions.len() as u64;
                let timestamp = transactions.first().unwrap().timestamp.unwrap();
                let timestamp_since_epoch =
                    Duration::new(timestamp.seconds as u64, timestamp.nanos as u32);
                let now_since_epoch = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
                let delta = now_since_epoch.saturating_sub(timestamp_since_epoch);

                if delta < Duration::from_secs(60) {
                    close_to_latest = true;
                }
                let responses = transactions
                    .chunks(max_num_transactions_per_batch)
                    .into_iter()
                    .map(|chunk| TransactionsResponse {
                        transactions: chunk.to_vec(),
                        chain_id: Some(self.chain_id),
                    });
                for response in responses {
                    if let Err(_) = response_sender.send(Ok(response)).await {
                        info!("Client dropped.");
                        break 'out;
                    }
                }
            }
            if close_to_latest {
                info!("Stream is approaching to the latest transactions, terminate.");
                break;
            }
        }

        self.connection_manager.remove_active_stream(&id);
    }
}

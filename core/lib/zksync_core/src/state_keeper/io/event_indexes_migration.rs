//! Temporary module for migrating fee addresses from L1 batches to miniblocks.

use std::{ops, time::Duration};

use anyhow::Context as _;
use tokio::sync::watch;
use zksync_dal::{ConnectionPool, StorageProcessor};
use zksync_types::MiniblockNumber;

/// Runs the migration for miniblocks. Should be run as a background task.
pub(crate) async fn migrate_miniblocks(
    pool: ConnectionPool,
    last_miniblock: MiniblockNumber,
    stop_receiver: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let MigrationOutput { events_affected } = migrate_miniblocks_inner(
        pool,
        last_miniblock,
        100_000,
        Duration::from_secs(1),
        stop_receiver,
    )
    .await?;

    tracing::info!("Finished event indexes migration with {events_affected} affected events");
    Ok(())
}

#[derive(Debug, Default)]
struct MigrationOutput {
    events_affected: u64,
}

/// It's important for the `chunk_size` to be a constant; this ensures that each chunk is migrated atomically.
async fn migrate_miniblocks_inner(
    pool: ConnectionPool,
    last_miniblock: MiniblockNumber,
    chunk_size: u32,
    sleep_interval: Duration,
    stop_receiver: watch::Receiver<bool>,
) -> anyhow::Result<MigrationOutput> {
    anyhow::ensure!(chunk_size > 0, "Chunk size must be positive");

    let mut chunk_start = MiniblockNumber(0);
    let mut events_affected = 0;

    tracing::info!(
        "Reassigning log indexes without ETH transfer for miniblocks {chunk_start}..={last_miniblock} \
         in chunks of {chunk_size} miniblocks"
    );

    while chunk_start <= last_miniblock {
        let chunk_end = last_miniblock.min(chunk_start + chunk_size - 1);
        let chunk = chunk_start..=chunk_end;

        let mut storage = pool.access_storage().await?;
        let is_chunk_migrated = are_event_indexes_migrated(&mut storage, chunk.clone()).await?;

        if is_chunk_migrated {
            tracing::debug!("Event indexes are migrated for chunk {chunk:?}");
        } else {
            tracing::debug!("Migrating event indexes for miniblocks chunk {chunk:?}");

            let rows_affected = storage
                .events_dal()
                .assign_indexes_without_eth_transfer(chunk.clone())
                .await
                .with_context(|| {
                    format!("Failed migrating events in miniblocks, chunk {chunk:?}")
                })?;
            tracing::debug!("Migrated {rows_affected} events in chunk {chunk:?}");
            events_affected += rows_affected;
        }
        drop(storage);

        if *stop_receiver.borrow() {
            tracing::info!("Stop signal received; event index migration shutting down");
            return Ok(MigrationOutput { events_affected });
        }
        chunk_start = chunk_end + 1;

        if !is_chunk_migrated {
            tokio::time::sleep(sleep_interval).await;
        }
    }

    Ok(MigrationOutput { events_affected })
}

#[allow(deprecated)]
async fn are_event_indexes_migrated(
    storage: &mut StorageProcessor<'_>,
    range: ops::RangeInclusive<MiniblockNumber>,
) -> anyhow::Result<bool> {
    storage
        .events_dal()
        .are_event_indexes_migrated(range.clone())
        .await
        .with_context(|| {
            format!(
                "Failed getting event indexes for miniblocks in range #{}..=#{}",
                range.start().0,
                range.end().0
            )
        })
}

#[cfg(test)]
mod tests {
    use multivm::zk_evm_1_3_1::ethereum_types::H256;
    use test_casing::test_casing;
    use zksync_system_constants::{L2_ETH_TOKEN_ADDRESS, TRANSFER_EVENT_TOPIC};
    use zksync_types::{
        api::GetLogsFilter, tx::IncludedTxLocation, Address, L1BatchNumber, ProtocolVersion,
        VmEvent,
    };

    use super::*;
    use crate::utils::testonly::create_miniblock;

    async fn store_events(
        storage: &mut StorageProcessor<'_>,
        miniblock_number: u32,
        start_idx: u32,
    ) -> anyhow::Result<(IncludedTxLocation, Vec<VmEvent>)> {
        let new_miniblock = create_miniblock(miniblock_number);
        storage
            .blocks_dal()
            .insert_miniblock(&new_miniblock)
            .await?;
        let tx_location = IncludedTxLocation {
            tx_hash: H256::repeat_byte(1),
            tx_index_in_miniblock: 0,
            tx_initiator_address: Address::repeat_byte(2),
        };
        let events = vec![
            // Matches address, doesn't match topics
            VmEvent {
                location: (L1BatchNumber(1), start_idx),
                address: Address::repeat_byte(23),
                indexed_topics: vec![],
                value: start_idx.to_le_bytes().to_vec(),
            },
            // Doesn't match address, matches topics
            VmEvent {
                location: (L1BatchNumber(1), start_idx + 1),
                address: Address::zero(),
                indexed_topics: vec![H256::repeat_byte(42)],
                value: (start_idx + 1).to_le_bytes().to_vec(),
            },
            // Doesn't match address or topics
            VmEvent {
                location: (L1BatchNumber(1), start_idx + 2),
                address: Address::zero(),
                indexed_topics: vec![H256::repeat_byte(1), H256::repeat_byte(42)],
                value: (start_idx + 2).to_le_bytes().to_vec(),
            },
            // Matches both address and topics
            VmEvent {
                location: (L1BatchNumber(1), start_idx + 3),
                address: Address::repeat_byte(23),
                indexed_topics: vec![H256::repeat_byte(42), H256::repeat_byte(111)],
                value: (start_idx + 3).to_le_bytes().to_vec(),
            },
            VmEvent {
                location: (L1BatchNumber(1), start_idx + 4),
                address: L2_ETH_TOKEN_ADDRESS,
                indexed_topics: vec![TRANSFER_EVENT_TOPIC],
                value: (start_idx + 4).to_le_bytes().to_vec(),
            },
            // ETH Transfer event with only topic matching
            VmEvent {
                location: (L1BatchNumber(1), start_idx + 5),
                address: Address::repeat_byte(12),
                indexed_topics: vec![TRANSFER_EVENT_TOPIC],
                value: (start_idx + 5).to_le_bytes().to_vec(),
            },
            // ETH Transfer event with only address matching
            VmEvent {
                location: (L1BatchNumber(1), start_idx + 6),
                address: L2_ETH_TOKEN_ADDRESS,
                indexed_topics: vec![H256::repeat_byte(25)],
                value: (start_idx + 6).to_le_bytes().to_vec(),
            },
        ];

        storage
            .events_dal()
            .save_events(
                MiniblockNumber(miniblock_number),
                &[(tx_location, events.iter().collect())],
            )
            .await;
        Ok((tx_location, events))
    }

    async fn prepare_storage(storage: &mut StorageProcessor<'_>) {
        storage
            .protocol_versions_dal()
            .save_protocol_version_with_tx(ProtocolVersion::default())
            .await;
        for number in 0..5 {
            store_events(storage, number, 0).await.unwrap();
        }

        // Remove indexes here to understand that migration works correctly.
        storage
            .events_dal()
            .remove_event_indexes_without_eth_transfer(MiniblockNumber(0)..=MiniblockNumber(5))
            .await
            .unwrap();
    }

    async fn assert_migration(storage: &mut StorageProcessor<'_>) {
        assert!(
            are_event_indexes_migrated(storage, MiniblockNumber(0)..=MiniblockNumber(5))
                .await
                .unwrap()
        );

        for number in 0..5 {
            let filter = GetLogsFilter {
                from_block: number.into(),
                to_block: number.into(),
                addresses: vec![],
                topics: vec![],
            };

            let raw_logs = storage
                .events_web3_dal()
                .get_raw_logs(filter, 1000)
                .await
                .unwrap();

            assert_eq!(raw_logs.len(), 7);

            for (i, log) in raw_logs.iter().enumerate() {
                if log.address == L2_ETH_TOKEN_ADDRESS.as_bytes()
                    && log.topic1 == TRANSFER_EVENT_TOPIC.as_bytes()
                {
                    assert_eq!(log.event_index_in_block_without_eth_transfer, Some(0));
                    assert_eq!(log.event_index_in_tx_without_eth_transfer, Some(0));
                } else if i < 4 {
                    assert_eq!(
                        log.event_index_in_block_without_eth_transfer,
                        Some(i as i32)
                    );
                    assert_eq!(log.event_index_in_tx_without_eth_transfer, Some(i as i32));
                } else {
                    assert_eq!(
                        log.event_index_in_block_without_eth_transfer,
                        Some(i as i32 - 1)
                    );
                    assert_eq!(
                        log.event_index_in_tx_without_eth_transfer,
                        Some(i as i32 - 1)
                    );
                }
            }
        }
    }

    #[test_casing(3, [1, 2, 3])]
    #[tokio::test]
    async fn migration_basics(chunk_size: u32) {
        let pool = ConnectionPool::test_pool().await;
        let mut storage = pool.access_storage().await.unwrap();
        prepare_storage(&mut storage).await;

        let raw_logs = storage
            .events_web3_dal()
            .get_raw_logs(
                GetLogsFilter {
                    from_block: 0.into(),
                    to_block: 4.into(),
                    addresses: vec![],
                    topics: vec![],
                },
                1000,
            )
            .await
            .unwrap();

        drop(storage);

        let (_stop_sender, stop_receiver) = watch::channel(false);
        let result = migrate_miniblocks_inner(
            pool.clone(),
            MiniblockNumber(4),
            chunk_size,
            Duration::ZERO,
            stop_receiver.clone(),
        )
        .await
        .unwrap();

        assert_eq!(result.events_affected, raw_logs.len() as u64);

        // Check that all blocks are migrated.
        let mut storage = pool.access_storage().await.unwrap();
        assert_migration(&mut storage).await;
        drop(storage);

        // Check that migration can run again w/o returning an error, hanging up etc.
        let result = migrate_miniblocks_inner(
            pool.clone(),
            MiniblockNumber(4),
            chunk_size,
            Duration::ZERO,
            stop_receiver,
        )
        .await
        .unwrap();

        assert_eq!(result.events_affected, 0);
    }

    #[test_casing(3, [1, 2, 3])]
    #[tokio::test]
    async fn stopping_and_resuming_migration(chunk_size: u32) {
        let pool = ConnectionPool::test_pool().await;
        let mut storage = pool.access_storage().await.unwrap();
        prepare_storage(&mut storage).await;

        let (_stop_sender, stop_receiver) = watch::channel(true); // signal stop right away
        let result = migrate_miniblocks_inner(
            pool.clone(),
            MiniblockNumber(4),
            chunk_size,
            Duration::from_secs(1_000),
            stop_receiver,
        )
        .await
        .unwrap();

        // Migration should stop after a single chunk.
        assert_eq!(result.events_affected, u64::from(chunk_size) * 7);

        // Check that migration resumes from the same point.
        let (_stop_sender, stop_receiver) = watch::channel(false);
        let result = migrate_miniblocks_inner(
            pool.clone(),
            MiniblockNumber(4),
            chunk_size,
            Duration::ZERO,
            stop_receiver,
        )
        .await
        .unwrap();

        assert_eq!(result.events_affected, (5 - u64::from(chunk_size)) * 7);
        assert_migration(&mut storage).await;
    }

    #[test_casing(3, [1, 2, 3])]
    #[tokio::test]
    async fn new_blocks_added_during_migration(chunk_size: u32) {
        let pool = ConnectionPool::test_pool().await;
        let mut storage = pool.access_storage().await.unwrap();
        prepare_storage(&mut storage).await;

        let (_stop_sender, stop_receiver) = watch::channel(true); // signal stop right away
        let result = migrate_miniblocks_inner(
            pool.clone(),
            MiniblockNumber(4),
            chunk_size,
            Duration::from_secs(1_000),
            stop_receiver,
        )
        .await
        .unwrap();

        // Migration should stop after a single chunk.
        assert_eq!(result.events_affected, u64::from(chunk_size) * 7);

        // Insert a new miniblock with new events into storage, indexes are assigned automatically
        store_events(&mut storage, 5, 0).await.unwrap();

        // Resume the migration.
        let (_stop_sender, stop_receiver) = watch::channel(false);
        let result = migrate_miniblocks_inner(
            pool.clone(),
            MiniblockNumber(5),
            chunk_size,
            Duration::ZERO,
            stop_receiver,
        )
        .await
        .unwrap();

        // The new miniblock should not be affected.
        assert_eq!(result.events_affected, (5 - u64::from(chunk_size)) * 7);
        assert_migration(&mut storage).await;
    }
}

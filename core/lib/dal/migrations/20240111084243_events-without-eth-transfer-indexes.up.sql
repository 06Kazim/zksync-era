ALTER TABLE events
    ADD COLUMN IF NOT EXISTS event_index_in_block_without_eth_transfer INT DEFAULT NULL;
ALTER TABLE events
    ADD COLUMN IF NOT EXISTS event_index_in_tx_without_eth_transfer INT DEFAULT NULL;

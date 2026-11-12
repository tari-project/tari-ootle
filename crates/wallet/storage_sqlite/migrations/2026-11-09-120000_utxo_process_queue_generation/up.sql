-- Every enqueue of a row's pair advances its generation, so that recovery only acknowledges the work it looked up.
ALTER TABLE utxo_process_queue ADD COLUMN generation BIGINT NOT NULL DEFAULT 0;

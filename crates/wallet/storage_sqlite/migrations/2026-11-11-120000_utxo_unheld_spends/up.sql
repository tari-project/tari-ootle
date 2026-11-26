-- The spends the scanner saw of outputs the wallet does not hold, in the order seen. Recovery stores an output only
-- when no spend of it was seen after the output was looked up.
CREATE TABLE utxo_unheld_spends
(
    id               INTEGER PRIMARY KEY AUTOINCREMENT NOT NULL,
    resource_address TEXT                              NOT NULL,
    commitment       TEXT                              NOT NULL
);

CREATE INDEX utxo_unheld_spends_resource_commitment ON utxo_unheld_spends (resource_address, commitment);

-- Update resources

UPDATE resources
SET token_symbol = 'tTARI',
    metadata     = json_set(metadata, '$.SYMBOL', 'tTARI')
WHERE address = 'resource_0101010101010101010101010101010101010101010101010101010101010101'
  AND token_symbol = 'tXTR';

-- Update vaults

UPDATE vaults
SET token_symbol = 'tTARI'
WHERE resource_address = 'resource_0101010101010101010101010101010101010101010101010101010101010101'
  AND token_symbol = 'tXTR';

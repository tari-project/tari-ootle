# Ristretto Value Lookup Generator

CLI utility for generating and validating Ristretto value-to-public-key lookup tables used by the Tari Ootle wallet.

The generated lookup table is a binary file that maps integer values to their corresponding Ristretto public keys
(`v -> v * G`), enabling fast lookups without expensive runtime computation.

## Usage

### Generate a lookup table

```sh
cargo run -p tari_value_lookup_generator -- --min 0 --max 1000000 -o value_lookup.bin
```

Options:

| Flag | Description |
|------|-------------|
| `-o`, `--output-file` | Path to write the lookup file (default: `<crate_dir>/value_lookup.bin`) |
| `-m`, `--min` | Minimum value to include (default: `0`) |
| `-x`, `--max` | Maximum value to include (required) |
| `-j`, `--jobs` | Number of worker threads (default: number of available cores) |
| `-v`, `--validate` | Validate an existing lookup table instead of generating |

### Validate an existing table

```sh
cargo run -p tari_value_lookup_generator -- --validate -o value_lookup.bin
```

This checks that every entry in the file matches the expected public key for its value. Note that validation of large
tables can take a long time.

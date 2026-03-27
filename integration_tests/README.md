# Integration Tests

This directory contains the integration tests for the Tari Ootle project using Cucumber for behavior-driven development (BDD).

## Running Tests

### Run All Tests

```bash
cargo test --release --test cucumber
```

### Run a Single Test by Name

To run a specific scenario by its name:

```bash
cargo test --release --test cucumber -- --name "Claim base layer burn funds with wallet daemon"
```

### Run Tests by Tag

```bash
cargo test --release --test cucumber -- --tags "@claim_burn"
```


## Test Structure

- `tests/features/` - Contains `.feature` files with Cucumber scenarios
- `src/` - Contains step definitions and test implementation code

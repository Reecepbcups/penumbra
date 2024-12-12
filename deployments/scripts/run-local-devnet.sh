#!/usr/bin/env bash
# Dev tooling to spin up a localhost devnet for Penumbra.
set -euo pipefail


repo_root="$(git rev-parse --show-toplevel)"
# The process-compose file already respects local state and will reuse it.
# "${repo_root}/deployments/scripts/warn-about-pd-state"

>&2 echo "Building binaries from latest code..."
cargo build --release --bin pd
# Also make sure to invoke via `cargo run` so that the process-compose
# spin-up doesn't block on more building/linking.
cargo --quiet run --release --bin pd -- --help > /dev/null

# Generate network from latest code, only if network does not already exist.
if [[ -d ~/.penumbra/network_data ]] ; then
    >&2 echo "network data exists locally, reusing it"
else
    # --allocations-input-file "/home/reece/Desktop/Programming/Rust/penumbra/testnets/base_allocations.csv" \
    cargo run --release --bin pd -- network generate \
        --chain-id penumbra-local-devnet \
        --unbonding-delay 302400 \
        --epoch-duration 302400 \
        --proposal-voting-blocks 50 \
        --gas-price-simple 0 \
        --allocation-address "penumbra1cvp32r5wp4lfnnww3g3fytxccqnu2xcj0r2qm0sa8ekjdezlm3gzk34qtg2xscqx9r6yrhz24k3l6j88q98rexyp7dnupq66cxllvpp9v0lw0xuqf0yfhv5ksfxzv0m968tmxn" \
        --timeout-commit 1s
    # opt in to cometbft abci indexing to postgres
    postgresql_db_url="postgresql://penumbra:penumbra@localhost:5432/penumbra_cometbft?sslmode=disable"
    sed -i -e "s#^indexer.*#indexer = \"psql\"\\npsql-conn = \"$postgresql_db_url\"#" ~/.penumbra/network_data/node0/cometbft/config/config.toml
fi

# Run the core fullnode config, plus any additional params passed via `$@`.
process-compose up --no-server --config "${repo_root}/deployments/compose/process-compose.yml" --keep-tui "$@"

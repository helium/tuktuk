#!/bin/bash

cargo release -p tuktuk-program --execute --no-confirm
cargo release -p tuktuk-sdk --execute --no-confirm
cargo release -p solana-transaction-utils --execute --no-confirm
cargo release -p tuktuk-cli --execute --no-confirm
cargo release -p tuktuk-crank-turner --execute --no-confirm

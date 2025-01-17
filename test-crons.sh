#! /bin/bash

cargo run -p tuktuk-cli -- -u http://127.0.0.1:8899 -w /Users/noahprince/.config/solana/id.json tuktuk-config create --min-deposit 10
cargo run -p tuktuk-cli -- -u http://127.0.0.1:8899 -w /Users/noahprince/.config/solana/id.json task-queue create --capacity 10 --name Noah --funding-amount 100000000 --min-crank-reward 100000000
cargo run -p tuktuk-cli -- -u http://127.0.0.1:8899 -w /Users/noahprince/.config/solana/id.json cron create --name Noah --task-queue-name Noah --schedule "0 * * * * *" --free-tasks-per-transaction 0 --funding-amount 1000000000 --num-tasks-per-queue-call 8

cargo run -p tuktuk-cli -- -u http://127.0.0.1:8899 -w /Users/noahprince/.config/solana/id.json cron-transaction create-remote --url http://localhost:3002/remote --signer $(solana address) --cron-name Noah --index 0
cargo run -p tuktuk-cli -- -u http://127.0.0.1:8899 -w /Users/noahprince/.config/solana/id.json cron-transaction create-remote --url http://localhost:3002/remote --signer $(solana address) --cron-name Noah --index 1
cargo run -p tuktuk-cli -- -u http://127.0.0.1:8899 -w /Users/noahprince/.config/solana/id.json cron-transaction create-remote --url http://localhost:3002/remote --signer $(solana address) --cron-name Noah --index 2
cargo run -p tuktuk-cli -- -u http://127.0.0.1:8899 -w /Users/noahprince/.config/solana/id.json cron-transaction create-remote --url http://localhost:3002/remote --signer $(solana address) --cron-name Noah --index 3
cargo run -p tuktuk-cli -- -u http://127.0.0.1:8899 -w /Users/noahprince/.config/solana/id.json cron-transaction create-remote --url http://localhost:3002/remote --signer $(solana address) --cron-name Noah --index 4
cargo run -p tuktuk-cli -- -u http://127.0.0.1:8899 -w /Users/noahprince/.config/solana/id.json cron-transaction create-remote --url http://localhost:3002/remote --signer $(solana address) --cron-name Noah --index 5
cargo run -p tuktuk-cli -- -u http://127.0.0.1:8899 -w /Users/noahprince/.config/solana/id.json cron-transaction create-remote --url http://localhost:3002/remote --signer $(solana address) --cron-name Noah --index 6

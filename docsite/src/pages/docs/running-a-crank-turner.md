---
title: Running a Crank Turner
description: Learn how to run a crank turner for TukTuk
---


## Install the Crank Turner

```bash
cargo install tuktuk-crank-turner
```

If you want to run a crank turner, create a config.toml file with the following:

```toml
rpc_url = "https://api.mainnet-beta.solana.com"
key_path = "/path/to/your/keypair.json"
min_crank_fee = 10000
```

## Run the Crank Turner
Then run the crank turner:

```bash
tuktuk-crank-turner -c config.toml
```

You can also provider configuration via environment variables

```bash
export TUKTUK__RPC_URL="https://api.mainnet-beta.solana.com"
export TUKTUK__KEY_PATH="/path/to/your/keypair.json"
export TUKTUK__MIN_CRANK_FEE=10000
tuktuk-crank-turner
```

### Protecting your wallet

A task queue can contain arbitrary instructions queued by anyone. The crank turner defends against
tasks that try to spend your keypair's lamports rather than pay you:

- `max_sol_balance_drop` (default `0`) is the most lamports your payer is allowed to lose over a
  simulated transaction, on top of fees. Any bundle that drops your balance by more is discarded and
  never sent. Only raise this if you knowingly run tasks that spend from your wallet.
- `allowed_task_queues` restricts cranking to an explicit list of task queue addresses. When empty,
  every queue is cranked, which means running code queued by anyone. Prefer an explicit list.
- `denied_task_queues` blocks specific queues. Applied after `allowed_task_queues`.

```toml
max_sol_balance_drop = 0
allowed_task_queues = ["<task queue address>"]
denied_task_queues = []
```

### Requirements

You will need a good Solana RPC that doesn't have heavy rate limits (for when there are a lot of tasks queued). You should also handle restarting the process if it crashes, as this can happen if your RPC disconnects the websocket without a proper handshake.
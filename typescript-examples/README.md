# TukTuk TypeScript Examples

This directory contains TypeScript examples showing how to use the TukTuk SDK.

## Table of Contents
- [Prerequisites](#prerequisites)
- [Setup](#setup)
- [Examples](#examples)
  - [Simple Memo Task](#simple-memo-task)
  - [Scheduled Memo Task (Cron)](#scheduled-memo-task-cron)
  - [Token Transfer Task](#token-transfer-task)

## Prerequisites

- Node.js (v16 or higher)
- Solana CLI tools
- A Solana wallet with some SOL for testing

## Setup

1. Install dependencies:
```bash
yarn install
```

2. Create a Solana wallet for testing if you don't have one:
```bash
solana-keygen new --outfile ./wallet.json
```

3. Get some devnet SOL:
```bash
solana airdrop 2 $(solana address -k ./wallet.json) --url devnet
```

## Examples

### Simple Memo Task

The `src/memo.ts` example demonstrates the basics of using TukTuk by queueing a simple memo transaction. This is a great starting point to understand how TukTuk works.

To run the example:

```bash
# Show help and available options
yarn memo -- --help

# Run with specific parameters
yarn memo -- \
  --queueName my-queue \
  --walletPath ./wallet.json \
  --rpcUrl https://api.devnet.solana.com \
  --message "Hello TukTuk!"
```

Required Parameters:
- `--queueName`: Name of the task queue (one will be created if it doesn't exist. NOTE: This will cost 1 sol to create. You can recover this by deleting the queue using the tuktuk-cli)
- `--walletPath`: Path to your Solana wallet keypair file
- `--rpcUrl`: Solana RPC URL (e.g., https://api.devnet.solana.com)

Optional Parameters:
- `--message`: Message to write in the memo (default: "Hello World!")

### Scheduled Memo Task (Cron)

The `src/cron-memo.ts` example shows how to schedule a memo task to run on a regular schedule using TukTuk's cron functionality. In this example, we schedule a memo to be posted every minute.

To run the example:

```bash
# Show help and available options
yarn cron-memo -- --help

# Run with specific parameters
yarn cron-memo -- \
  --cronName my-cron \
  --queueName my-queue \
  --walletPath ./wallet.json \
  --rpcUrl https://api.devnet.solana.com \
  --message "Hello from cron!" \
  --fundingAmount 1000000000
```

Required Parameters:
- `--cronName`: Name of the cron job (must be unique)
- `--queueName`: Name of the task queue to use (one will be created if it doesn't exist)
- `--walletPath`: Path to your Solana wallet keypair file
- `--rpcUrl`: Solana RPC URL (e.g., https://api.devnet.solana.com)

Optional Parameters:
- `--message`: Message to write in the memo (default: "Hello World!")
- `--fundingAmount`: Amount of SOL to fund the cron job with in lamports (default: 1 SOL)

To stop the cron job:
```bash
tuktuk cron close --cron-name my-cron
```

### Token Transfer Task

The `src/token-transfer.ts` example demonstrates how to:
1. Create a custom PDA wallet for a task queue
2. Create a test SPL token
3. Queue a task to transfer tokens from the PDA to another wallet

To run the example:

```bash
# Show help and available options
yarn token-transfer -- --help

# Run with specific parameters
yarn token-transfer -- \
  --queueName my-queue \
  --walletPath ./wallet.json \
  --rpcUrl https://api.devnet.solana.com
```

Required Parameters:
- `--queueName`: Name of the task queue (one will be created if it doesn't exist. NOTE: This will cost 1 sol to create. You can recover this by deleting the queue using the tuktuk-cli)
- `--walletPath`: Path to your Solana wallet keypair file
- `--rpcUrl`: Solana RPC URL (e.g., https://api.devnet.solana.com)

This will:
1. Create a new task queue if it doesn't exist
2. Create a test SPL token mint
3. Create token accounts and mint some tokens
4. Queue a task to transfer the tokens
5. Monitor the task status


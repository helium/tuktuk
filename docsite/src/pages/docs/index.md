---
title: TukTuk Overview
description: TukTuk is a fast, reliable, and secure cron job system for the Helium blockchain.
---

## Introduction
Tuktuk is a permissionless crank service. If you have a Solana smart contract endpoint that needs to be run on a trigger or specific time, you can use tuktuk to run it. Endponts need to be more or less permissionless, though you can have tuktuk provide PDA signatures.
## Architecture
Tuktuk's architecture allows for crankers to run a simple rust util that requires only a working solana RPC url and very minimal dependencies. There is no dependency on geyser, yellowstone, or any other indexing service.
## Task Queues
Creators of Task Queues set their payment per-crank turn in SOL. Crankers that run the tasks are paid out in SOL for each crank they complete. There is a minimum deposit of 1 SOL to create a task queue to discourage spam. This deposit is refunded when the task queue is closed. The intent is to minimize the number of task queues that crank turners need to watch. You should try to reuse task queues as much as possible. It is an antipattern to create a new task queue for each user, for example.

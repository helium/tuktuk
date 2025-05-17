## Closing a Task Queue

You can close a task queue by using the `task-queue close` command. This will refund the 1 SOL deposit and the funding amount.

First, close any queue authorities:

```bash
tuktuk -u <your-solana-url> task-queue remove-queue-authority --task-queue-name <your-queue-name> --queue-authority <your-wallet-address>
```

Then, close the task queue:

```bash
tuktuk -u <your-solana-url> task-queue close --task-queue-name <your-queue-name>
```
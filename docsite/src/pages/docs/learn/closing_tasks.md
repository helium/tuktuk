## Closing Tasks

A task queue has a limited capacity. Therefore, you will want to close tasks that have failed and will never be able to succeed. When you close these tasks, you will be refunded the SOL fees. 

```bash
tuktuk -u <your-solana-url> task close --task-queue-name <your-queue-name> --task-id <task-id>
```

You can also close tasks by prefix using the `--description` flag. This can be useful if you have a lot of tasks in a queue and want to only close specific kinds of tasks.

```bash
tuktuk -u <your-solana-url> task close --task-queue-name <your-queue-name> --description <prefix>
```
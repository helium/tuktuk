## Funding a Task Queue

Generally, tasks are funded by the wallet that creates the task. The only exception is for tasks that schedule more tasks.

If your task queue has any tasks that themselves queue tasks, you will need to keep it funded. This is because the task queue uses its own sol to fund recursively queued tasks. Note that you will not need to fund the task queue immediately if you specified a `funding-amount` in the `create` command.

```bash
tuktuk -u <your-solana-url> task-queue fund --task-queue-name <your-queue-name> --amount 100000000
```

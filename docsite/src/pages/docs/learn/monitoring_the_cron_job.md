## Monitoring the Cron Job

You can list your cron jobs by using the `cron list` command:

```bash
tuktuk -u <your-solana-url> cron list
```

You can get a particular cron job by name using the `cron get` command:

```bash
tuktuk -u <your-solana-url> cron get --cron-name <your-cron-job-name>
```

You can list the transactions in a cron job by using the `cron-transaction list` command:

```bash
tuktuk -u <your-solana-url> cron-transaction list --cron-name <your-cron-job-name>
```

You can delete a cron job by using the `cron close` command. First you must close all cron-transactions in the cron job (for-each id):

```bash
tuktuk -u <your-solana-url> cron-transaction close --cron-name <your-cron-job-name> --id <id>
```

Then you can close the cron job itself:

```bash
tuktuk -u <your-solana-url> cron close --cron-name <your-cron-job-name>
```

### Cron Task Requeuing

Occasionally, a cron task can run out of funding and get removed from the queue. If this happens, the cron will have `removed_from_queue: true` set on its state. This can also happen when you forcefully remove it from the queue via `tuktuk task close`. In this case you should use the `cron requeue` command to requeue the task:

```bash
tuktuk -u <your-solana-url> cron requeue --cron-name <your-cron-job-name>
```

You can always check if your cron is queued by searching your task queue for `"queue <your-cron-job-name>"`:

```bash
tuktuk -u <your-solana-url> task list --task-queue-name <your-queue-name> --description "queue <your-cron-job-name>"
```
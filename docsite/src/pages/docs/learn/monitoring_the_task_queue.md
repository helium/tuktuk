## Monitoring the Task Queue

You can monitor tasks by using the cli:

```bash
tuktuk -u <your-solana-url> task list --task-queue-name <your-queue-name> --description <prefix>
```

The `--description` flag allows you to filter by prefix on the description field of tasks. This can be useful if you have a lot of tasks in a queue and want to only view specific kinds of tasks.

Note that this will only show you tasks that have not been run. Tasks that have been run are closed, with rent refunded to the task creator.

If a task is active but has not yet been run, the cli will display a simulation result for the task. This is to help you debug the task if for some reason it is not running.
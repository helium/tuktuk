## Running a Task

Occasionally, a task could be missed by the tuktuk-crank-turner due to running out of retries. This can happen in cases where the task had a bug, which you later fixed. In this case, when you run `task list` you will see a successful simulation result for the task, but it will not have been run. If task list is taking too long to run, you can use the --skip-simulate flag.

You can run a task by using the `task run` command. This will run the task and mark it as run.

```bash
tuktuk -u <your-solana-url> task run --task-queue-name <your-queue-name> --task-id <task-id>
```

You can also run tasks by prefix using the `--description` flag. This can be useful if you have a lot of tasks in a queue and want to only run specific kinds of tasks.

```bash
tuktuk -u <your-solana-url> task run --task-queue-name <your-queue-name> --description <prefix>
```